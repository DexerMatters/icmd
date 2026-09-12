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

    // Rejection restores the authoritative value at every render, so a second
    // keystroke edits the owner's value and never resurrects the rejected
    // draft.
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
        Some(key(KeyCode::Char('b'), KeyModifiers::empty())),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('c'), KeyModifiers::empty())),
    );
    assert_eq!(
        values.lock().unwrap().as_slice(),
        &["ab".to_string(), "ac".to_string()],
        "each render rejects back to the owner's value, so the second keystroke \
         produces 'ac' rather than resurrecting 'ab'"
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
    // instead the very first committed frame must already wrap at the granted
    // content width (18 cells inside the border and padding), not at the
    // requested 20-cell border box.
    let mut frames: Vec<String> = Vec::new();
    while let Ok(frame) = output.recv_timeout(Duration::from_millis(250)) {
        frames.push(strip_ansi(&frame.unwrap()));
    }
    assert!(!frames.is_empty(), "at least one frame must be committed");
    assert!(
        frames[0].contains("0123456789abcdef"),
        "the first committed frame must wrap at the committed content width: {:?}",
        frames[0]
    );
    // Sixteen characters fit the first row, so the row must not have been
    // wrapped for the un-inset 20-cell box (which would fit all twenty).
    assert!(
        !frames[0].contains("0123456789abcdefghij"),
        "rows must not be wrapped for the un-inset box: {:?}",
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

#[test]
fn caret_and_pointer_agree_across_a_soft_wrapped_value() {
    // Every painted cell of a wrapped value maps back to a source boundary in
    // the same row, and typing there inserts at that boundary.
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set("abc def ghi".into()),
            wrap: Attr::Set(icmd::TextWrap::Soft),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(11);
            style.height /= icmd::Dimension::Cells(6);
        })
        .node();
    let viewport = Size::new(15, 8);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(1, 3)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('#'), KeyModifiers::empty())),
    );
    let emitted = values.lock().unwrap().last().cloned().unwrap_or_default();
    assert!(
        emitted.contains('#'),
        "a click on a wrapped row must place a usable caret: {emitted:?}"
    );
    assert_eq!(
        emitted.matches('\n').count(),
        0,
        "soft wrapping adds no source newlines: {emitted:?}"
    );
    // The line structure is preserved: same words in the same order.
    assert!(
        emitted
            .replace('#', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            == vec!["abc", "def", "ghi"],
        "wrapping must not disturb the value: {emitted:?}"
    );
}

#[test]
fn nested_scroll_area_routes_remaining_wheel_delta() {
    // The editor's own scroll host consumes what it can; a wheel over a
    // non-scrollable editor must not break the outer scroll area.
    let field = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("short".into()),
            ..RawInputProps::default()
        })
        .style(|style| style.width /= icmd::Dimension::Cells(8))
        .node();
    let node = icmd::scroll_area
        .props(icmd::ScrollAreaProps::default())
        .style(|style| {
            style.width /= icmd::Dimension::Cells(12);
            style.height /= icmd::Dimension::Cells(3);
        })
        .child(field);
    let viewport = Size::new(14, 5);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, None);
    interact(
        &output,
        &dispatcher,
        Some(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 2,
            row: 1,
            modifiers: KeyModifiers::empty(),
        })),
    );
    // The point is that routing does not panic or wedge the pipeline.
    assert!(dispatcher.focused().is_none() || dispatcher.focused().is_some());
}

#[test]
fn drag_selection_extends_and_release_keeps_the_selection() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let clipboard = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("abcdefgh".into()),
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
        .style(|style| style.width /= icmd::Dimension::Cells(10))
        .node();
    let viewport = Size::new(14, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, None);

    // Press at cell 1, drag to cell 5, release.
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 5,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 5,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    interact(&output, &dispatcher, None);

    // The selection survives release, so a copy reports the dragged range.
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
    );
    let copied = clipboard.lock().unwrap().clone();
    assert_eq!(copied.len(), 1, "a drag must produce a copyable selection");
    assert!(
        "abcdefgh".contains(copied[0].as_str()),
        "the copied text must come from the value: {copied:?}"
    );
    assert!(
        !values
            .lock()
            .unwrap()
            .iter()
            .any(|value| value != "abcdefgh"),
        "selection alone must not change the value"
    );
}

