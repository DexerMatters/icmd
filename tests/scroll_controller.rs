//! ScrollController: reveal helpers, committed metrics, controlled wheel
//! mirroring, and the overlay scrollbar mode.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
use icmd::advanced::{
    Commit, EventDispatcher, FrameError, Lower, Renderer, Runtime, RuntimeHandle,
};
use icmd::{
    Attr, Component, ComponentContext, Dimension, Node, Props, ScrollAreaProps, ScrollController,
    ScrollbarVisibility, Size, scroll_area,
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

    fn pump(&self) {
        while self.output.recv_timeout(Duration::from_millis(150)).is_ok() {}
    }
}

fn rows(count: usize) -> Vec<Node> {
    (0..count)
        .map(|index| icmd::text(format!("row{index}")))
        .collect()
}

#[derive(Clone, Default)]
struct ControllerProps {
    slot: Arc<Mutex<Option<ScrollController>>>,
    visibility: Attr<ScrollbarVisibility>,
}

fn controller_component(cx: &mut ComponentContext, props: &Props<ControllerProps>) -> Node {
    let controller = cx.use_scroll_controller();
    *props.data().slot.lock().unwrap() = Some(controller.clone());
    scroll_area
        .props(ScrollAreaProps {
            controller: Attr::Set(controller),
            scrollbar_visibility: props.data().visibility,
            ..ScrollAreaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
        })
        .children(rows(10))
}

#[test]
fn controller_reveals_rows_and_mirrors_wheel_events() {
    let slot = Arc::new(Mutex::new(None));
    let node = controller_component.apply(ControllerProps {
        slot: slot.clone(),
        visibility: Attr::Set(ScrollbarVisibility::Auto),
    });
    let pipeline = Pipeline::new(Size::new(20, 6));
    pipeline.send(node);
    pipeline.frame();
    let controller = slot.lock().unwrap().clone().expect("controller");
    assert_eq!(controller.visible_rows(), 4);
    assert_eq!(controller.max_offset().y, 6);

    controller.reveal(6..7);
    pipeline.pump();
    assert_eq!(controller.offset().y, 3, "reveal scrolls the row into view");

    controller.scroll_to_row(0);
    pipeline.pump();
    assert_eq!(controller.offset().y, 0);

    pipeline.dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    pipeline.pump();
    assert_eq!(
        controller.offset().y,
        1,
        "a wheel notch is mirrored into the controller offset"
    );
    drop(pipeline.runtime);
}

#[test]
fn committed_metrics_report_reserved_and_overlay_scrollbars() {
    fn measured(visibility: ScrollbarVisibility) -> (ScrollController, Pipeline) {
        let slot = Arc::new(Mutex::new(None));
        let node = controller_component.apply(ControllerProps {
            slot: slot.clone(),
            visibility: Attr::Set(visibility),
        });
        let pipeline = Pipeline::new(Size::new(20, 6));
        pipeline.send(node);
        pipeline.frame();
        let controller = slot.lock().unwrap().clone().expect("controller");
        (controller, pipeline)
    }

    let (reserved, pipeline) = measured(ScrollbarVisibility::Always);
    assert_eq!(reserved.viewport_size(), Size::new(7, 4));
    assert_eq!(reserved.scrollbar_width(), 1);
    assert_eq!(reserved.scrollbar_height(), 0);
    drop(pipeline.runtime);

    let (overlay, pipeline) = measured(ScrollbarVisibility::Overlay);
    assert_eq!(
        overlay.viewport_size(),
        Size::new(8, 4),
        "overlay reserves no layout cell"
    );
    assert_eq!(overlay.scrollbar_width(), 1);
    drop(pipeline.runtime);
}
