// The clipboard chords.
//
// Ctrl+C is the runtime's exit key and is checked before dispatch, so a widget
// that also claimed it would quit the application instead of copying. The
// clipboard therefore lives on the Shift-qualified family, and these tests pin
// both halves of that contract: plain Ctrl+C is never consumed by a widget, and
// Shift+Ctrl+C / Shift+Ctrl+V copy and paste through the framework clipboard.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use icmd::__private::{clipboard_clear, clipboard_load};
use icmd::advanced::{Commit, Lower, Renderer, Runtime};
use icmd::{Attr, Component, Dimension, EventListener, InputProps, Size, TextValueEvent, input};

fn pipeline(
    viewport: Size,
) -> (
    crossbeam_channel::Sender<icmd::Node>,
    crossbeam_channel::Receiver<Result<String, icmd::advanced::FrameError>>,
    icmd::advanced::EventDispatcher,
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

// Dispatch one event and let the runtime settle.
fn interact(
    output: &crossbeam_channel::Receiver<Result<String, icmd::advanced::FrameError>>,
    dispatcher: &icmd::advanced::EventDispatcher,
    event: Event,
) -> icmd::events::DispatchOutcome {
    while output.recv_timeout(Duration::from_millis(150)).is_ok() {}
    let outcome = dispatcher.dispatch(event);
    while output.recv_timeout(Duration::from_millis(150)).is_ok() {}
    outcome
}

fn click(row: u16, column: u16) -> Event {
    Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })
}

struct Recorder {
    values: Vec<String>,
    clipboard: Vec<String>,
}

fn editor(recorder: &Arc<Mutex<Recorder>>) -> icmd::Node {
    input
        .props(InputProps {
            default_value: Attr::Set("hello".into()),
            on_change: Attr::Set(EventListener::new({
                let recorder = recorder.clone();
                move |event: TextValueEvent| recorder.lock().unwrap().values.push(event.value)
            })),
            on_clipboard: Attr::Set(EventListener::new({
                let recorder = recorder.clone();
                move |event: icmd::TextClipboardEvent| {
                    recorder.lock().unwrap().clipboard.push(event.text)
                }
            })),
            ..InputProps::default()
        })
        .style(|style| style.width /= Dimension::Cells(12))
        .node()
}

// The bug: with the editor focused and a live selection, plain Ctrl+C belongs to
// the runtime, not to the widget. The widget must not stop propagation, or the
// exit key would never reach the loop that checks it before dispatch.
#[test]
fn plain_ctrl_c_is_left_to_the_runtime_even_with_a_selection() {
    let recorder = Arc::new(Mutex::new(Recorder {
        values: Vec::new(),
        clipboard: Vec::new(),
    }));
    let (sender, output, dispatcher) = pipeline(Size::new(14, 3));
    sender.send(editor(&recorder)).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    interact(&output, &dispatcher, click(0, 1));
    let selected = interact(
        &output,
        &dispatcher,
        key(KeyCode::Char('a'), KeyModifiers::CONTROL),
    );
    assert!(
        selected.propagation_stopped,
        "Select-all is the widget's chord"
    );

    let outcome = interact(
        &output,
        &dispatcher,
        key(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    assert!(
        !outcome.propagation_stopped,
        "plain Ctrl+C must reach the runtime's exit-key check"
    );
    assert!(
        recorder.lock().unwrap().clipboard.is_empty(),
        "plain Ctrl+C must not copy"
    );
}

#[test]
fn shift_ctrl_c_copies_and_shift_ctrl_v_pastes() {
    clipboard_clear();
    let recorder = Arc::new(Mutex::new(Recorder {
        values: Vec::new(),
        clipboard: Vec::new(),
    }));
    let (sender, output, dispatcher) = pipeline(Size::new(14, 3));
    sender.send(editor(&recorder)).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    interact(&output, &dispatcher, click(0, 1));
    interact(
        &output,
        &dispatcher,
        key(KeyCode::Char('a'), KeyModifiers::CONTROL),
    );
    let copied = interact(
        &output,
        &dispatcher,
        key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
    );
    assert!(copied.propagation_stopped, "the copy chord is the widget's");
    assert_eq!(
        recorder.lock().unwrap().clipboard.as_slice(),
        &["hello".to_string()],
        "the host observes the copy"
    );
    assert_eq!(
        clipboard_load().as_deref(),
        Some("hello"),
        "the framework clipboard holds the copied text"
    );

    // Clear the selection, then paste it back with the paste chord.
    interact(
        &output,
        &dispatcher,
        key(KeyCode::Backspace, KeyModifiers::empty()),
    );
    assert_eq!(
        recorder.lock().unwrap().values.last().map(String::as_str),
        Some(""),
        "backspace removes the selected text"
    );
    let pasted = interact(
        &output,
        &dispatcher,
        key(
            KeyCode::Char('v'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
    );
    assert!(
        pasted.propagation_stopped,
        "the paste chord is the widget's"
    );
    assert_eq!(
        recorder.lock().unwrap().values.last().map(String::as_str),
        Some("hello"),
        "paste inserts the clipboard at the caret"
    );
    clipboard_clear();
}
