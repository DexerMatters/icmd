//! Overlay primitives: portal hoisting, pointer transparency, dialog focus
//! restoration, popover anchoring, and menu keyboard selection.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::advanced::{
    Commit, EventDispatcher, FrameError, Lower, Renderer, Runtime, RuntimeHandle,
};
use icmd::{
    AppHandle, Attr, Component, Dimension, DomProps, ElementRef, EventListener, Layout, Node,
    Overflow, Size,
};
use icmd::{
    DialogProps, MenuItem, MenuProps, PopoverPlacement, PopoverProps, PortalProps, dialog, menu,
    popover, portal,
};

type StageOutput = Result<String, FrameError>;

struct Pipeline {
    runtime: RuntimeHandle<Node, StageOutput>,
    dispatcher: EventDispatcher,
    handle: AppHandle,
    output: crossbeam_channel::Receiver<StageOutput>,
}

impl Pipeline {
    fn new(viewport: Size) -> Self {
        let (commit, _, dispatcher) = Commit::new_with_events(viewport);
        let handle = AppHandle::default();
        handle.attach_services(Arc::new(dispatcher.clone()));
        let runtime = Runtime::new(Lower::default().with_handle(handle.clone()))
            .then(commit)
            .then(Renderer::new(viewport).expect("renderer"))
            .start_handle();
        Self {
            output: runtime.output(),
            runtime,
            dispatcher,
            handle,
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

    fn pump(&self) {
        while self.output.recv_timeout(Duration::from_millis(150)).is_ok() {}
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

fn root_with(child: Node) -> Node {
    let mut dom = DomProps::default();
    dom.style.width = Attr::Set(Dimension::Max);
    dom.style.height = Attr::Set(Dimension::Max);
    Node::element(dom, [child])
}

fn focusable_root(child: Node) -> Node {
    let mut dom = DomProps::default();
    dom.style.width = Attr::Set(Dimension::Max);
    dom.style.height = Attr::Set(Dimension::Max);
    dom.focusable = Attr::Set(true);
    dom.autofocus = true;
    Node::element(dom, [child])
}

#[test]
fn portal_escapes_ancestor_clipping_and_paints_above() {
    let mut clip_dom = DomProps::default();
    clip_dom.style.width = Attr::Set(Dimension::Cells(4));
    clip_dom.style.height = Attr::Set(Dimension::Cells(1));
    clip_dom.style.overflow = Attr::Set(Overflow::Clip);
    clip_dom.style.layout = Attr::Set(Layout::Vertical);

    let mut label = DomProps::default();
    label.style.width = Attr::Set(Dimension::Cells(8));
    label.style.height = Attr::Set(Dimension::Cells(1));
    label.style.line = Attr::Set(icmd::AxisPosition::Cells(4));
    let child = Node::element(label, [icmd::text("OUTSIDE")]);
    let portal_node = portal.props(PortalProps::default()).children([child]);
    let clip = Node::element(clip_dom, [portal_node]);
    let pipeline = Pipeline::new(Size::new(30, 8));
    pipeline.send(root_with(clip));
    let mut frame = String::new();
    while let Ok(Ok(part)) = pipeline.output.recv_timeout(Duration::from_millis(200)) {
        frame.push_str(&part);
    }
    assert!(
        frame.contains("OUTSIDE"),
        "a portal must paint outside its ancestor's clip: {frame:?}"
    );
    drop(pipeline.runtime);
}

#[test]
fn portal_host_is_pointer_transparent_and_child_is_not() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_for_listener = hits.clone();
    let mut target = DomProps::default();
    target.style.width = Attr::Set(Dimension::Cells(5));
    target.style.height = Attr::Set(Dimension::Cells(2));
    target.events.pointer_down =
        Attr::Set(EventListener::new(move |_event: icmd::PointerEvent| {
            hits_for_listener.fetch_add(1, Ordering::SeqCst);
        }));
    let target = Node::element(target, [icmd::text("target")]);

    let mut overlay = DomProps::default();
    overlay.style.line = Attr::Set(icmd::AxisPosition::Cells(5));
    overlay.style.column = Attr::Set(icmd::AxisPosition::Cells(10));
    overlay.style.width = Attr::Set(Dimension::Cells(4));
    overlay.style.height = Attr::Set(Dimension::Cells(1));
    overlay.style.background = Attr::Set(crossterm::style::Color::Blue);
    let overlay = Node::element(overlay, [icmd::text("card")]);
    let portal_node = portal.props(PortalProps::default()).children([overlay]);

    let mut dom = DomProps::default();
    dom.style.width = Attr::Set(Dimension::Max);
    dom.style.height = Attr::Set(Dimension::Max);
    let root = Node::element(dom, [target, portal_node]);
    let pipeline = Pipeline::new(Size::new(20, 8));
    pipeline.send(root);
    pipeline.frame();
    pipeline.dispatcher.dispatch(click(1, 1));
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "a click outside the overlay card must reach content under the portal host"
    );
    pipeline.dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    pipeline.dispatcher.dispatch(click(5, 11));
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "the overlay card itself stops the click from reaching content behind it"
    );
    drop(pipeline.runtime);
}

fn dialog_node(open: bool, closes: Arc<AtomicUsize>) -> Node {
    dialog
        .props(DialogProps {
            open: Attr::Set(open),
            title: Attr::Set("Title".into()),
            modal: Attr::Set(false),
            on_close: Attr::Set(EventListener::new(move |_| {
                closes.fetch_add(1, Ordering::SeqCst);
            })),
            ..DialogProps::default()
        })
        .children([icmd::text("body")])
}

#[test]
fn dialog_saves_and_restores_focus_and_closes_on_escape_and_outside() {
    let closes = Arc::new(AtomicUsize::new(0));
    let pipeline = Pipeline::new(Size::new(30, 10));
    pipeline.send(focusable_root(dialog_node(false, closes.clone())));
    pipeline.frame();
    let root_focus = pipeline.dispatcher.focused().expect("root focused");

    pipeline.send(focusable_root(dialog_node(true, closes.clone())));
    pipeline.pump();
    let dialog_focus = pipeline.dispatcher.focused().expect("dialog focused");
    assert_ne!(dialog_focus, root_focus);

    pipeline.dispatcher.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::empty(),
    )));
    assert_eq!(closes.load(Ordering::SeqCst), 1, "Escape closes");

    pipeline.send(focusable_root(dialog_node(false, closes.clone())));
    pipeline.pump();
    assert_eq!(
        pipeline.dispatcher.focused(),
        Some(root_focus),
        "closing the dialog restores the previously focused region"
    );

    // Reopen and dismiss by pressing outside the centered card.
    pipeline.send(focusable_root(dialog_node(true, closes.clone())));
    pipeline.pump();
    pipeline.dispatcher.dispatch(click(0, 0));
    assert!(
        closes.load(Ordering::SeqCst) >= 2,
        "an outside press closes a non-modal dialog"
    );
    drop(pipeline.runtime);
}

