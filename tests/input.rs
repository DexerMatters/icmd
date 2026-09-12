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
    Attr, Commit, Component, EventDispatcher, EventListener, Lower, Node, RawInputMode,
    RawInputProps, Renderer, Runtime, Size, TextValueEvent, raw_input,
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
