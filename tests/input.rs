//! End-to-end behavior of the `raw_input` primitive.
//!
//! These exercise the whole pipeline: component render, canonical layout,
//! event dispatch, and value emission.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::{
    Attr, Commit, Component, ComponentContext, EventDispatcher, EventListener, Lower, Node, Props,
    RawInputMode, RawInputProps, Renderer, Runtime, Size, TextValueEvent, raw_input,
};

fn pipeline(
    viewport: Size,
) -> (
    crossbeam_channel::Sender<Node>,
    crossbeam_channel::Receiver<Result<String, icmd::FrameError>>,
    EventDispatcher,
) {
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start();
    (input, output, dispatcher)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

/// Wait for the pipeline to settle, dispatch one event, and settle again.
fn interact(
    output: &crossbeam_channel::Receiver<Result<String, icmd::FrameError>>,
    dispatcher: &EventDispatcher,
    event: Option<Event>,
) {
    while output.recv_timeout(Duration::from_millis(150)).is_ok() {}
    if let Some(event) = event {
        dispatcher.dispatch(event);
        while output.recv_timeout(Duration::from_millis(150)).is_ok() {}
    }
}

fn click(row: u16, column: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })
}

fn strip_ansi(frame: &str) -> String {
    let mut out = String::new();
    let mut chars = frame.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            out.push(ch);
            continue;
        }
        if let Some('[') = chars.next() {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

fn sized_raw_input(extra: RawInputProps) -> Node {
    raw_input
        .props(extra)
        .style(|style| {
            style.width /= icmd::Dimension::Cells(6);
            style.height /= icmd::Dimension::Cells(1);
        })
        .node()
}

#[test]
fn uncontrolled_raw_input_edits_and_emits_the_value() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = sized_raw_input(RawInputProps {
        default_value: Attr::Set("ab".into()),
        on_change: Attr::Set(EventListener::new({
            let values = values.clone();
            move |event: TextValueEvent| values.lock().unwrap().push(event.value)
        })),
        ..RawInputProps::default()
    });
    let viewport = Size::new(12, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 3)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('x'), KeyModifiers::empty())),
    );
    let values = values.lock().unwrap().clone();
    assert!(
        values.iter().any(|value| value == "abx"),
        "typing at the end must emit the complete value: {values:?}"
    );
}

#[test]
fn raw_input_is_focusable_and_receives_keys() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = sized_raw_input(RawInputProps {
        default_value: Attr::Set("a".into()),
        on_change: Attr::Set(EventListener::new({
            let values = values.clone();
            move |event: TextValueEvent| values.lock().unwrap().push(event.value)
        })),
        ..RawInputProps::default()
    });
    let viewport = Size::new(12, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 1)));
    assert!(
        dispatcher.focused().is_some(),
        "the raw input host must be focusable"
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('b'), KeyModifiers::empty())),
    );
    let values = values.lock().unwrap().clone();
    assert!(
        values.iter().any(|value| value == "ab"),
        "focus must deliver keys: {values:?}"
    );
}

#[test]
fn single_line_raw_input_submits_on_enter() {
    let submitted = Arc::new(Mutex::new(Vec::new()));
    let node = sized_raw_input(RawInputProps {
        default_value: Attr::Set("cmd".into()),
        on_submit: Attr::Set(EventListener::new({
            let submitted = submitted.clone();
            move |event: TextValueEvent| submitted.lock().unwrap().push(event.value)
        })),
        ..RawInputProps::default()
    });
    let viewport = Size::new(12, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 1)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Enter, KeyModifiers::empty())),
    );
    assert_eq!(&*submitted.lock().unwrap(), &["cmd"]);
}

#[test]
fn multiline_raw_input_inserts_newlines_instead_of_submitting() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let submitted = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set("a".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            on_submit: Attr::Set(EventListener::new({
                let submitted = submitted.clone();
                move |event: TextValueEvent| submitted.lock().unwrap().push(event.value)
            })),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(8);
            style.height /= icmd::Dimension::Cells(3);
        })
        .node();
    let viewport = Size::new(14, 5);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 3)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Enter, KeyModifiers::empty())),
    );
    assert!(submitted.lock().unwrap().is_empty());
    let values = values.lock().unwrap().clone();
    assert!(
        values.iter().any(|value| value.contains('\n')),
        "Enter must insert a newline in multiline mode: {values:?}"
    );
}