#[test]
fn dialog_initial_focus_can_target_a_child() {
    let child = ElementRef::new();
    let mut child_dom = DomProps::default();
    child_dom.style.width = Attr::Set(Dimension::Cells(6));
    child_dom.style.height = Attr::Set(Dimension::Cells(1));
    child_dom.focusable = Attr::Set(true);
    child_dom.element_ref = Attr::Set(child.clone());
    let child_node = Node::element(child_dom, [icmd::text("field")]);
    let dialog = dialog
        .props(DialogProps {
            open: Attr::Set(true),
            title: Attr::Set("Focus".into()),
            initial_focus: Attr::Set(child.clone()),
            ..DialogProps::default()
        })
        .children([child_node]);
    let pipeline = Pipeline::new(Size::new(30, 10));
    pipeline.send(focusable_root(dialog));
    pipeline.pump();
    let child_id = child.current().expect("child committed").id();
    assert_eq!(
        pipeline.dispatcher.focused(),
        Some(child_id),
        "initial_focus overrides the dialog card's autofocus"
    );
    drop(pipeline.runtime);
}

#[test]
fn popover_waits_for_its_anchor_then_paints_and_dismisses_outside() {
    let closes = Arc::new(AtomicUsize::new(0));
    let closes_for_listener = closes.clone();
    let anchor = ElementRef::new();
    let mut anchor_dom = DomProps::default();
    anchor_dom.style.width = Attr::Set(Dimension::Cells(3));
    anchor_dom.style.height = Attr::Set(Dimension::Cells(1));
    anchor_dom.element_ref = Attr::Set(anchor.clone());
    let anchor_node = Node::element(anchor_dom, [icmd::text("btn")]);
    let pop = popover
        .props(PopoverProps {
            open: Attr::Set(true),
            anchor: Attr::Set(anchor.clone()),
            placement: Attr::Set(PopoverPlacement::BelowStart),
            on_close: Attr::Set(EventListener::new(move |_| {
                closes_for_listener.fetch_add(1, Ordering::SeqCst);
            })),
            ..PopoverProps::default()
        })
        .children([icmd::text("POPUP")]);
    let mut column = DomProps::default();
    column.style.layout = Attr::Set(Layout::Vertical);
    column.style.width = Attr::Set(Dimension::Max);
    column.style.height = Attr::Set(Dimension::Max);
    let pipeline = Pipeline::new(Size::new(30, 10));
    pipeline.send(Node::element(column, [anchor_node, pop]));
    // The popover requests one follow-up pass when its anchor is not committed
    // on the first render. Drain until it has painted.
    let mut frame = String::new();
    for _ in 0..10 {
        if let Ok(Ok(part)) = pipeline.output.recv_timeout(Duration::from_millis(200)) {
            frame.push_str(&part);
            if frame.contains("POPUP") {
                break;
            }
        }
    }
    assert!(
        frame.contains("POPUP"),
        "the popover paints once its anchor is committed: {frame:?}"
    );
    pipeline.dispatcher.dispatch(click(20, 20));
    assert!(
        closes.load(Ordering::SeqCst) >= 1,
        "a press outside the popover dismisses it"
    );
    drop(pipeline.runtime);
}

