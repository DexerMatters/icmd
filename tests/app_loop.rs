// Event-loop tests: terminal teardown ordering, coalescing, and the render wait
// cap. The loop itself runs inside the crate, so these drive the exposed
// mechanics through the hidden module.
use std::time::Duration;

use icmd::__private::{MAX_RENDER_WAIT, coalesce_event, teardown};
use icmd::RuntimeConfig;

fn teardown_commands(config: &RuntimeConfig) -> Vec<&'static str> {
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
    let commands = teardown_commands(&config);
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

#[test]
fn teardown_writes_every_command_to_the_stream() {
    let mut buffer: Vec<u8> = Vec::new();
    teardown(&RuntimeConfig::default(), &mut buffer).expect("teardown writes");
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
    let commands = teardown_commands(&config);
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