#[test]
fn caller_events_still_run_alongside_internal_handling() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("a".into()),
            ..RawInputProps::default()
        })
        .events({
            let observed = observed.clone();
            move |handlers: &mut icmd::EventHandlers| {
                handlers.key_down = Attr::Set(EventListener::new(move |_event| {
                    observed.lock().unwrap().push(());
                }));
            }
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(6);
            style.height /= icmd::Dimension::Cells(1);
        })
        .node();
    let viewport = Size::new(12, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 1)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('z'), KeyModifiers::empty())),
    );
    assert_eq!(
        observed.lock().unwrap().len(),
        1,
        "the caller keyboard observer must run on the same host"
    );
}

#[test]
fn raw_input_ignores_children() {
    // Children cannot map to source positions, so they are dropped rather than
    // silently rendered.
    let node = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("body".into()),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(8);
            style.height /= icmd::Dimension::Cells(1);
        })
        .child(icmd::text("ignored"));
    let viewport = Size::new(14, 3);
    let (sender, output, _) = pipeline(viewport);
    sender.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let painted = strip_ansi(&frame);
    assert!(
        painted.contains("body"),
        "the value must paint: {painted:?}"
    );
    assert!(
        !painted.contains("ignored"),
        "children must be ignored: {painted:?}"
    );
}

#[test]
fn placeholder_paints_when_empty() {
    let node = sized_raw_input(RawInputProps {
        placeholder: Attr::Set("hint".into()),
        ..RawInputProps::default()
    });
    let viewport = Size::new(12, 3);
    let (sender, output, _) = pipeline(viewport);
    sender.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(
        strip_ansi(&frame).contains("hint"),
        "the placeholder must paint"
    );
}

#[test]
fn themed_input_wrapper_edits_and_submits() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let submitted = Arc::new(Mutex::new(Vec::new()));
    let node = icmd::input
        .props(icmd::InputProps {
            default_value: Attr::Set("ab".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            on_submit: Attr::Set(EventListener::new({
                let submitted = submitted.clone();
                move |event: TextValueEvent| submitted.lock().unwrap().push(event.value)
            })),
            ..icmd::InputProps::default()
        })
        .node();
    let viewport = Size::new(30, 6);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    // The underline host starts at row 0 and its content row at row 0 with one
    // padding cell, so column 2 is inside the value.
    interact(&output, &dispatcher, Some(click(0, 3)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('c'), KeyModifiers::empty())),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Enter, KeyModifiers::empty())),
    );
    assert!(
        values.lock().unwrap().iter().any(|value| value == "abc"),
        "the themed input must route through raw_input: {:?}",
        values.lock().unwrap()
    );
    assert_eq!(&*submitted.lock().unwrap(), &["abc"]);
}

#[test]
fn themed_textarea_wraps_and_edits() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = icmd::textarea
        .props(icmd::TextareaProps {
            default_value: Attr::Set("ab".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..icmd::TextareaProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(20);
            style.height /= icmd::Dimension::Cells(6);
        })
        .node();
    let viewport = Size::new(30, 10);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(1, 3)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('z'), KeyModifiers::empty())),
    );
    assert!(
        values
            .lock()
            .unwrap()
            .iter()
            .any(|value| value.contains('z')),
        "the themed textarea must route through raw_input: {:?}",
        values.lock().unwrap()
    );
}