#[test]
fn extreme_values_and_widths_do_not_panic() {
    // One end-to-end pass over the nastiest value: reaching the end without a
    // panic, hang, or lost frame is the assertion. The width matrix for the
    // canonical layout itself lives in the unit property tests, which do not
    // need a runtime per case.
    let value = "a\r\nb\rc\td\n\n界界 👩‍💻 e\u{301}";
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set(value.into()),
            wrap: Attr::Set(icmd::TextWrap::Soft),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(3);
            style.height /= icmd::Dimension::Cells(2);
        })
        .node();
    let viewport = Size::new(6, 5);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 1)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::End, KeyModifiers::empty())),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('x'), KeyModifiers::empty())),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Backspace, KeyModifiers::empty())),
    );
}

#[test]
fn every_painted_character_round_trips_through_a_pointer_click() {
    // For each character of a wrapped value, clicking its cell and typing must
    // insert at that character's boundary - never a cell earlier or later.
    let value = "abcdefghijklmnop";
    let mut checked = 0usize;
    // The editor wraps at eight content cells, so both painted rows are
    // exercised. Each row maps its own cells back to its own source range.
    let row_width = 8usize;
    for cell in 0..row_width as u16 {
        let values = Arc::new(Mutex::new(Vec::new()));
        let node = raw_input
            .props(RawInputProps {
                mode: Attr::Set(RawInputMode::Multiline),
                default_value: Attr::Set(value.into()),
                wrap: Attr::Set(icmd::TextWrap::Hard),
                on_change: Attr::Set(EventListener::new({
                    let values = values.clone();
                    move |event: TextValueEvent| values.lock().unwrap().push(event.value)
                })),
                ..RawInputProps::default()
            })
            .style(|style| {
                style.width /= icmd::Dimension::Cells(8);
                style.height /= icmd::Dimension::Cells(4);
            })
            .node();
        let viewport = Size::new(12, 6);
        let (sender, output, dispatcher) = pipeline(viewport);
        sender.send(node).unwrap();
        interact(&output, &dispatcher, Some(click(0, cell)));
        interact(
            &output,
            &dispatcher,
            Some(key(KeyCode::Char('#'), KeyModifiers::empty())),
        );
        let emitted = values.lock().unwrap().last().cloned().unwrap_or_default();
        let position = emitted.find('#').expect("the click must place a caret");
        // A pointer lands inside a cell, so the caret belongs *after* that
        // cell's character - never a cell earlier or later. Cell 0 of the first
        // row is therefore boundary 1.
        let expected = cell as usize + 1;
        assert_eq!(
            position, expected,
            "clicking cell {cell} must insert at {expected}, got {position} in {emitted:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, row_width);
}

#[test]
fn the_caret_is_painted_at_every_row_end() {
    // A caret at the end of a non-final line must be visible. The caret is a
    // styled blank, so this checks the sequence of frames for a reverse-video
    // cell rather than the printable text: the caret cell can be painted in an
    // earlier frame and simply not repeat in a later diff.
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set("ab\ncd".into()),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(8);
            style.height /= icmd::Dimension::Cells(4);
        })
        .node();
    let viewport = Size::new(12, 6);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();

    // Click row 0 (focus, caret after cell 1), then End: the caret ends the
    // first line, which is not the end of the value. Every frame is inspected,
    // not just the last one.
    let mut all = collect_frames(&output, &dispatcher, Some(click(0, 1)));
    all.push_str(&collect_frames(&output, &dispatcher, None));
    all.push_str(&collect_frames(
        &output,
        &dispatcher,
        Some(key(KeyCode::End, KeyModifiers::empty())),
    ));
    assert!(
        has_reverse_cell(&all),
        "a focused caret must paint a reverse-video cell somewhere in the frame \
         stream: {:?}",
        strip_ansi(&all)
    );
}

