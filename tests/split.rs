//! Split/pane/frame primitives: weighted children, splitter drag, and keyboard
//! adjustment.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::advanced::{
    Commit, EventDispatcher, FrameError, Lower, Renderer, Runtime, RuntimeHandle,
};
use icmd::{
    Attr, Component, Dimension, DomProps, ElementRef, EventListener, Layout, Node, PaneProps, Size,
    SplitProps, SplitResizeEvent, pane, split,
};

type StageOutput = Result<String, FrameError>;

struct Pipeline {
    runtime: RuntimeHandle<Node, StageOutput>,
    output: crossbeam_channel::Receiver<StageOutput>,
    dispatcher: EventDispatcher,
}

impl Pipeline {
    fn new(viewport: Size) -> Self {
        let (commit, _, dispatcher) = Commit::new_with_events(viewport);
        let runtime = Runtime::new(Lower::default())
            .then(commit)
            .then(Renderer::new(viewport).expect("renderer"))
            .start_handle();
        Self {
            output: runtime.output(),
            runtime,
            dispatcher,
        }
    }

    fn send(&self, node: Node) {
        self.runtime.input().send(node).expect("root accepted");
    }

    fn frame(&self) -> String {
        self.output
            .recv_timeout(Duration::from_secs(2))
            .expect("a frame arrives")
            .expect("renderer succeeds")
    }
}

fn mouse(kind: MouseEventKind, row: u16, column: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })
}

fn tracked(refs: &mut Vec<ElementRef>) -> Node {
    let handle = ElementRef::new();
    refs.push(handle.clone());
    let mut dom = DomProps::default();
    dom.style.width = Attr::Set(Dimension::Max);
    dom.style.height = Attr::Set(Dimension::Max);
    dom.element_ref = Attr::Set(handle);
    Node::element(dom, [icmd::text("x")])
}

#[test]
fn split_allocates_weighted_widths_and_pane_titles_paint() {
    let mut refs = Vec::new();
    let node = split
        .props(SplitProps {
            axis: Attr::Set(Layout::Horizontal),
            weights: Attr::Set(vec![1, 2]),
            splitter_size: Attr::Set(1),
            ..SplitProps::default()
        })
        .children([
            pane.props(PaneProps {
                title: Attr::Set("Left".into()),
            })
            .children([tracked(&mut refs)]),
            pane.props(PaneProps {
                title: Attr::Set("Right".into()),
            })
            .children([tracked(&mut refs)]),
        ]);
    let mut root = DomProps::default();
    root.style.width = Attr::Set(Dimension::Max);
    root.style.height = Attr::Set(Dimension::Max);
    let pipeline = Pipeline::new(Size::new(20, 8));
    pipeline.send(Node::element(root, [node]));
    let frame = pipeline.frame();
    assert!(
        frame.contains("Left") || frame.contains("L"),
        "pane title paints"
    );
    let widths: Vec<u32> = refs
        .iter()
        .map(|handle| handle.current().expect("committed").bounding_rect().width)
        .collect();
    // Each pane spends four cells on border and horizontal padding, so the
    // tracked content boxes are 6-4 and 13-4.
    assert_eq!(widths, vec![2, 9], "19 cells minus one splitter split 1:2");
    drop(pipeline.runtime);
}

#[test]
fn dragging_a_splitter_proposes_new_weights() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_listener = events.clone();
    let mut refs = Vec::new();
    let node = split
        .props(SplitProps {
            axis: Attr::Set(Layout::Horizontal),
            splitter_size: Attr::Set(1),
            on_weights_change: Attr::Set(EventListener::new(move |event: SplitResizeEvent| {
                events_for_listener.lock().unwrap().push(event)
            })),
            ..SplitProps::default()
        })
        .children([tracked(&mut refs), tracked(&mut refs)]);
    let mut root = DomProps::default();
    root.style.width = Attr::Set(Dimension::Max);
    root.style.height = Attr::Set(Dimension::Max);
    let pipeline = Pipeline::new(Size::new(20, 8));
    pipeline.send(Node::element(root, [node]));
    pipeline.frame();

    // Equal panes put the splitter in column 10; drag it two cells right.
    pipeline
        .dispatcher
        .dispatch(mouse(MouseEventKind::Down(MouseButton::Left), 1, 10));
    pipeline
        .dispatcher
        .dispatch(mouse(MouseEventKind::Drag(MouseButton::Left), 1, 12));
    pipeline
        .dispatcher
        .dispatch(mouse(MouseEventKind::Up(MouseButton::Left), 1, 12));
    let events = events.lock().unwrap().clone();
    let last = events.last().expect("a resize event is emitted");
    assert_eq!(
        last.weights,
        vec![11, 7],
        "a two-cell drag transfers two cells of weight from the right pane"
    );
    // The uncontrolled split rerenders from its live weights: 19 cells split
    // 11:7 become two content boxes after each pane's border and padding.
    while pipeline
        .output
        .recv_timeout(Duration::from_millis(200))
        .is_ok()
    {}
    let widths: Vec<u32> = refs
        .iter()
        .map(|handle| handle.current().expect("committed").bounding_rect().width)
        .collect();
    assert_eq!(widths, vec![12, 7], "the split paints the dragged weights");
    drop(pipeline.runtime);
}

#[test]
fn a_focused_splitter_moves_with_the_arrow_keys() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_listener = events.clone();
    let mut refs = Vec::new();
    let node = split
        .props(SplitProps {
            axis: Attr::Set(Layout::Horizontal),
            weights: Attr::Set(vec![2, 2]),
            splitter_size: Attr::Set(1),
            on_weights_change: Attr::Set(EventListener::new(move |event: SplitResizeEvent| {
                events_for_listener.lock().unwrap().push(event)
            })),
            ..SplitProps::default()
        })
        .children([tracked(&mut refs), tracked(&mut refs)]);
    let mut root = DomProps::default();
    root.style.width = Attr::Set(Dimension::Max);
    root.style.height = Attr::Set(Dimension::Max);
    let pipeline = Pipeline::new(Size::new(20, 8));
    pipeline.send(Node::element(root, [node]));
    pipeline.frame();
    pipeline
        .dispatcher
        .dispatch(mouse(MouseEventKind::Down(MouseButton::Left), 1, 10));
    pipeline
        .dispatcher
        .dispatch(mouse(MouseEventKind::Up(MouseButton::Left), 1, 10));
    pipeline.dispatcher.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::empty(),
    )));
    let events = events.lock().unwrap().clone();
    assert_eq!(
        events.last().map(|event| event.weights.clone()),
        Some(vec![3, 1]),
        "Right moves one weight from the right pane to the left pane"
    );
    drop(pipeline.runtime);
}