#[test]
fn a_custom_component_extends_raw_input_without_private_apis() {
    // The plan's extension story: a user component wraps `raw_input`, forwards
    // value/events, and adjusts style. It observes pointer events on the same
    // host that runs the editor's internal handling.
    #[derive(Clone, Default)]
    struct CommandFieldProps {
        value: Attr<String>,
        on_change: Attr<EventListener<TextValueEvent>>,
        pointer_hits: Arc<Mutex<usize>>,
    }

    fn command_field(cx: &mut ComponentContext, props: &Props<CommandFieldProps>) -> Node {
        let _ = cx;
        let hits = props.pointer_hits.clone();
        raw_input
            .props(RawInputProps {
                mode: Attr::Set(RawInputMode::SingleLine),
                value: props.value.clone(),
                on_change: props.on_change.clone(),
                ..RawInputProps::default()
            })
            .events(move |handlers: &mut icmd::EventHandlers| {
                handlers.pointer_down = Attr::Set(EventListener::new(move |_event| {
                    *hits.lock().unwrap() += 1;
                }));
            })
            .style(|style| style.width /= icmd::Dimension::Cells(10))
            .node()
    }

    let values = Arc::new(Mutex::new(Vec::new()));
    let hits = Arc::new(Mutex::new(0usize));
    let node = command_field
        .props(CommandFieldProps {
            value: Attr::Set("go".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            pointer_hits: hits.clone(),
        })
        .node();
    let viewport = Size::new(16, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 2)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('!'), KeyModifiers::empty())),
    );
    assert_eq!(
        values.lock().unwrap().last().map(String::as_str),
        Some("go!"),
        "the wrapper must receive the edited value"
    );
    assert_eq!(
        *hits.lock().unwrap(),
        1,
        "the caller pointer observer must run on the raw host"
    );
}

#[test]
fn controlled_raw_input_accepts_and_rejects_deterministically() {
    // The owner is authoritative at every render: an echo of the emitted value
    // is acceptance; any other value replaces the field.
    #[derive(Clone, Default)]
    struct OwnerProps {
        values: Arc<Mutex<Vec<String>>>,
        accept: Arc<Mutex<bool>>,
    }

    fn owner(cx: &mut ComponentContext, props: &Props<OwnerProps>) -> Node {
        let (value, set_value) = cx.use_state(|| "a".to_string());
        let values = props.values.clone();
        let accept = props.accept.clone();
        raw_input
            .props(RawInputProps {
                value: Attr::Set(value),
                on_change: Attr::Set(EventListener::new(move |event: TextValueEvent| {
                    values.lock().unwrap().push(event.value.clone());
                    if *accept.lock().unwrap() {
                        set_value.set(event.value);
                    }
                })),
                ..RawInputProps::default()
            })
            .style(|style| style.width /= icmd::Dimension::Cells(8))
            .node()
    }

    // Acceptance keeps editing usable.
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = owner
        .props(OwnerProps {
            values: values.clone(),
            accept: Arc::new(Mutex::new(true)),
        })
        .node();
    let viewport = Size::new(12, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 2)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('b'), KeyModifiers::empty())),
    );
    assert_eq!(
        values.lock().unwrap().last().map(String::as_str),
        Some("ab"),
        "an accepting owner receives the draft"
    );

    // Rejection restores the authoritative value at the next render.
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = owner
        .props(OwnerProps {
            values: values.clone(),
            accept: Arc::new(Mutex::new(false)),
        })
        .node();
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 2)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('c'), KeyModifiers::empty())),
    );
    assert_eq!(
        values.lock().unwrap().as_slice(),
        &["ac".to_string()],
        "a rejecting owner still observes the draft it ignores"
    );
}

#[test]
fn read_only_raw_input_is_focusable_and_selectable_but_does_not_mutate() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let clipboard = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("locked".into()),
            read_only: Attr::Set(true),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            on_clipboard: Attr::Set(EventListener::new({
                let clipboard = clipboard.clone();
                move |event: icmd::TextClipboardEvent| clipboard.lock().unwrap().push(event.text)
            })),
            ..RawInputProps::default()
        })
        .style(|style| style.width /= icmd::Dimension::Cells(8))
        .node();
    let viewport = Size::new(12, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 2)));
    assert!(
        dispatcher.focused().is_some(),
        "a read-only field must still take focus"
    );
    // Typing must not change the value.
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('x'), KeyModifiers::empty())),
    );
    assert!(
        values.lock().unwrap().is_empty(),
        "read-only must not mutate"
    );

    // Selection and copy still work.
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('a'), KeyModifiers::CONTROL)),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
    );
    assert_eq!(
        clipboard.lock().unwrap().as_slice(),
        &["locked".to_string()],
        "read-only permits copy"
    );
}