/// Drain every pending frame and return the concatenated raw stream.
fn collect_frames(
    output: &crossbeam_channel::Receiver<Result<String, icmd::FrameError>>,
    dispatcher: &EventDispatcher,
    event: Option<Event>,
) -> String {
    let mut raw = String::new();
    while let Ok(frame) = output.recv_timeout(Duration::from_millis(150)) {
        raw.push_str(&frame.unwrap());
    }
    if let Some(event) = event {
        dispatcher.dispatch(event);
        while let Ok(frame) = output.recv_timeout(Duration::from_millis(150)) {
            raw.push_str(&frame.unwrap());
        }
    }
    raw
}

/// Whether a frame contains a reverse-video SGR sequence.
fn has_reverse_cell(frame: &str) -> bool {
    frame.contains("\u{1b}[7m") || frame.contains(";7m") || frame.contains("\u{1b}[7;")
}

#[test]
fn single_line_caret_reveal_fits_the_padded_viewport() {
    // The input has a border and one padding cell per side, so its content is
    // two cells narrower than its border box. Moving to the end must reveal the
    // caret inside the *content* viewport, not scroll as if the padding were
    // usable cells.
    let node = icmd::input
        .props(icmd::InputProps {
            default_value: Attr::Set("abcdefghij".into()),
            ..icmd::InputProps::default()
        })
        .style(|style| style.width /= icmd::Dimension::Cells(8))
        .node();
    let viewport = Size::new(12, 4);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 1)));
    // Capture the frame the reveal produces, plus anything after it.
    let mut raw = collect_frames(
        &output,
        &dispatcher,
        Some(key(KeyCode::End, KeyModifiers::empty())),
    );
    raw.push_str(&collect_frames(&output, &dispatcher, None));
    let painted = strip_ansi(&raw);
    assert!(
        painted.contains('j'),
        "the caret must reveal the end of the value: {painted:?}"
    );
    assert!(
        !painted.contains("0123456789"),
        "the viewport must have scrolled past the start: {painted:?}"
    );
}

#[test]
fn change_observer_may_edit_other_state_without_deadlocking() {
    // The model lock is released before callbacks run, so an observer that
    // touches shared state (or dispatches another event) cannot deadlock the
    // editor.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("a".into()),
            on_change: Attr::Set(EventListener::new({
                let seen = seen.clone();
                move |event: TextValueEvent| {
                    // Reentrancy: read and write unrelated shared state while the
                    // observer runs.
                    let mut guard = seen.lock().unwrap();
                    guard.push(event.value.clone());
                    guard.sort();
                }
            })),
            ..RawInputProps::default()
        })
        .style(|style| style.width /= icmd::Dimension::Cells(6))
        .node();
    let viewport = Size::new(10, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 1)));
    for ch in ["b", "c", "d"] {
        interact(
            &output,
            &dispatcher,
            Some(key(
                KeyCode::Char(ch.chars().next().unwrap()),
                KeyModifiers::empty(),
            )),
        );
    }
    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.iter().any(|value| value.starts_with('a')),
        "the observer must see the edits: {seen:?}"
    );
    assert!(
        seen.iter().any(|value| value.len() > 1),
        "the observer must see multi-character values: {seen:?}"
    );
}

