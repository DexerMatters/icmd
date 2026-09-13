use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use crossbeam_channel::{Receiver, Sender, bounded};
use crossterm::style::Color;

use crate::{DomId, DomNode, EmojiMerging, Frame, Image, ImageId, Size, Text};

use super::event::{EventDispatcher, EventRegion, RuntimeScrollOffset};
use geometry::RectI;
use layout::collect_scroll_ids;
use style::terminal_text;
use types::{Clip, ComputedText, PaintFragment, PaintKey, make_frame};

mod geometry;
pub(crate) mod layout;
mod paint;
mod scene;
mod style;
pub(crate) mod text;
mod types;

#[derive(Debug, Clone, Copy, Default)]
pub struct CommitConfig {
    pub emoji_merging: EmojiMerging,
}

struct CachedTextRaster {
    text: Text,
    inherited: ComputedText,
    content_size: (i32, i32),
    visible_local: RectI,
    backdrop: Color,
    image: Image,
    used: u64,
}

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
    scene_order: Vec<PaintKey>,
    image_ids: HashMap<PaintKey, ImageId>,
    next_image_id: u64,
    event_dispatcher: EventDispatcher,
    scroll_offsets: Arc<Mutex<HashMap<DomId, RuntimeScrollOffset>>>,
    text_cache: HashMap<DomId, Vec<CachedTextRaster>>,
    text_cache_tick: u64,
    text_seen: HashSet<DomId>,
    emoji_merging: EmojiMerging,
}

impl Commit {
    pub fn new_with_events(viewport: Size) -> (Self, ViewportSetter, EventDispatcher) {
        Self::with_config_and_events(viewport, CommitConfig::default())
    }

    pub fn new(viewport: Size) -> (Self, ViewportSetter) {
        let (commit, setter, _) = Self::with_config_and_events(viewport, CommitConfig::default());
        (commit, setter)
    }

    pub fn with_config(viewport: Size, config: CommitConfig) -> (Self, ViewportSetter) {
        let (commit, setter, _) = Self::with_config_and_events(viewport, config);
        (commit, setter)
    }

    pub fn with_config_and_events(
        viewport: Size,
        config: CommitConfig,
    ) -> (Self, ViewportSetter, EventDispatcher) {
        let (wake, viewport_rx) = bounded(1);
        let viewport_state = Arc::new(Mutex::new(viewport));
        let setter = ViewportSetter {
            value: viewport_state.clone(),
            wake,
        };
        let event_dispatcher = EventDispatcher::new(setter.clone());
        let scroll_offsets = event_dispatcher.scroll_offsets();
        let commit = Self {
            viewport,
            viewport_state,
            viewport_rx,
            latest: None,
            scene: HashMap::new(),
            scene_order: Vec::new(),
            image_ids: HashMap::new(),
            next_image_id: 1,
            event_dispatcher: event_dispatcher.clone(),
            scroll_offsets,
            text_cache: HashMap::new(),
            text_cache_tick: 0,
            text_seen: HashSet::new(),
            emoji_merging: config.emoji_merging,
        };
        (commit, setter, event_dispatcher)
    }

    pub fn event_dispatcher(&self) -> EventDispatcher {
        self.event_dispatcher.clone()
    }

    #[allow(clippy::too_many_arguments)]
    fn raster_text_cached(
        &mut self,
        id: DomId,
        text: &Text,
        content: RectI,
        visible: RectI,
        inherited: ComputedText,
        backdrop: Color,
        scroll: (i32, i32),
    ) -> Option<Image> {
        self.text_seen.insert(id);
        self.text_cache_tick = self.text_cache_tick.saturating_add(1);
        let used = self.text_cache_tick;
        let visible_local = RectI::new(
            visible.line.saturating_sub(content.line),
            visible.column.saturating_sub(content.column),
            visible.width,
            visible.height,
        );
        let entries = self.text_cache.entry(id).or_default();
        if let Some(entry) = entries.iter_mut().find(|entry| {
            &entry.text == text
                && entry.inherited == inherited
                && entry.content_size == (content.width, content.height)
                && entry.visible_local == visible_local
                && entry.backdrop == backdrop
        }) {
            entry.used = used;
            return Some(entry.image.clone());
        }
        let image = text::raster_text(
            text,
            content,
            visible,
            inherited,
            backdrop,
            scroll,
            self.emoji_merging,
        )?;
        if entries.len() >= 2 {
            let oldest = entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(index, _)| index)
                .expect("non-empty text cache");
            entries.remove(oldest);
        }
        entries.push(CachedTextRaster {
            text: text.clone(),
            inherited,
            content_size: (content.width, content.height),
            visible_local,
            backdrop,
            image: image.clone(),
            used,
        });
        Some(image)
    }

    fn frame_for(&mut self, dom: Option<DomNode>, include_viewport: bool) -> Frame {
        if let Some(dom) = dom {
            self.latest = Some(dom);
        }
        // Painting needs mutable commit state, so temporarily move the
        // retained DOM out rather than cloning an entire tree on every resize
        // or scroll-driven frame.
        let Some(root) = self.latest.take() else {
            self.event_dispatcher.publish(Vec::new(), &HashSet::new());
            self.text_cache.clear();
            self.text_seen.clear();
            return make_frame(Vec::new(), include_viewport.then_some(self.viewport));
        };

        let mut next = HashMap::new();
        self.text_seen.clear();
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
            (0, 0),
        );
        // Autofocus is an explicit post-publication focus request: the runtime
        // grants it on the next published region set. This is how an input
        // starts focused without inferring focus from receiving input.
        if let Some(region) = event_regions.iter().find(|region| region.autofocus) {
            self.event_dispatcher.request_focus(region.id);
        }
        self.event_dispatcher
            .publish(event_regions, &retained_scroll_ids);

        let operations = self.diff_scene(&next);
        self.scene_order = next.keys().copied().collect();
        self.scene_order
            .sort_by_key(|key| (next[key].order, key.node.0, key.role));
        self.scene = next;
        self.latest = Some(root);
        self.text_cache.retain(|id, _| self.text_seen.contains(id));
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