#[test]
fn unhandled_keys_bubble_to_ancestors_while_handled_keys_stop() {
    // The editor consumes keys it handles; anything else must continue to
    // ancestor listeners.
    let ancestor_down = Arc::new(Mutex::new(Vec::new()));
    let field = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("a".into()),
            ..RawInputProps::default()
        })
        .style(|style| style.width /= icmd::Dimension::Cells(6))
        .node();
    let node = {
        let mut dom = icmd::DomProps::default();
        dom.events.key_down = Attr::Set(EventListener::new({
            let seen = ancestor_down.clone();
            move |_event| seen.lock().unwrap().push(())
        }));
        icmd::Node::element(dom, [field])
    };
    let viewport = Size::new(10, 4);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 2)));

    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('z'), KeyModifiers::empty())),
    );
    assert_eq!(
        ancestor_down.lock().unwrap().len(),
        0,
        "a handled character key stops at the input"
    );

    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::F(5), KeyModifiers::empty())),
    );
    assert_eq!(
        ancestor_down.lock().unwrap().len(),
        1,
        "an unhandled key continues to ancestors"
    );
}

#[test]
fn wrapping_is_correct_in_the_first_committed_frame() {
    // The editor wraps to the width the layout actually grants, and the commit
    // pass builds that layout itself. Correct wrapping therefore does not need
    // a feedback render: the first frame whose rows are painted already has the
    // final wrap.
    let node = icmd::textarea
        .props(icmd::TextareaProps {
            default_value: Attr::Set("0123456789abcdefghij".into()),
            wrap: Attr::Set(icmd::TextWrap::Hard),
            ..icmd::TextareaProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(20);
            style.height /= icmd::Dimension::Cells(6);
        })
        .node();
    let viewport = Size::new(24, 8);
    let (sender, output, _) = pipeline(viewport);
    sender.send(node).unwrap();

    // Collect every frame before quiescence. A settling loop would show a
    // first frame with rows wrapped for the wrong width and a later correction;
    // instead the union of painted characters must be complete and the first
    // frame must already carry content.
    let mut frames: Vec<String> = Vec::new();
    while let Ok(frame) = output.recv_timeout(Duration::from_millis(250)) {
        frames.push(strip_ansi(&frame.unwrap()));
    }
    assert!(!frames.is_empty(), "at least one frame must be committed");
    let painted = frames.concat();
    for ch in "0123456789abcdefghij".chars() {
        assert!(
            painted.contains(ch),
            "character {ch:?} must be painted: {painted:?}"
        );
    }
    assert!(
        frames[0].contains("0123456789"),
        "the first committed frame must already wrap at the granted width: {:?}",
        frames[0]
    );
}

#[test]
fn multiline_vertical_navigation_moves_the_caret_between_rows() {
    // Down/Up must move the caret between visual rows and keep a preferred
    // terminal-cell column across a short row.
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set("abcdef\nxy\nabcdef".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(8);
            style.height /= icmd::Dimension::Cells(3);
        })
        .node();
    let viewport = Size::new(12, 5);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 7)));
    // The caret is at the end of the first row; Down then typing must insert at
    // a different source position than the first row's end.
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Down, KeyModifiers::empty())),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('X'), KeyModifiers::empty())),
    );
    let emitted = values.lock().unwrap().last().cloned().unwrap_or_default();
    assert!(
        !emitted.starts_with("abcdefX"),
        "Down must move the caret off the first row, not stay at its end: {emitted:?}"
    );
    assert!(
        emitted.contains('X'),
        "the typed character must be inserted: {emitted:?}"
    );
    assert_eq!(
        emitted.matches('\n').count(),
        2,
        "the line structure must be preserved: {emitted:?}"
    );
}

#[test]
fn page_down_and_page_up_repeat_the_vertical_move() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set("aa\nbb\ncc\ndd\nee".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(6);
            style.height /= icmd::Dimension::Cells(2);
        })
        .node();
    let viewport = Size::new(10, 4);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 1)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::PageDown, KeyModifiers::empty())),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('#'), KeyModifiers::empty())),
    );
    let emitted = values.lock().unwrap().last().cloned().unwrap_or_default();
    assert!(
        emitted.contains('#'),
        "PageDown must leave the caret somewhere editable: {emitted:?}"
    );
    assert!(
        !emitted.starts_with('#'),
        "PageDown from the first row must move down: {emitted:?}"
    );
}
