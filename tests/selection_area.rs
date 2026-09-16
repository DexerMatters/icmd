// End-to-end selection-area behavior: pointer drags, keyboard extension, the
// clipboard contract, focus, and the disabled/keyboard switches. The region
// contains two separate text leaves on two painted lines, so the document
// separator rule is exercised too.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::advanced::{Commit, EventDispatcher, Lower, Renderer, Runtime};
use icmd::{
    Attr, Component, Dimension, DomProps, EventListener, Node, Props, SelectionAreaProps, Size,
    TextClipboardEvent, TextSelectionEvent, selection_area,
};

fn pipeline(
    viewport: Size,
) -> (
    crossbeam_channel::Sender<icmd::Node>,
    crossbeam_channel::Receiver<Result<String, icmd::advanced::FrameError>>,
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

fn mouse(kind: MouseEventKind, row: u16, column: u16) -> Event {
    mouse_with(kind, row, column, KeyModifiers::empty())
}

fn mouse_with(kind: MouseEventKind, row: u16, column: u16, modifiers: KeyModifiers) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers,
    })
}

// Drive one event and let the runtime settle, exactly like the editor tests.
fn interact(
    output: &crossbeam_channel::Receiver<Result<String, icmd::advanced::FrameError>>,
    dispatcher: &EventDispatcher,
    event: Option<Event>,
) -> icmd::events::DispatchOutcome {
    while output.recv_timeout(Duration::from_millis(150)).is_ok() {}
    let outcome = match event {
        Some(event) => dispatcher.dispatch(event),
        None => icmd::events::DispatchOutcome::default(),
    };
    while output.recv_timeout(Duration::from_millis(150)).is_ok() {}
    outcome
}

fn painted(frame: &str) -> String {
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

#[derive(Default)]
struct Recorder {
    clipboard: Vec<TextClipboardEvent>,
    selection: Vec<TextSelectionEvent>,
}

// The region under test: two text leaves on two painted lines.
fn region(props: SelectionAreaProps, recorder: Arc<Mutex<Recorder>>) -> Node {
    let children = vec![Node::from("hello"), Node::from("world")];
    let props = Props::with_parts(DomProps::default(), children, props);
    let _ = recorder;
    selection_area.apply(props)
}

fn props_with_recorder(recorder: &Arc<Mutex<Recorder>>) -> SelectionAreaProps {
    SelectionAreaProps {
        on_clipboard: Attr::Set(EventListener::new({
            let recorder = recorder.clone();
            move |event: TextClipboardEvent| recorder.lock().unwrap().clipboard.push(event)
        })),
        on_selection_change: Attr::Set(EventListener::new({
            let recorder = recorder.clone();
            move |event: TextSelectionEvent| recorder.lock().unwrap().selection.push(event)
        })),
        ..SelectionAreaProps::default()
    }
}

#[test]
fn dragging_across_two_leaves_copies_the_document_with_a_separator() {
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let node = region(props_with_recorder(&recorder), recorder.clone());
    let (sender, output, dispatcher) = pipeline(Size::new(12, 4));
    sender.send(node).unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(
        painted(&first).contains("hello") && painted(&first).contains("world"),
        "both leaves must be painted: {:?}",
        painted(&first)
    );

    // Press on the first cell of "hello", drag to the last cell of "world".
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0)),
    );
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Drag(MouseButton::Left), 1, 5)),
    );
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Up(MouseButton::Left), 1, 5)),
    );
    let outcome = interact(
        &output,
        &dispatcher,
        Some(key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
    );
    assert!(
        outcome.propagation_stopped,
        "a live selection consumes Ctrl+C"
    );
    let recorder = recorder.lock().unwrap();
    assert_eq!(
        recorder.clipboard.last().map(|event| event.text.as_str()),
        Some("hello\nworld"),
        "the two blocks copy with a newline between them"
    );
    // The drag reported its intermediate ranges.
    assert!(
        recorder
            .selection
            .iter()
            .any(|event| event.range == (0..11)),
        "the final range covers the whole document: {:?}",
        recorder.selection
    );
}

#[test]
fn drag_release_keeps_the_selection() {
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let node = region(props_with_recorder(&recorder), recorder.clone());
    let (sender, output, dispatcher) = pipeline(Size::new(12, 4));
    sender.send(node).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Down(MouseButton::Left), 0, 1)),
    );
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Drag(MouseButton::Left), 0, 3)),
    );
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Up(MouseButton::Left), 0, 3)),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
    );
    assert_eq!(
        recorder
            .lock()
            .unwrap()
            .clipboard
            .last()
            .map(|event| event.text.as_str()),
        Some("ell"),
        "the selection survives the release"
    );
}

