use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use crossbeam_channel::{Receiver, Sender, bounded};

use crate::{DomId, DomNode, Frame, ImageId, Size};

use super::event::{EventDispatcher, EventRegion, ScrollOffset};
use geometry::RectI;
use layout::collect_scroll_ids;
use style::terminal_text;
use types::{Clip, PaintFragment, PaintKey, make_frame};

mod geometry;
mod layout;
mod paint;
mod scene;
mod style;
mod text;
mod types;

#[derive(Clone)]
pub struct ViewportSetter {
    value: Arc<Mutex<Size>>,
    wake: Sender<()>,
}

impl ViewportSetter {
    pub fn set(&self, size: Size) {
        *self.value.lock().expect("viewport mutex poisoned") = size;
        let _ = self.wake.try_send(());
    }

    pub fn viewport(&self) -> Size {
        *self.value.lock().expect("viewport mutex poisoned")
    }

    pub(crate) fn request_redraw(&self) {
        let _ = self.wake.try_send(());
    }
}

pub struct Commit {
    viewport: Size,
    viewport_state: Arc<Mutex<Size>>,
    viewport_rx: Receiver<()>,
    latest: Option<DomNode>,
    scene: HashMap<PaintKey, PaintFragment>,
    image_ids: HashMap<PaintKey, ImageId>,
    next_image_id: u64,
    event_dispatcher: EventDispatcher,
    scroll_offsets: Arc<Mutex<HashMap<DomId, ScrollOffset>>>,
}

impl Commit {
    pub fn new_with_events(viewport: Size) -> (Self, ViewportSetter, EventDispatcher) {
        let (commit, setter) = Self::new(viewport);
        let dispatcher = commit.event_dispatcher();
        (commit, setter, dispatcher)
    }

    pub fn new(viewport: Size) -> (Self, ViewportSetter) {
        let (wake, viewport_rx) = bounded(1);
        let viewport_state = Arc::new(Mutex::new(viewport));
        let setter = ViewportSetter {
            value: viewport_state.clone(),
            wake,
        };
        let event_dispatcher = EventDispatcher::new(setter.clone());
        let scroll_offsets = event_dispatcher.scroll_offsets();
        (
            Self {
                viewport,
                viewport_state,
                viewport_rx,
                latest: None,
                scene: HashMap::new(),
                image_ids: HashMap::new(),
                next_image_id: 1,
                event_dispatcher,
                scroll_offsets,
            },
            setter,
        )
    }

    pub fn event_dispatcher(&self) -> EventDispatcher {
        self.event_dispatcher.clone()
    }

    fn frame_for(&mut self, dom: Option<DomNode>, include_viewport: bool) -> Frame {
        if let Some(dom) = dom {
            self.latest = Some(dom);
        }
        let Some(root) = self.latest.clone() else {
            self.event_dispatcher.publish(Vec::new(), &HashSet::new());
            return make_frame(Vec::new(), include_viewport.then_some(self.viewport));
        };

        let mut next = HashMap::new();
        let root_rect = self.root_rect(&root);
        let mut retained_scroll_ids = HashSet::new();
        collect_scroll_ids(&root, &mut retained_scroll_ids);
        let inherited = terminal_text();
        let mut order = 0;
        let mut event_order = 0;
        let mut event_regions = Vec::<EventRegion>::new();
        let viewport_clip = Clip::Bounded(RectI::new(
            0,
            0,
            self.viewport.width as i32,
            self.viewport.height as i32,
        ));
        self.paint_node(
            &root,
            root_rect,
            inherited,
            None,
            0,
            viewport_clip,
            &mut order,
            &mut next,
            &mut event_regions,
            None,
            &mut event_order,
        );
        self.event_dispatcher
            .publish(event_regions, &retained_scroll_ids);

        let operations = self.diff_scene(&next);
        self.scene = next;
        make_frame(operations, include_viewport.then_some(self.viewport))
    }
}

impl crate::runtime::pipeline::PipelineComponent for Commit {
    type Input = DomNode;
    type Output = Frame;

    fn run(mut self, input: Receiver<Self::Input>, output: Sender<Self::Output>) {
        loop {
            crossbeam_channel::select! {
                recv(input) -> message => {
                    let Ok(dom) = message else { break; };
                    if output.send(self.frame_for(Some(dom), true)).is_err() { break; }
                }
                recv(self.viewport_rx) -> _ => {
                    self.viewport = *self.viewport_state.lock().expect("viewport mutex poisoned");
                    if self.output_latest(&output).is_err() { break; }
                }
            }
        }
    }
}

impl Commit {
    fn output_latest(&mut self, output: &Sender<Frame>) -> Result<(), ()> {
        let frame = self.frame_for(None, true);
        output.send(frame).map_err(|_| ())
    }
}

impl Drop for Commit {
    fn drop(&mut self) {
        self.event_dispatcher.publish(Vec::new(), &HashSet::new());
    }
}