#[test]
fn menu_navigates_and_reports_the_selected_item() {
    let selected = Arc::new(Mutex::new(Vec::new()));
    let selected_for_listener = selected.clone();
    let anchor = ElementRef::new();
    let mut anchor_dom = DomProps::default();
    anchor_dom.style.width = Attr::Set(Dimension::Cells(3));
    anchor_dom.style.height = Attr::Set(Dimension::Cells(1));
    anchor_dom.element_ref = Attr::Set(anchor.clone());
    let anchor_node = Node::element(anchor_dom, [icmd::text("btn")]);
    let menu_node = menu
        .props(MenuProps {
            open: Attr::Set(true),
            anchor: Attr::Set(anchor.clone()),
            items: Attr::Set(vec![
                MenuItem::new("one", "One"),
                MenuItem::new("two", "Two"),
                MenuItem::new("three", "Three").disabled(),
            ]),
            on_select: Attr::Set(EventListener::new(move |id: icmd::Key| {
                selected_for_listener.lock().unwrap().push(id);
            })),
            ..MenuProps::default()
        })
        .node();
    let mut column = DomProps::default();
    column.style.layout = Attr::Set(Layout::Vertical);
    column.style.width = Attr::Set(Dimension::Max);
    column.style.height = Attr::Set(Dimension::Max);
    let pipeline = Pipeline::new(Size::new(30, 10));
    pipeline.send(Node::element(column, [anchor_node, menu_node]));
    pipeline.frame();
    pipeline.handle.request_frame();
    pipeline.pump();
    assert!(
        pipeline.dispatcher.focused().is_some(),
        "the menu takes focus"
    );

    pipeline.dispatcher.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Down,
        KeyModifiers::empty(),
    )));
    // No pump: Enter must read the live active row, not the render snapshot.
    pipeline.dispatcher.dispatch(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::empty(),
    )));
    pipeline.pump();
    assert_eq!(
        selected
            .lock()
            .unwrap()
            .first()
            .map(|key| key.as_str().to_string()),
        Some("two".to_string()),
        "Down plus Enter selects the second item"
    );
    drop(pipeline.runtime);
}