// The pointer-to-cell edge rule, asserted against the pixels rather than only
// against the copied text: every cell dragged over is highlighted, including
// the cell under the pointer.
#[test]
fn every_cell_dragged_over_is_highlighted() {
    crossterm::style::force_color_output(true);
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let node = {
        let props = props_with_recorder(&recorder);
        let children = vec![Node::from("hello world")];
        selection_area.apply(Props::with_parts(DomProps::default(), children, props))
    };
    let (sender, output, dispatcher) = pipeline(Size::new(14, 3));
    sender.send(node).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    // Press on the cell holding "e" (column 1) and drag to the cell holding the
    // second "l" (column 3): cells 1..=3 must carry the selection background.
    dispatcher.dispatch(mouse(MouseEventKind::Down(MouseButton::Left), 0, 1));
    dispatcher.dispatch(mouse(MouseEventKind::Drag(MouseButton::Left), 0, 3));
    let frame = settled_frame(&output).expect("a frame after the drag");
    assert!(
        frame.contains("[48;5;12mell"),
        "the highlighted run must be exactly the dragged cells: {frame:?}"
    );
    assert!(
        !frame.contains("[48;5;12mhello"),
        "the cell before the press must not be highlighted: {frame:?}"
    );
    assert!(
        !frame.contains("[48;5;12mello"),
        "the cell after the pointer must not be highlighted: {frame:?}"
    );
}

#[test]
fn ctrl_a_selects_the_whole_document_and_shift_click_extends() {
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let node = region(props_with_recorder(&recorder), recorder.clone());
    let (sender, output, dispatcher) = pipeline(Size::new(12, 4));
    sender.send(node).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    // Focus the region, then select all and copy.
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0)),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('a'), KeyModifiers::CONTROL)),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
    );
    assert_eq!(
        recorder
            .lock()
            .unwrap()
            .clipboard
            .last()
            .map(|event| event.text.as_str()),
        Some("hello\nworld")
    );

    // Shift+Right extends from the live anchor instead of collapsing.
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Left, KeyModifiers::empty())),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Right, KeyModifiers::SHIFT)),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
    );
    let recorder = recorder.lock().unwrap();
    let copied = recorder.clipboard.last().unwrap();
    assert!(
        !copied.text.is_empty() && copied.text.len() < "hello\nworld".len(),
        "Shift+Right leaves a partial selection: {:?}",
        copied.text
    );
}

#[test]
fn a_collapsed_selection_does_not_consume_copy() {
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let node = region(props_with_recorder(&recorder), recorder.clone());
    let (sender, output, dispatcher) = pipeline(Size::new(12, 4));
    sender.send(node).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    // No selection at all: Ctrl+C must bubble so an application handler can
    // still see it.
    let outcome = interact(
        &output,
        &dispatcher,
        Some(key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
    );
    assert!(
        !outcome.propagation_stopped,
        "an empty selection leaves Ctrl+C alone"
    );
    assert!(recorder.lock().unwrap().clipboard.is_empty());
}

#[test]
fn enable_keyboard_off_keeps_pointer_selection_but_not_copy() {
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let mut props = props_with_recorder(&recorder);
    props.enable_keyboard = Attr::Set(false);
    let node = region(props, recorder.clone());
    let (sender, output, dispatcher) = pipeline(Size::new(12, 4));
    sender.send(node).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0)),
    );
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Drag(MouseButton::Left), 0, 3)),
    );
    let outcome = interact(
        &output,
        &dispatcher,
        Some(key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
    );
    assert!(!outcome.propagation_stopped, "keyboard selection is off");
    assert!(
        recorder.lock().unwrap().clipboard.is_empty(),
        "no copy is produced when the keyboard is disabled"
    );
}

#[test]
fn disabled_region_takes_no_focus_and_selects_nothing() {
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let mut props = props_with_recorder(&recorder);
    props.disabled = Attr::Set(true);
    let node = region(props, recorder.clone());
    let (sender, output, dispatcher) = pipeline(Size::new(12, 4));
    sender.send(node).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0)),
    );
    assert!(
        dispatcher.focused().is_none(),
        "a disabled region is not focusable"
    );
    interact(
        &output,
        &dispatcher,
        Some(key(KeyCode::Char('a'), KeyModifiers::CONTROL)),
    );
    interact(
        &output,
        &dispatcher,
        Some(key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
    );
    assert!(recorder.lock().unwrap().clipboard.is_empty());
}

// Drain frames until the runtime goes quiet and return the last one: a
// settle helper must not require a *new* frame to arrive.
fn settled_frame(
    output: &crossbeam_channel::Receiver<Result<String, icmd::advanced::FrameError>>,
) -> Option<String> {
    let mut last = None;
    loop {
        match output.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(frame)) => last = Some(frame),
            Ok(Err(error)) => panic!("frame error: {error}"),
            Err(_) => return last,
        }
    }
}

