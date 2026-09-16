//! Commit stage: turns a lowered DOM into a `Frame`, retaining the painted
//! scene, event regions, text rasters, and layout caches between frames.
//! Emits a frame both on new DOM input and on viewport changes.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use crossbeam_channel::{Receiver, Sender, bounded};
use crossterm::style::Color;

use crate::EventListener;
use crate::basic::element_ref::{ElementRef, ElementSnapshot};
use crate::basic::selection::SelectionOverlay;
use crate::basic::text_layout;
use crate::{DomId, DomNode, EmojiMerging, Frame, Image, ImageId, Size, Text};

use super::event::{EventDispatcher, EventRegion, RuntimeScrollOffset};
use geometry::RectI;
use layout::collect_scroll_ids;
use style::terminal_text;
use types::{Clip, ComputedText, PaintFragment, PaintKey, make_frame};

pub(crate) mod geometry;
pub(crate) mod layout;
mod paint;
mod scene;
mod style;
pub(crate) mod text;
mod types;

/// A host element's measurement bindings retained across commits so refs can
/// be cleared when a host disappears and listeners can receive `None`.
#[derive(Clone)]
pub(super) struct ElementBinding {
    pub(super) element_ref: Option<ElementRef>,
    pub(super) listener: Option<EventListener<Option<ElementSnapshot>>>,
    pub(super) snapshot: ElementSnapshot,
}

fn collect_element_measurement_paths(node: &DomNode, paths: &mut HashSet<DomId>) -> bool {
    match node {
        DomNode::Element {
            id,
            props,
            children,
        } => {
            let own = props.element_ref.is_set() || props.element_change.is_set();
            let mut descendant = false;
            for child in children {
                descendant |= collect_element_measurement_paths(child, paths);
            }
            if own || descendant {
                paths.insert(*id);
            }
            own || descendant
        }
        DomNode::Text { .. } | DomNode::Image { .. } | DomNode::Raster { .. } => false,
    }
}

/// Configuration for a [`Commit`] stage.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommitConfig {
    /// Emoji merging mode applied to every text leaf in the commit.
    pub emoji_merging: EmojiMerging,
}

struct CachedTextRaster {
    text: Text,
    inherited: ComputedText,
    content_size: (i32, i32),
    visible_local: RectI,
    backdrop: Color,
    overlay: Option<crate::basic::selection::SelectionOverlay>,
    image: Image,
    used: u64,
}

struct CachedTextGeometry {
    text: Text,
    inherited: ComputedText,
    content_size: (i32, i32),
    merging: EmojiMerging,
    geometry: text::TextGeometry,
    used: u64,
}

/// Cloneable handle used to publish a new viewport size to the commit stage.
#[derive(Clone)]
pub struct ViewportSetter {
    value: Arc<Mutex<Size>>,
    wake: Sender<()>,
}

impl ViewportSetter {
    /// Stores `size`, in terminal cells, and wakes the commit stage to repaint.
    pub fn set(&self, size: Size) {
        *self.value.lock().expect("viewport mutex poisoned") = size;
        let _ = self.wake.try_send(());
    }

    /// Returns the current viewport size in terminal cells.
    pub fn viewport(&self) -> Size {
        *self.value.lock().expect("viewport mutex poisoned")
    }

    pub(crate) fn request_redraw(&self) {
        let _ = self.wake.try_send(());
    }
}

/// Commit stage: owns the retained DOM, painted scene, caches, and event state.
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
    text_geometry: HashMap<DomId, CachedTextGeometry>,
    text_geometry_tick: u64,
    text_seen: HashSet<DomId>,
    emoji_merging: EmojiMerging,
    instrument: Arc<LayoutInstrument>,
    natural_cache: crate::runtime::commit::text::NaturalCache,
    element_bindings: Vec<(DomId, ElementBinding)>,
}