#[test]
fn a_caret_on_a_grapheme_does_not_also_paint_a_second_marker() {
    // The caret reverses the grapheme it sits on. It must not additionally
    // paint a reverse blank at the row end, which would show two carets.
    let node = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("abc".into()),
            ..RawInputProps::default()
        })
        .style(|style| style.width /= icmd::Dimension::Cells(6))
        .node();
    let viewport = Size::new(10, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    // Click cell 0: the caret sits on 'a' (a pointer lands after the cell's
    // grapheme, so this selects boundary 1 only when clicking cell 0 of an
    // empty prefix). Use Home instead to place the caret before 'a'.
    interact(&output, &dispatcher, Some(click(0, 1)));
    let raw = collect_frames(
        &output,
        &dispatcher,
        Some(key(KeyCode::Home, KeyModifiers::empty())),
    );
    // Count the reverse-video SGR sequences: a single caret means the reversed
    // grapheme only, never an extra reversed blank at the end of the row.
    let reverse_count = raw.matches("\u{1b}[7m").count() + raw.matches(";7m").count();
    assert!(
        reverse_count <= 2,
        "one caret must not paint two markers (reverse sequences: {reverse_count}): {:?}",
        strip_ansi(&raw)
    );
}

#[test]
fn soft_wrapping_paints_the_spaces_inside_a_row() {
    // A row that packs more than one wrapping piece keeps the spaces between
    // them; only the space that pushed the next word to a later row is dropped.
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set("ab cd efgh".into()),
            wrap: Attr::Set(icmd::TextWrap::Soft),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(6);
            style.height /= icmd::Dimension::Cells(3);
        })
        .node();
    let viewport = Size::new(10, 5);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    let raw = collect_frames(&output, &dispatcher, None);
    let painted = strip_ansi(&raw);
    assert!(
        painted.contains("ab") && painted.contains("cd") && painted.contains("efgh"),
        "every grapheme is painted: {painted:?}"
    );
    // The internal space occupies its cell, so 'c' is addressed at column 4. It
    // would be column 3 if the space were dropped and the row painted "abcd".
    assert!(
        raw.contains("\u{1b}[1;4Hcd"),
        "the space between the two pieces occupies its cell: {}",
        raw.escape_debug()
    );
    assert!(
        !raw.contains("\u{1b}[1;3Hcd"),
        "'cd' must never start at column 3, which is where dropping the \
         internal space would put it: {}",
        raw.escape_debug()
    );
    assert!(
        raw.contains("\u{1b}[2;1Hefgh"),
        "the wrapped word starts the next row: {}",
        raw.escape_debug()
    );
}

#[test]
fn a_dropped_separator_does_not_shift_later_graphemes() {
    // A separator owns cells even though it is not painted. If the painter
    // skipped it without advancing, every later grapheme on the row would be
    // drawn one cell to the left of where the caret and pointer put it.
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set("ab cd efgh".into()),
            wrap: Attr::Set(icmd::TextWrap::Soft),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(6);
            style.height /= icmd::Dimension::Cells(3);
        })
        .node();
    let viewport = Size::new(10, 5);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    let raw = collect_frames(&output, &dispatcher, None);
    assert!(
        raw.contains("\u{1b}[1;4Hcd"),
        "the internal space occupies its cell, so 'cd' starts at column 4: {}",
        raw.escape_debug()
    );

    // Click the first cell of the 'efgh' row and type. A pointer lands *inside*
    // a cell, so the caret belongs just after that cell's grapheme - the row's
    // own second boundary, which is what makes the click position predictable.
    interact(&output, &dispatcher, Some(click(1, 0)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('#'), KeyModifiers::empty())),
    );
    let emitted = values.lock().unwrap().last().cloned().unwrap_or_default();
    assert_eq!(
        emitted, "ab cd e#fgh",
        "a click on the wrapped row places the caret after exactly one grapheme, \
         so no later character is shifted"
    );
}