#[test]
fn blur_keeps_the_selection_and_repaints_it_inactive() {
    // Colour output is disabled under NO_COLOR (and for a non-tty), which would
    // make the active and inactive styles byte-identical. This test is about the
    // styling, so it asks for colour explicitly.
    crossterm::style::force_color_output(true);
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let node = region(props_with_recorder(&recorder), recorder.clone());
    let (sender, output, dispatcher) = pipeline(Size::new(12, 4));
    sender.send(node).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    // Dispatch the drag without draining, so the selected frame is observed.
    dispatcher.dispatch(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
    dispatcher.dispatch(mouse(MouseEventKind::Drag(MouseButton::Left), 0, 3));
    let selected = settled_frame(&output).expect("a frame after the drag");

    dispatcher.blur();
    let blurred = settled_frame(&output).expect("a frame after focus loss");
    assert_ne!(
        selected, blurred,
        "losing focus must repaint the selection in its inactive style"
    );
    // Both frames still paint a selection highlight: losing focus changes the
    // styling, not the selection.
    for (name, frame) in [("active", &selected), ("inactive", &blurred)] {
        assert!(
            frame.contains("[48;5;"),
            "the {name} frame must still paint a selection background: {frame:?}"
        );
    }

    // The range survives: a Shift press after focus loss extends from the
    // anchor the drag left behind instead of starting a new selection.
    interact(
        &output,
        &dispatcher,
        Some(mouse_with(
            MouseEventKind::Down(MouseButton::Left),
            0,
            3,
            KeyModifiers::SHIFT,
        )),
    );
    assert!(dispatcher.focused().is_some());
    interact(
        &output,
        &dispatcher,
        Some(key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
    );
    assert_eq!(
        recorder
            .lock()
            .unwrap()
            .clipboard
            .last()
            .map(|event| event.text.as_str()),
        Some("hel"),
        "the pre-blur anchor is still there to extend from"
    );
}

#[test]
fn a_region_sizes_to_its_children_and_clips_nothing_by_default() {
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let mut props = props_with_recorder(&recorder);
    // A caller style must still apply, and the host must not force a size.
    let node = {
        let children = vec![Node::from("x")];
        let mut dom = DomProps::default();
        dom.style.width = Attr::Set(Dimension::Cells(3));
        let props = Props::with_parts(dom, children, std::mem::take(&mut props));
        selection_area.apply(props)
    };
    let (sender, output, dispatcher) = pipeline(Size::new(12, 4));
    sender.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(painted(&frame).contains('x'));
    let _ = dispatcher;
}

// A disabled region is a barrier: it owns its subtree's document, so an ancestor
// cannot select the text below it, and it consumes a press so a drag inside it
// does not select the prose around it. This is what keeps an embedded example
// out of a surrounding selectable region while a nested enabled region inside
// the example stays selectable on its own terms.
#[test]
fn a_disabled_region_keeps_its_subtree_out_of_an_ancestor() {
    let recorder = Arc::new(Mutex::new(Recorder::default()));
    let barrier = {
        let children = vec![Node::from("example")];
        let props = Props::with_parts(
            DomProps::default(),
            children,
            SelectionAreaProps {
                disabled: Attr::Set(true),
                ..SelectionAreaProps::default()
            },
        );
        selection_area.apply(props)
    };
    // Three painted rows: ancestor text, the barrier's own line, ancestor text.
    let children = vec![Node::from("above"), barrier, Node::from("below")];
    let props = Props::with_parts(
        DomProps::default(),
        children,
        props_with_recorder(&recorder),
    );
    let node = selection_area.apply(props);

    let (sender, output, dispatcher) = pipeline(Size::new(16, 4));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();

    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Down(MouseButton::Left), 1, 1)),
    );
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Drag(MouseButton::Left), 1, 6)),
    );
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Up(MouseButton::Left), 1, 6)),
    );
    assert!(
        recorder.lock().unwrap().selection.is_empty(),
        "a drag inside the barrier must select nothing at all"
    );

    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Down(MouseButton::Left), 0, 1)),
    );
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Drag(MouseButton::Left), 0, 5)),
    );
    interact(
        &output,
        &dispatcher,
        Some(mouse(MouseEventKind::Up(MouseButton::Left), 0, 5)),
    );
    let selected = recorder.lock().unwrap().selection.clone();
    let last = selected
        .last()
        .expect("the ancestor still selects its own text");
    assert!(
        !last.text.is_empty() && "above".contains(last.text.as_str()),
        "the ancestor's own text must still select, got {:?}",
        last.text
    );
}