impl Commit {
    /// Builds a commit stage with its viewport setter and event dispatcher.
    pub fn new_with_events(viewport: Size) -> (Self, ViewportSetter, EventDispatcher) {
        Self::with_config_and_events(viewport, CommitConfig::default())
    }

    /// Builds a commit stage with its viewport setter and default configuration.
    pub fn new(viewport: Size) -> (Self, ViewportSetter) {
        let (commit, setter, _) = Self::with_config_and_events(viewport, CommitConfig::default());
        (commit, setter)
    }

    /// Builds a commit stage with its viewport setter and the given configuration.
    pub fn with_config(viewport: Size, config: CommitConfig) -> (Self, ViewportSetter) {
        let (commit, setter, _) = Self::with_config_and_events(viewport, config);
        (commit, setter)
    }

    /// Builds a commit stage with its viewport setter, event dispatcher, and configuration.
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
            text_geometry: HashMap::new(),
            text_geometry_tick: 0,
            text_seen: HashSet::new(),
            instrument: Arc::new(LayoutInstrument::default()),
            natural_cache: crate::runtime::commit::text::NaturalCache::default(),
            element_bindings: Vec::new(),
            emoji_merging: config.emoji_merging,
        };
        (commit, setter, event_dispatcher)
    }

    /// Returns a clone of the dispatcher that publishes regions and routes events.
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
        self.raster_text_entry(
            id, text, content, visible, inherited, backdrop, scroll, None, None,
        )
    }

    fn text_geometry_cached(
        &mut self,
        id: DomId,
        text: &Text,
        inherited: ComputedText,
        content: RectI,
    ) -> text::TextGeometry {
        self.text_geometry_tick = self.text_geometry_tick.saturating_add(1);
        let used = self.text_geometry_tick;
        let content_size = (content.width, content.height);
        if let Some(entry) = self.text_geometry.get_mut(&id)
            && &entry.text == text
            && entry.inherited == inherited
            && entry.content_size == content_size
            && entry.merging == self.emoji_merging
        {
            entry.used = used;
            return entry.geometry.clone();
        }
        let layout = Arc::new(text::layout(
            text,
            content_size.0.max(1) as usize,
            inherited,
            self.emoji_merging,
        ));
        let value: Arc<str> = Arc::from(
            text.spans
                .iter()
                .map(|span| span.content.as_str())
                .collect::<String>(),
        );
        let geometry = text::TextGeometry { layout, value };
        self.text_geometry.insert(
            id,
            CachedTextGeometry {
                text: text.clone(),
                inherited,
                content_size,
                merging: self.emoji_merging,
                geometry: geometry.clone(),
                used,
            },
        );
        geometry
    }

    #[allow(clippy::too_many_arguments)]
    fn selectable_text_cached(
        &mut self,
        id: DomId,
        text: &Text,
        geometry: &text::TextGeometry,
        content: RectI,
        visible: RectI,
        inherited: ComputedText,
        backdrop: Color,
        scroll: (i32, i32),
        overlay: Option<SelectionOverlay>,
    ) -> Option<Image> {
        self.raster_text_entry(
            id,
            text,
            content,
            visible,
            inherited,
            backdrop,
            scroll,
            overlay,
            Some(&geometry.layout),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn raster_text_entry(
        &mut self,
        id: DomId,
        text: &Text,
        content: RectI,
        visible: RectI,
        inherited: ComputedText,
        backdrop: Color,
        scroll: (i32, i32),
        overlay: Option<SelectionOverlay>,
        layout: Option<&Arc<text_layout::TextLayout>>,
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
                && entry.overlay == overlay
        }) {
            entry.used = used;
            return Some(entry.image.clone());
        }
        let image = match layout {
            Some(layout) => text::raster_plain_with_layout(
                text,
                layout,
                content,
                visible,
                inherited,
                backdrop,
                overlay.as_ref(),
            ),
            None => text::raster_text(
                text,
                content,
                visible,
                inherited,
                backdrop,
                scroll,
                self.emoji_merging,
            ),
        }?;
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
            overlay,
            image: image.clone(),
            used,
        });
        Some(image)
    }

    /// Publishes refs before invoking callbacks so every callback sees a
    /// coherent set of snapshots from this commit.
    fn publish_element_bindings(
        &mut self,
        next: Vec<(DomId, ElementBinding)>,
    ) -> Result<(), crate::runtime::pipeline::RuntimeError> {
        let previous = std::mem::replace(&mut self.element_bindings, next);
        let next_refs: Vec<ElementRef> = self
            .element_bindings
            .iter()
            .filter_map(|(_, binding)| binding.element_ref.clone())
            .collect();
        let mut callbacks = Vec::new();

        for (id, old) in &previous {
            let Some((_, current)) = self
                .element_bindings
                .iter()
                .find(|(current_id, _)| current_id == id)
            else {
                if let Some(element_ref) = &old.element_ref
                    && !next_refs.iter().any(|candidate| candidate == element_ref)
                {
                    element_ref.clear();
                }
                if let Some(listener) = old.listener.clone() {
                    callbacks.push((listener, None));
                }
                continue;
            };

            if let Some(old_ref) = &old.element_ref
                && current.element_ref.as_ref() != Some(old_ref)
                && !next_refs.iter().any(|candidate| candidate == old_ref)
            {
                old_ref.clear();
            }

            let changed = old.snapshot != current.snapshot;
            let listener_added = old.listener.is_none() && current.listener.is_some();
            if (changed || listener_added)
                && let Some(listener) = current.listener.clone()
            {
                callbacks.push((listener, Some(current.snapshot.clone())));
            }
        }

        for (id, current) in &self.element_bindings {
            let old = previous
                .iter()
                .find(|(old_id, _)| old_id == id)
                .map(|(_, binding)| binding);
            if let Some(element_ref) = &current.element_ref {
                element_ref.publish(Some(current.snapshot.clone()));
            }
            if old.is_none()
                && let Some(listener) = current.listener.clone()
            {
                callbacks.push((listener, Some(current.snapshot.clone())));
            }
        }

        for (listener, snapshot) in callbacks {
            listener.call(snapshot);
            if let Some(fault) = crate::basic::events::take_callback_fault() {
                for (_, binding) in &mut self.element_bindings {
                    binding.listener = None;
                }
                return Err(crate::runtime::pipeline::RuntimeError::ApplicationCallback(
                    fault.message(),
                ));
            }
        }
        Ok(())
    }

    /// Builds the frame for `dom`: paints the scene, collects the event regions,
    /// hands focus to the topmost autofocus region, and publishes the result.
    ///
    /// The last autofocus region in paint order is the one on top, so a dialog or
    /// overlay wins focus over a region behind it.
    fn frame_for(
        &mut self,
        dom: Option<DomNode>,
        include_viewport: bool,
    ) -> Result<Frame, crate::runtime::pipeline::RuntimeError> {
        if let Some(dom) = dom {
            self.latest = Some(dom);
        }
        let Some(root) = self.latest.take() else {
            self.event_dispatcher.publish(Vec::new(), &HashSet::new());
            self.text_cache.clear();
            self.text_geometry.clear();
            self.text_seen.clear();
            self.publish_element_bindings(Vec::new())?;
            return Ok(make_frame(
                Vec::new(),
                include_viewport.then_some(self.viewport),
            ));
        };

        let mut next = HashMap::new();
        self.text_seen.clear();
        self.instrument.reset();
        let root_rect = self.root_rect(&root);
        let mut retained_scroll_ids = HashSet::new();
        collect_scroll_ids(&root, &mut retained_scroll_ids);
        let inherited = terminal_text();
        let mut order = 0;
        let mut event_order = 0;
        let mut event_regions = Vec::<EventRegion>::new();
        let mut measurement_paths = HashSet::new();
        collect_element_measurement_paths(&root, &mut measurement_paths);
        let mut element_bindings = Vec::new();
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
            &mut element_bindings,
            &measurement_paths,
            None,
            &mut event_order,
            (0, 0),
            None,
        );
        if let Some(region) = event_regions.iter().rev().find(|region| region.autofocus) {
            self.event_dispatcher.request_focus(region.id);
        }
        self.event_dispatcher
            .publish(event_regions, &retained_scroll_ids);
        self.publish_element_bindings(element_bindings)?;

        let (operations, order) = self.diff_scene(&next);
        self.scene_order = order;
        self.scene = next;
        self.latest = Some(root);
        self.text_cache.retain(|id, _| self.text_seen.contains(id));
        self.text_geometry
            .retain(|id, _| self.text_seen.contains(id));
        Ok(make_frame(
            operations,
            include_viewport.then_some(self.viewport),
        ))
    }
}