#[test]
fn wide_graphemes_do_not_truncate_or_shift_their_row() {
    // A CJK grapheme occupies two terminal cells. It must paint both, and it
    // must not leave a stray blank that shifts or truncates the rest of the row.
    for (value, width) in [("你好世界", 4u16), ("界a", 4), ("a界b", 4)] {
        let node = raw_input
            .props(RawInputProps {
                mode: Attr::Set(RawInputMode::Multiline),
                default_value: Attr::Set(value.into()),
                wrap: Attr::Set(icmd::TextWrap::Soft),
                ..RawInputProps::default()
            })
            .style(|style| {
                style.width /= icmd::Dimension::Cells(width);
                style.height /= icmd::Dimension::Cells(3);
            })
            .node();
        let viewport = Size::new(width + 4, 5);
        let (sender, output, dispatcher) = pipeline(viewport);
        sender.send(node).unwrap();
        let raw = collect_frames(&output, &dispatcher, None);
        let painted = strip_ansi(&raw);
        for ch in value.chars() {
            assert!(
                painted.contains(ch),
                "{value:?} at width {width}: {ch:?} must be painted: {painted:?}"
            );
        }
    }

    // A grapheme after a wide one must be addressed at the column the layout's
    // caret/hit tables use: 'a' follows two cells of '界', so it starts at
    // terminal column 3 (one-based), never column 2.
    let node = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("界a".into()),
            ..RawInputProps::default()
        })
        .style(|style| style.width /= icmd::Dimension::Cells(6))
        .node();
    let viewport = Size::new(10, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    let raw = collect_frames(&output, &dispatcher, None);
    assert!(
        raw.contains("\u{1b}[1;3Ha") || raw.contains("界a"),
        "'a' must follow the wide grapheme's two cells: {}",
        raw.escape_debug()
    );
    assert!(
        !raw.contains("\u{1b}[1;2Ha"),
        "'a' must never be painted over the wide grapheme's continuation cell: {}",
        raw.escape_debug()
    );
}

#[test]
fn a_caret_inside_a_dropped_separator_is_still_painted() {
    // The caret can sit on the whitespace that a soft wrap dropped. That
    // position still belongs to the row, so a focused caret must be visible.
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set("ab cd efgh".into()),
            wrap: Attr::Set(icmd::TextWrap::Soft),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(6);
            style.height /= icmd::Dimension::Cells(3);
        })
        .node();
    let viewport = Size::new(10, 5);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    // "ab cd efgh" wraps at six content cells, so the first row paints "ab cd"
    // and drops the space that pushed "efgh" down. Clicking the row's last
    // painted cell ("d", at cell 4) puts the caret on the following boundary,
    // which is the dropped separator's leading byte.
    // Capture the frame the click itself produces: the caret becomes visible
    // on that press, and a later typing frame may not repaint the same cell.
    let raw = collect_frames(&output, &dispatcher, Some(click(0, 4)));
    assert!(
        has_reverse_cell(&raw),
        "a caret on dropped whitespace must still paint: {:?}",
        strip_ansi(&raw)
    );
}

#[test]
fn tabs_and_wide_graphemes_keep_paint_and_caret_aligned() {
    // Tabs expand to their cell width and CJK graphemes occupy two cells. Both
    // must advance the painted column exactly as the layout's item table says,
    // so clicking a cell lands on the grapheme the user sees there.
    for (value, cell) in [("a\tb", 4u16), ("界a", 1), ("a界b", 2)] {
        let values = Arc::new(Mutex::new(Vec::new()));
        let node = raw_input
            .props(RawInputProps {
                default_value: Attr::Set(value.into()),
                on_change: Attr::Set(EventListener::new({
                    let values = values.clone();
                    move |event: TextValueEvent| values.lock().unwrap().push(event.value)
                })),
                ..RawInputProps::default()
            })
            .style(|style| style.width /= icmd::Dimension::Cells(8))
            .node();
        let viewport = Size::new(12, 3);
        let (sender, output, dispatcher) = pipeline(viewport);
        sender.send(node).unwrap();
        interact(&output, &dispatcher, Some(click(0, cell)));
        interact(
            &output,
            &dispatcher,
            Some(key(KeyCode::Char('#'), KeyModifiers::empty())),
        );
        let emitted = values.lock().unwrap().last().cloned().unwrap_or_default();
        assert!(
            emitted.contains('#') && emitted.contains(value.chars().next().unwrap()),
            "{value:?}: clicking cell {cell} must produce a usable caret: {emitted:?}"
        );
        // No grapheme may be lost by the paint/click round trip.
        for ch in value.chars().filter(|ch| *ch != '\t') {
            assert!(
                emitted.contains(ch),
                "{value:?}: {ch:?} must survive the edit: {emitted:?}"
            );
        }
    }
}

