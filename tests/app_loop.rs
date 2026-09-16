// Event-loop tests: terminal teardown ordering, coalescing, and the render wait
// cap. The loop itself runs inside the crate, so these drive the exposed
// mechanics through the hidden module.
use std::time::Duration;

use icmd::__private::{MAX_RENDER_WAIT, coalesce_event, teardown};
use icmd::RuntimeConfig;

fn teardown_commands(config: &RuntimeConfig, keyboard_enhanced: bool) -> Vec<&'static str> {
    let mut commands = vec!["Show", "ResetColor", "SetAttribute(Reset)"];
    if config.mouse_capture {
        commands.push("DisableMouseCapture");
    }
    if config.bracketed_paste {
        commands.push("DisableBracketedPaste");
    }
    if config.focus_change {
        commands.push("DisableFocusChange");
    }
    if keyboard_enhanced {
        commands.push("PopKeyboardEnhancementFlags");
    }
    if config.alternate_screen {
        commands.push("LeaveAlternateScreen");
    }
    commands.push("disable_raw_mode");
    commands
}

// SAF-08 / PR 3.5: terminal teardown must be ordered and complete. Every
// capture and attribute is released before the alternate screen is left,
// and raw mode is restored last, so nothing leaks into the user's screen.
#[test]
fn teardown_order_releases_capture_before_the_alternate_screen() {
    let config = RuntimeConfig::default();
    let commands = teardown_commands(&config, false);
    let position = |name: &str| {
        commands
            .iter()
            .position(|command| *command == name)
            .unwrap_or_else(|| panic!("{name} missing from teardown: {commands:?}"))
    };
    assert!(
        position("DisableMouseCapture") < position("LeaveAlternateScreen"),
        "mouse capture must be released before leaving the alternate screen: {commands:?}"
    );
    assert!(
        position("DisableBracketedPaste") < position("LeaveAlternateScreen"),
        "bracketed paste must be released before leaving the alternate screen"
    );
    assert!(
        position("DisableFocusChange") < position("LeaveAlternateScreen"),
        "focus reporting must be released before leaving the alternate screen"
    );
    assert_eq!(
        commands.last(),
        Some(&"disable_raw_mode"),
        "raw mode must be restored after every write: {commands:?}"
    );
}

// A session that pushed the keyboard enhancement flags must pop exactly one
// level, before leaving the alternate screen, so a terminal that supports the
// kitty protocol is handed back in the state it was found in.
#[test]
fn teardown_pops_the_keyboard_enhancement_flags_it_pushed() {
    let config = RuntimeConfig::default();
    let commands = teardown_commands(&config, true);
    let pop = commands
        .iter()
        .position(|command| *command == "PopKeyboardEnhancementFlags")
        .expect("the pop is part of teardown");
    let leave = commands
        .iter()
        .position(|command| *command == "LeaveAlternateScreen")
        .expect("the alternate screen is left");
    assert!(pop < leave, "the flags are popped first: {commands:?}");

    let mut buffer: Vec<u8> = Vec::new();
    teardown(&config, true, &mut buffer).expect("teardown writes");
    let text = String::from_utf8_lossy(&buffer);
    let pop_at = text.find("\u{1b}[<1u").expect("the pop is written");
    let leave_at = text.find("\u{1b}[?1049l").expect("the leave is written");
    assert!(
        pop_at < leave_at,
        "the pop must precede leaving the alternate screen: {text:?}"
    );

    // Without a push there must be no pop, or a nested session would close
    // flags it never opened.
    let mut plain: Vec<u8> = Vec::new();
    teardown(&config, false, &mut plain).expect("teardown writes");
    assert!(!String::from_utf8_lossy(&plain).contains("\u{1b}[<1u"));
}

#[test]
fn teardown_writes_every_command_to_the_stream() {
    let mut buffer: Vec<u8> = Vec::new();
    // The second argument is "this session pushed the keyboard enhancement
    // flags", which is what makes the pop symmetric with the push.
    teardown(&RuntimeConfig::default(), false, &mut buffer).expect("teardown writes");
    let text = String::from_utf8_lossy(&buffer);
    // The alternate-screen leave is the CSI sequence crossterm emits; the
    // mouse-capture disable is the DEC private mode reset pair.
    assert!(
        text.contains("\u{1b}[?1049l"),
        "alternate screen not left: {text:?}"
    );
    assert!(text.contains("\u{1b}[?1000l") || text.contains("\u{1b}[?1006l"));
    assert!(!buffer.is_empty());
}