/// Atomic counters of intrinsic measurements and placements for one frame.
#[derive(Debug, Default)]
pub struct LayoutInstrument {
    intrinsic: std::sync::atomic::AtomicU64,
    placement: std::sync::atomic::AtomicU64,
}

impl LayoutInstrument {
    pub(crate) fn intrinsic(&self) {
        self.intrinsic
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn placement(&self) {
        self.placement
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn reset(&self) {
        self.intrinsic
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.placement
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns `(intrinsic measurements, placement visits)` since the last reset.
    pub fn counts(&self) -> (u64, u64) {
        (
            self.intrinsic.load(std::sync::atomic::Ordering::Relaxed),
            self.placement.load(std::sync::atomic::Ordering::Relaxed),
        )
    }
}

impl Commit {
    /// Builds a commit stage that shares its layout instrument with the caller.
    #[doc(hidden)]
    pub fn instrumented(
        viewport: Size,
    ) -> (Self, ViewportSetter, EventDispatcher, Arc<LayoutInstrument>) {
        let (commit, setter, dispatcher) =
            Self::with_config_and_events(viewport, CommitConfig::default());
        let instrument = commit.instrument.clone();
        (commit, setter, dispatcher, instrument)
    }
}

impl crate::runtime::pipeline::PipelineComponent for Commit {
    type Input = DomNode;
    type Output = Frame;

    const STAGE: crate::runtime::pipeline::Stage = crate::runtime::pipeline::Stage::Commit;

    fn run(
        mut self,
        input: Receiver<Self::Input>,
        output: Sender<Self::Output>,
        errors: Sender<crate::runtime::pipeline::RuntimeError>,
    ) -> Result<(), crate::runtime::pipeline::RuntimeError> {
        loop {
            crossbeam_channel::select! {
                recv(input) -> message => {
                    let Ok(dom) = message else { break; };
                    match self.frame_for(Some(dom), true) {
                        Ok(frame) => if output.send(frame).is_err() { break; },
                        Err(error) => { let _ = errors.send(error); break; }
                    }
                }
                recv(self.viewport_rx) -> _ => {
                    self.viewport = *self.viewport_state.lock().expect("viewport mutex poisoned");
                    if let Err(error) = self.output_latest(&output) {
                        let _ = errors.send(error);
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Commit {
    fn output_latest(
        &mut self,
        output: &Sender<Frame>,
    ) -> Result<(), crate::runtime::pipeline::RuntimeError> {
        let frame = self.frame_for(None, true)?;
        output
            .send(frame)
            .map_err(|_| crate::runtime::pipeline::RuntimeError::StageClosed {
                stage: crate::runtime::pipeline::Stage::Commit,
            })
    }
}

impl Drop for Commit {
    fn drop(&mut self) {
        self.event_dispatcher.publish(Vec::new(), &HashSet::new());
        for (_, binding) in self.element_bindings.drain(..) {
            if let Some(element_ref) = binding.element_ref {
                element_ref.clear();
            }
            if let Some(listener) = binding.listener {
                listener.call(None);
            }
        }
    }
}