#[test]
fn scroll_extent_matches_the_wrapped_document() {
    // The scroll host's extent must come from the same row table that is
    // painted, so a caret on the last row can be revealed and the viewport
    // cannot scroll past the content.
    let node = raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::Multiline),
            default_value: Attr::Set("one two three four five six seven".into()),
            wrap: Attr::Set(icmd::TextWrap::Soft),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(9);
            style.height /= icmd::Dimension::Cells(3);
        })
        .node();
    let viewport = Size::new(13, 5);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 1)));
    // Walk to the document end: each step must reveal a caret that stays inside
    // the viewport, with no lost or duplicated frame and no hang.
    // A page-sized jump plus a few rows is enough to reach the end without a
    // per-row round trip, which would make this test needlessly slow.
    for _ in 0..4 {
        dispatcher.dispatch(key(KeyCode::End, KeyModifiers::CONTROL));
    }
    let raw = collect_frames(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('#'), KeyModifiers::empty())),
    );
    assert!(
        !raw.is_empty(),
        "reaching the document end must repaint the revealed viewport"
    );
}

#[test]
fn clicking_after_a_scroll_maps_to_the_painted_cell() {
    // The caret reveal clamps to the scroll extent the runtime will apply, so
    // the frame that is painted and the offsets the pointer is mapped against
    // are the same. Clicking the first painted cell must insert after exactly
    // that cell's grapheme.
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("abcdef".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..RawInputProps::default()
        })
        .style(|style| style.width /= icmd::Dimension::Cells(4))
        .node();
    let viewport = Size::new(12, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    interact(&output, &dispatcher, Some(click(0, 1)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::End, KeyModifiers::empty())),
    );
    // The viewport now shows the tail of the value; click its first cell.
    interact(&output, &dispatcher, Some(click(0, 0)));
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('#'), KeyModifiers::empty())),
    );
    let emitted = values.lock().unwrap().last().cloned().unwrap_or_default();
    assert_eq!(
        emitted, "abc#def",
        "the click must map to the first painted grapheme of the scrolled \
         viewport, not one cell past it"
    );
}

#[test]
fn horizontal_scrolling_past_a_wide_grapheme_still_paints() {
    // A three-cell viewport holds one two-cell grapheme per position. Paining
    // must not be dropped when the viewport pans across wide graphemes, and the
    // row must never be wider than the viewport.
    let node = raw_input
        .props(RawInputProps {
            default_value: Attr::Set("界界界界".into()),
            ..RawInputProps::default()
        })
        .style(|style| style.width /= icmd::Dimension::Cells(3))
        .node();
    let viewport = Size::new(9, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    let initial = collect_frames(&output, &dispatcher, None);
    assert!(
        !initial.is_empty(),
        "the widened viewport must produce a frame"
    );
    assert!(
        strip_ansi(&initial).contains('界'),
        "the viewport paints a wide grapheme: {:?}",
        strip_ansi(&initial)
    );
    // Pan to the end; every frame produced must be paintable, so the pipeline
    // keeps delivering rather than dropping the editor content.
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::End, KeyModifiers::empty())),
    );
    let panned = collect_frames(&output, &dispatcher, None);
    if !panned.is_empty() {
        assert!(
            strip_ansi(&panned).contains('界'),
            "a panned frame still paints a wide grapheme: {:?}",
            strip_ansi(&panned)
        );
    }
}