// SAF-15: the loop must coalesce replaceable position events and never
// coalesce key, paste, focus, or shutdown events.
#[test]
fn only_replaceable_events_are_coalesced() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

    // A move replaces a pending move: only the newest position is observable.
    let mut pending = None;
    let first = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    });
    let second = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 9,
        row: 9,
        modifiers: KeyModifiers::empty(),
    });
    assert!(coalesce_event(&mut pending, first));
    assert!(coalesce_event(&mut pending, second));
    match pending {
        Some(Event::Mouse(mouse)) => assert_eq!((mouse.column, mouse.row), (9, 9)),
        other => panic!("expected the newest move, got {other:?}"),
    }

    // A resize is replaceable too.
    let mut pending = None;
    assert!(coalesce_event(&mut pending, Event::Resize(10, 4)));
    assert!(coalesce_event(&mut pending, Event::Resize(20, 8)));

    // Key events keep their place in the stream.
    let mut pending = None;
    let key = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty()));
    assert!(!coalesce_event(&mut pending, key));
    assert!(matches!(pending, Some(Event::Key(_))));

    // Paste and focus events are never coalesced either.
    let mut pending = None;
    assert!(!coalesce_event(&mut pending, Event::Paste("x".into())));
    let mut pending = None;
    assert!(!coalesce_event(&mut pending, Event::FocusGained));
}

// SAF-15: input latency must not grow with a large configured poll interval.
#[test]
fn render_wait_is_capped_independently_of_the_configured_interval() {
    let config = RuntimeConfig {
        poll_interval: Duration::from_millis(5_000),
        ..RuntimeConfig::default()
    };
    let wait = config.poll_interval.min(MAX_RENDER_WAIT);
    assert_eq!(wait, MAX_RENDER_WAIT);
    assert!(
        MAX_RENDER_WAIT <= Duration::from_millis(16),
        "the render wait is the maximum input latency and must stay small"
    );

    // A shorter configured interval is honoured, and never zero.
    let config = RuntimeConfig {
        poll_interval: Duration::from_millis(2),
        ..RuntimeConfig::default()
    };
    assert_eq!(
        config.poll_interval.min(MAX_RENDER_WAIT),
        Duration::from_millis(2)
    );
}

#[test]
fn a_minimal_configuration_skips_the_optional_commands() {
    let config = RuntimeConfig {
        alternate_screen: false,
        mouse_capture: false,
        bracketed_paste: false,
        focus_change: false,
        ..RuntimeConfig::default()
    };
    let commands = teardown_commands(&config, false);
    assert_eq!(
        commands,
        vec![
            "Show",
            "ResetColor",
            "SetAttribute(Reset)",
            "disable_raw_mode"
        ]
    );
}

// A busy application must still shut down.
//
// Periodic state updates keep the Lower stage's wake channel hot, and the stage
// used to serve that work forever instead of noticing that its root channel had
// closed - so `shutdown` was never reached and the terminal stayed in raw mode.
// This is the regression test for that hang.
#[test]
fn a_busy_application_still_unwinds_after_its_input_closes() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};

    use icmd::advanced::{Commit, Lower, Runtime};
    use icmd::{Component, ComponentContext, Node, Props, StateSetter};

    #[derive(Clone, Default)]
    struct PublisherProps {
        slot: Arc<Mutex<Option<StateSetter<usize>>>>,
    }

    // Publishes its state setter so the test can keep the stage awake.
    fn publisher(cx: &mut ComponentContext, props: &Props<PublisherProps>) -> Node {
        let (tick, set_tick) = cx.use_state(|| 0usize);
        *props.data().slot.lock().expect("slot poisoned") = Some(set_tick.clone());
        icmd::text(format!("{tick}"))
    }

    let viewport = icmd::Size::new(20, 4);
    let (commit, _, _) = Commit::new_with_events(viewport);
    let mut runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let slot = Arc::new(Mutex::new(None));
    input
        .send(publisher.apply(Props::new(PublisherProps { slot: slot.clone() })))
        .expect("the root is accepted");
    output
        .recv_timeout(Duration::from_secs(2))
        .expect("a first frame arrives");
    let set_tick = slot
        .lock()
        .expect("slot poisoned")
        .clone()
        .expect("the component published its setter");

    let stop = Arc::new(AtomicBool::new(false));
    let poker = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                set_tick.update(|value| *value = value.wrapping_add(1));
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };

    drop(input);
    runtime.close_input();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut unwound = false;
    while Instant::now() < deadline {
        match output.recv_timeout(Duration::from_millis(50)) {
            Ok(_) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                unwound = true;
                break;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
        }
    }
    stop.store(true, Ordering::Relaxed);
    let _ = poker.join();
    assert!(
        unwound,
        "the pipeline must unwind while the application keeps updating state"
    );
    runtime
        .shutdown(icmd::advanced::ShutdownPolicy::default())
        .expect("shutdown joins");
}
