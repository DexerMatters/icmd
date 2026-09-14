use super::image::Passthrough;
use super::image::{
    ImageManager, KittyBackend, NativeTile, PreparedRaster, ScreenRect, TileKey, TransformKey,
    surround_native,
};
use super::pipeline::PipelineComponent;
use crate::data::{
    Cell, CellSlot, EmojiMerging, Frame, Image, ImageError, ImageId, MAX_SURFACE_CELLS, Operation,
    Rect, ScreenPosition, Size,
};
use crate::runtime::limits::ResourceLimits;
use crate::{
    ImageMode, ImageProtocol, ImageSource, ImageUpdatePolicy, RasterImage, RasterPlacement,
    raster::{RasterPixels, render_rgba_with_cell_size, symbols_from_pixels},
};
use crossbeam_channel::{Receiver, Sender};
use crossterm::cursor::{MoveTo, RestorePosition, SavePosition};
use crossterm::style::{
    Attribute, Attributes, Color, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{Clear, ClearType};
use std::collections::{HashMap, HashSet};
use std::error::Error;
#[cfg(feature = "native-raster")]
use std::ffi::CString;
use std::fmt;
use std::fmt::Write as _;
use std::ops::Range;
use std::time::{Duration, Instant};

const ADAPTIVE_REPLAY_DELAY: Duration = Duration::from_millis(80);

// Which representation a retained surface holds. Validation tracks it so an
// operation aimed at the wrong representation is rejected instead of silently
// doing nothing.
// Bounded-cardinality image pipeline metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImageMetrics {
    pub in_flight_bytes: usize,
    pub cached_source_bytes: usize,
    pub prepared_bytes: usize,
    pub native_cache_bytes: usize,
    pub pending_sources: usize,
    pub max_in_flight_bytes: usize,
    pub max_cache_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Cells,
    Raster,
}

impl fmt::Display for SurfaceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cells => write!(f, "cell surface"),
            Self::Raster => write!(f, "raster surface"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    DuplicateImage(ImageId),
    UnknownImage(ImageId),
    // An operation named the wrong kind of surface. The operation index and
    // surface ID make the offending entry unambiguous.
    WrongSurface {
        operation: usize,
        id: ImageId,
        expected: SurfaceKind,
        actual: SurfaceKind,
    },
    InvalidPatch {
        image: ImageId,
        error: ImageError,
    },
    SurfaceTooLarge {
        width: u16,
        height: u16,
        cells: usize,
    },
    // The requested raster protocol needs the `native-raster` feature. Emitted
    // before any worker or terminal resource exists, so capability is reported
    // deterministically instead of degrading silently.
    UnsupportedProtocol {
        protocol: ImageProtocol,
    },
    // The renderer configuration was rejected before resource creation.
    Config {
        detail: Box<str>,
    },
    // One frame's assembled terminal payload exceeded the configured output
    // budget. Nothing was written and the previously presented frame stands.
    OutputTooLarge {
        limit: usize,
        requested: usize,
    },
    AllocationFailed {
        cells: usize,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateImage(id) => write!(f, "image {id:?} already exists"),
            Self::UnknownImage(id) => write!(f, "image {id:?} does not exist"),
            Self::WrongSurface {
                operation,
                id,
                expected,
                actual,
            } => write!(
                f,
                "operation {operation} for {id:?} requires a {expected}, but the surface is a {actual}"
            ),
            Self::InvalidPatch { image, error } => {
                write!(f, "invalid patch for {image:?}: {error}")
            }
            Self::SurfaceTooLarge {
                width,
                height,
                cells,
            } => write!(
                f,
                "viewport {width}x{height} requests {cells} cells; maximum is {MAX_SURFACE_CELLS}"
            ),
            Self::AllocationFailed { cells } => {
                write!(f, "renderer allocation failed for {cells} cells")
            }
            Self::UnsupportedProtocol { protocol } => write!(
                f,
                "image protocol {protocol:?} requires the `native-raster` feature"
            ),
            Self::Config { detail } => write!(f, "invalid renderer configuration: {detail}"),
            Self::OutputTooLarge { limit, requested } => write!(
                f,
                "frame output of {requested} bytes exceeds the {limit}-byte budget"
            ),
        }
    }
}
impl Error for FrameError {}

#[derive(Debug, Clone, Copy)]
struct ValidatedSurface {
    size: (usize, usize),
    kind: SurfaceKind,
}

#[derive(Debug, Clone)]
pub(super) enum Surface {
    Cells(Image),
    Raster(RasterPlacement),
}

#[derive(Debug, Clone)]
pub(super) struct ImageNode {
    pub(super) surface: Surface,
    pub(super) fallback: Option<Image>,
    pub(super) position: ScreenPosition,
    pub(super) raster_clip: Option<Rect>,
    pub(super) level: i32,
    pub(super) order: u64,
    pub(super) mutation: u64,
}

#[derive(Debug, Clone)]
pub struct RendererConfig {
    pub image_protocol: ImageProtocol,
    pub image_cache_bytes: usize,
    pub image_update_policy: ImageUpdatePolicy,
    pub cell_pixel_size: Option<Size>,
    pub emoji_merging: EmojiMerging,
    // One validated budget for decode, transform, cache, and output work.
    pub limits: ResourceLimits,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            image_protocol: ImageProtocol::Auto,
            image_cache_bytes: 64 * 1024 * 1024,
            image_update_policy: ImageUpdatePolicy::Adaptive,
            cell_pixel_size: None,
            emoji_merging: EmojiMerging::default(),
            limits: ResourceLimits::default(),
        }
    }
}

#[derive(Debug)]
struct CachedPayload {
    value: String,
    used: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct LayerKey(i32, u64, u64, u64);

// SAFETY/invariants for this wrapper:
// * It is created and dropped inside `detect_terminal`, which runs on the
//   calling thread before the renderer worker starts; the pointer never
//   outlives that function. Only the derived `Passthrough` value crosses into
//   the worker, so no foreign object needs a thread-transfer guarantee.
// * `chafa_term_db_get_default` returns a borrowed, library-owned database and
//   `chafa_term_db_detect` transfers one reference to the caller. `Drop`
//   releases exactly that reference and is the only release path.
// * Every use null-checks the pointer and holds no other borrow across the
//   unref.
#[cfg(feature = "native-raster")]
struct TermInfo(*mut chafa_sys::ChafaTermInfo);

#[cfg(feature = "native-raster")]
impl Drop for TermInfo {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { chafa_sys::chafa_term_info_unref(self.0) };
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Damage {
    line: i64,
    column: i64,
    width: i64,
    height: i64,
}

impl Damage {
    fn new(line: i64, column: i64, width: usize, height: usize) -> Self {
        Self {
            line,
            column,
            width: width as i64,
            height: height as i64,
        }
    }
    fn union(self, other: Self) -> Self {
        let left = self.column.min(other.column);
        let top = self.line.min(other.line);
        let right = self
            .column
            .saturating_add(self.width)
            .max(other.column.saturating_add(other.width));
        let bottom = self
            .line
            .saturating_add(self.height)
            .max(other.line.saturating_add(other.height));
        Self {
            line: top,
            column: left,
            width: right - left,
            height: bottom - top,
        }
    }
}

pub struct Renderer {
    pub(super) viewport: Size,
    // Retained scene nodes remain protocol-independent; image-specific state
    // lives in the manager/backend fields below.
    pub(super) images: HashMap<ImageId, ImageNode>,
    next_order: u64,
    next_mutation: u64,
    last: Vec<CellSlot>,
    scratch: Vec<CellSlot>,
    dirty: Vec<bool>,
    // Which cells `dirty` currently marks, so it can be cleared in O(damage)
    // instead of a viewport-wide fill every frame.
    dirty_marks: Vec<(usize, Range<usize>)>,
    // Counter: cells compared by the last frame encode. A one-cell patch must
    // keep this bounded by damage, independent of viewport area.
    cells_examined: u64,
    changed: Vec<bool>,
    damage: Vec<Damage>,
    // Damage is normalized into row spans before composition. `owners` stores
    // the one-based ordinal of the surface that supplied each scratch cell.
    // It makes wide-glyph validation constant-time without another scene walk.
    damage_rows: Vec<Vec<Range<usize>>>,
    owners: Vec<usize>,
    layers: Vec<ImageId>,
    layers_dirty: bool,
    full_redraw: bool,
    pub(super) protocol: ImageProtocol,
    passthrough: Passthrough,
    // Shared resource policy: decode/transform/cache/output ceilings.
    pub(super) limits: ResourceLimits,
    // Source I/O and fallback transitions are shared by every output mode.
    pub(super) image_manager: ImageManager,
    native_cache: HashMap<NativeKey, CachedPayload>,
    native_cache_bytes: usize,
    native_cache_limit: usize,
    prepared: HashMap<TransformKey, PreparedRaster>,
    prepared_bytes: usize,
    cache_tick: u64,
    pub(super) cell_pixels: Size,
    detect_cell_pixels: bool,
    image_update_policy: ImageUpdatePolicy,
    pub(super) symbols_for_native: bool,
    native_replay_at: Option<Instant>,
    pub(super) raster_scene_dirty: bool,
    native_tiles: HashSet<TileKey>,
    kitty: KittyBackend,
    emoji_merging: EmojiMerging,
}

impl Renderer {
    pub fn new(viewport: Size) -> Result<Self, FrameError> {
        Self::with_config(viewport, RendererConfig::default())
    }

    pub fn with_config(viewport: Size, config: RendererConfig) -> Result<Self, FrameError> {
        // Validate the configuration before allocating any cell or native
        // resource. A cell pixel size that can never satisfy the transform
        // budget is rejected here rather than failing later during a resize.
        config
            .limits
            .validate()
            .map_err(|error| FrameError::Config {
                detail: error.to_string().into(),
            })?;
        if let Some(size) = config.cell_pixel_size {
            if size.width == 0 {
                return Err(FrameError::Config {
                    detail: "cell pixel width must be nonzero".into(),
                });
            }
            if size.height == 0 {
                return Err(FrameError::Config {
                    detail: "cell pixel height must be nonzero".into(),
                });
            }
            // The renderer must be able to transform a raster that fills the
            // viewport; that is the geometry every frame can actually request.
            // A single oversized source is refused later, per transform, by
            // the same budget.
            let viewport_pixels = u64::from(viewport.width)
                .saturating_mul(u64::from(viewport.height))
                .saturating_mul(u64::from(size.width))
                .saturating_mul(u64::from(size.height));
            if viewport_pixels > config.limits.max_transform_pixels {
                return Err(FrameError::Config {
                    detail: "renderer cell pixel size cannot satisfy the transform pixel budget"
                        .into(),
                });
            }
        }
        let cells = blank_cells(viewport)?;
        let cell_count = cells.len();
        let (protocol, passthrough) = detect_terminal(config.image_protocol);
        #[cfg(not(feature = "native-raster"))]
        if matches!(
            protocol,
            ImageProtocol::Kitty | ImageProtocol::Sixel | ImageProtocol::Iterm2
        ) {
            return Err(FrameError::UnsupportedProtocol { protocol });
        }
        let detect_cell_pixels = config.cell_pixel_size.is_none();
        let cell_pixels = config
            .cell_pixel_size
            .or_else(live_cell_pixels)
            .map(|size| Size::new(size.width.max(1), size.height.max(1)))
            .unwrap_or(Size::new(8, 16));
        Ok(Self {
            viewport,
            images: HashMap::new(),
            next_order: 0,
            next_mutation: 0,
            last: cells,
            scratch: Vec::with_capacity(cell_count),
            dirty: vec![false; cell_count],
            dirty_marks: Vec::new(),
            cells_examined: 0,
            changed: vec![false; cell_count],
            damage: Vec::new(),
            damage_rows: vec![Vec::new(); usize::from(viewport.height)],
            owners: vec![0; cell_count],
            layers: Vec::new(),
            layers_dirty: true,
            full_redraw: true,
            protocol,
            passthrough,
            limits: config.limits,
            image_manager: ImageManager::with_limits(config.limits),
            native_cache: HashMap::new(),
            native_cache_bytes: 0,
            native_cache_limit: config.image_cache_bytes,
            prepared: HashMap::new(),
            prepared_bytes: 0,
            cache_tick: 0,
            cell_pixels,
            detect_cell_pixels,
            image_update_policy: config.image_update_policy,
            symbols_for_native: false,
            native_replay_at: None,
            raster_scene_dirty: true,
            native_tiles: HashSet::new(),
            kitty: KittyBackend::new(),
            emoji_merging: config.emoji_merging,
        })
    }

    // Structured, bounded observability for the image pipeline. Labels are
    // stage/kind only; no path or text content is exposed.
    // Cells compared by the most recent frame encode, and the viewport size it
    // was compared against. Bounded-cardinality renderer metrics.
    pub fn take_cells_examined(&mut self) -> (u64, usize) {
        let examined = self.cells_examined;
        self.cells_examined = 0;
        (
            examined,
            usize::from(self.viewport.width) * usize::from(self.viewport.height),
        )
    }

    pub fn image_metrics(&self) -> ImageMetrics {
        ImageMetrics {
            in_flight_bytes: self.image_manager.bytes_in_flight(),
            cached_source_bytes: self.image_manager.source_cache_bytes(),
            prepared_bytes: self.prepared_bytes,
            native_cache_bytes: self.native_cache_bytes,
            pending_sources: self.image_manager.pending_count(),
            max_in_flight_bytes: self.limits.max_in_flight_image_bytes,
            max_cache_bytes: self.image_manager.limits().max_cache_bytes,
        }
    }

    // Test-only hook so a budget rejection can be observed followed by a
    // successful render on the same renderer.
    #[doc(hidden)]
    pub fn set_output_budget_for_test(&mut self, bytes: usize) {
        self.limits.max_output_bytes_per_frame = bytes;
    }

    pub fn viewport(&self) -> Size {
        self.viewport
    }

    fn raster_node(
        &self,
        raster: RasterPlacement,
        position: ScreenPosition,
        level: i32,
        order: u64,
        mutation: u64,
    ) -> ImageNode {
        let fallback = self.image_manager.initial_fallback(&raster);
        ImageNode {
            surface: Surface::Raster(raster),
            fallback,
            position,
            raster_clip: None,
            level,
            order,
            mutation,
        }
    }

    pub fn apply_frame(&mut self, frame: Frame) -> Result<(), FrameError> {
        self.validate_frame(&frame)?;
        if let Some(viewport) = frame.viewport
            && viewport != self.viewport
        {
            self.refresh_cell_pixels();
            let cells = blank_cells(viewport)?;
            self.viewport = viewport;
            self.last = cells;
            self.scratch.clear();
            self.dirty.clear();
            self.dirty.resize(self.last.len(), false);
            self.changed.clear();
            self.changed.resize(self.last.len(), false);
            self.damage.clear();
            self.damage_rows.clear();
            self.damage_rows
                .resize_with(usize::from(viewport.height), Vec::new);
            self.owners.clear();
            self.owners.resize(self.last.len(), 0);
            self.full_redraw = true;
            self.raster_scene_dirty = true;
        }
        if frame.force_redraw {
            self.full_redraw = true;
            self.raster_scene_dirty = true;
        }

        for operation in frame.operations {
            // Cell patches preserve a fragment's footprint, so they cannot
            // change native visibility. Every other operation can change
            // ownership, geometry, or source content.
            if !matches!(
                operation,
                Operation::PatchRect { .. } | Operation::PatchCells { .. }
            ) {
                self.raster_scene_dirty = true;
            }
            match operation {
                Operation::Create {
                    id,
                    image,
                    position,
                    level,
                } => {
                    self.add_damage(Self::node_damage_for(&ImageNode {
                        surface: Surface::Cells(image.clone()),
                        fallback: None,
                        position,
                        raster_clip: None,
                        level,
                        order: self.next_order,
                        mutation: self.next_mutation,
                    }));
                    self.images.insert(
                        id,
                        ImageNode {
                            surface: Surface::Cells(image),
                            fallback: None,
                            position,
                            raster_clip: None,
                            level,
                            order: self.next_order,
                            mutation: self.next_mutation,
                        },
                    );
                    self.layers_dirty = true;
                    self.next_order = self.next_order.saturating_add(1);
                    self.next_mutation = self.next_mutation.saturating_add(1);
                }
                Operation::CreateRaster {
                    id,
                    raster,
                    position,
                    level,
                } => {
                    let node = self.raster_node(
                        raster,
                        position,
                        level,
                        self.next_order,
                        self.next_mutation,
                    );
                    self.add_damage(Self::node_damage_for(&node));
                    self.images.insert(id, node);
                    self.layers_dirty = true;
                    self.next_order = self.next_order.saturating_add(1);
                    self.next_mutation = self.next_mutation.saturating_add(1);
                    self.full_redraw = true;
                }
                Operation::Remove { id } => {
                    if let Some(node) = self.images.remove(&id) {
                        self.add_damage(Self::node_damage_for(&node));
                        self.layers_dirty = true;
                    }
                }
                Operation::Move { id, position } => {
                    if let Some(node) = self.images.get_mut(&id) {
                        let old = Self::node_damage_for(node);
                        node.position = position;
                        let new = Self::node_damage_for(node);
                        self.add_damage(old.union(new));
                    }
                }
                Operation::SetLevel { id, level } => {
                    let damage = self.images.get_mut(&id).map(|node| {
                        let damage = Self::node_damage_for(node);
                        node.level = level;
                        damage
                    });
                    if let Some(damage) = damage {
                        self.add_damage(damage);
                        self.layers_dirty = true;
                    }
                }
                Operation::SetOrder { id, order } => {
                    let mutation = self.next_mutation;
                    let damage = self.images.get_mut(&id).map(|node| {
                        let damage = Self::node_damage_for(node);
                        node.order = order;
                        node.mutation = mutation;
                        damage
                    });
                    self.next_order = self.next_order.max(order.saturating_add(1));
                    self.next_mutation = self.next_mutation.saturating_add(1);
                    if let Some(damage) = damage {
                        self.add_damage(damage);
                        self.layers_dirty = true;
                    }
                }
                Operation::Replace { id, image } => {
                    if let Some(node) = self.images.get_mut(&id) {
                        let old = Self::node_damage_for(node);
                        node.surface = Surface::Cells(image);
                        node.fallback = None;
                        let new = Self::node_damage_for(node);
                        self.add_damage(old.union(new));
                    }
                }
                Operation::ReplaceRaster { id, raster } => {
                    let fallback = self.image_manager.initial_fallback(&raster);
                    if let Some(node) = self.images.get_mut(&id) {
                        let old = Self::node_damage_for(node);
                        node.fallback = fallback;
                        node.surface = Surface::Raster(raster);
                        let new = Self::node_damage_for(node);
                        self.add_damage(old.union(new));
                        self.full_redraw = true;
                    }
                }
                Operation::SetRasterClip { id, clip } => {
                    let damage = if let Some(node) = self.images.get_mut(&id) {
                        let old = Self::node_damage_for(node);
                        node.raster_clip = clip;
                        Some(old.union(Self::node_damage_for(node)))
                    } else {
                        None
                    };
                    if let Some(damage) = damage {
                        self.add_damage(damage);
                    }
                }
                Operation::PatchRect { id, rect, rows } => {
                    let damage = self.images.get_mut(&id).map(|node| {
                        let Surface::Cells(image) = &mut node.surface else {
                            return Damage::new(0, 0, 0, 0);
                        };
                        let local = image
                            .patch_rect(rect, &rows)
                            .expect("frame validation guarantees patch validity");
                        Self::node_damage_for_rect(node, local)
                    });
                    if let Some(damage) = damage {
                        self.add_damage(damage);
                    }
                }
                Operation::PatchCells { id, edits } => {
                    let damage = self.images.get_mut(&id).map(|node| {
                        let Surface::Cells(image) = &mut node.surface else {
                            return Damage::new(0, 0, 0, 0);
                        };
                        let local = image
                            .patch_cells(&edits)
                            .expect("frame validation guarantees patch validity");
                        Self::node_damage_for_rect(node, local)
                    });
                    if let Some(damage) = damage {
                        self.add_damage(damage);
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_frame(&self, frame: &Frame) -> Result<(), FrameError> {
        if let Some(viewport) = frame.viewport {
            validate_viewport(viewport)?;
        }
        // Keep only frame-local overrides. Cloning the dimensions of every
        // retained image made validating a one-cell patch scale with scene
        // size even though validation is otherwise purely transactional.
        let mut dimensions: HashMap<ImageId, Option<ValidatedSurface>> = HashMap::new();
        let lookup = |id: &ImageId, changes: &HashMap<ImageId, Option<ValidatedSurface>>| {
            changes.get(id).copied().flatten().or_else(|| {
                (!changes.contains_key(id))
                    .then(|| self.images.get(id))
                    .flatten()
                    .map(|node| ValidatedSurface {
                        size: surface_size(&node.surface),
                        kind: match node.surface {
                            Surface::Cells(_) => SurfaceKind::Cells,
                            Surface::Raster(_) => SurfaceKind::Raster,
                        },
                    })
            })
        };
        // Reject an operation whose required surface kind differs from what the
        // ID currently holds. `require` keeps transactional behavior: every
        // mismatch is found before the renderer is mutated.
        let require = |id: &ImageId,
                       expected: SurfaceKind,
                       operation: usize,
                       changes: &HashMap<ImageId, Option<ValidatedSurface>>|
         -> Result<ValidatedSurface, FrameError> {
            let Some(surface) = lookup(id, changes) else {
                return Err(FrameError::UnknownImage(*id));
            };
            if surface.kind != expected {
                return Err(FrameError::WrongSurface {
                    operation,
                    id: *id,
                    expected,
                    actual: surface.kind,
                });
            }
            Ok(surface)
        };
        for (index, operation) in frame.operations.iter().enumerate() {
            match operation {
                Operation::Create { id, image, .. } => {
                    if lookup(id, &dimensions).is_some() {
                        return Err(FrameError::DuplicateImage(*id));
                    }
                    dimensions.insert(
                        *id,
                        Some(ValidatedSurface {
                            size: (image.width(), image.height()),
                            kind: SurfaceKind::Cells,
                        }),
                    );
                }
                Operation::CreateRaster { id, raster, .. } => {
                    if lookup(id, &dimensions).is_some() {
                        return Err(FrameError::DuplicateImage(*id));
                    }
                    dimensions.insert(
                        *id,
                        Some(ValidatedSurface {
                            size: (usize::from(raster.width), usize::from(raster.height)),
                            kind: SurfaceKind::Raster,
                        }),
                    );
                }
                Operation::Remove { id } => {
                    if lookup(id, &dimensions).is_none() {
                        return Err(FrameError::UnknownImage(*id));
                    }
                    dimensions.insert(*id, None);
                }
                Operation::Move { id, .. }
                | Operation::SetLevel { id, .. }
                | Operation::SetOrder { id, .. } => {
                    if lookup(id, &dimensions).is_none() {
                        return Err(FrameError::UnknownImage(*id));
                    }
                }
                Operation::Replace { id, image } => {
                    if lookup(id, &dimensions).is_none() {
                        return Err(FrameError::UnknownImage(*id));
                    }
                    dimensions.insert(
                        *id,
                        Some(ValidatedSurface {
                            size: (image.width(), image.height()),
                            kind: SurfaceKind::Cells,
                        }),
                    );
                }
                Operation::ReplaceRaster { id, raster } => {
                    if lookup(id, &dimensions).is_none() {
                        return Err(FrameError::UnknownImage(*id));
                    }
                    dimensions.insert(
                        *id,
                        Some(ValidatedSurface {
                            size: (usize::from(raster.width), usize::from(raster.height)),
                            kind: SurfaceKind::Raster,
                        }),
                    );
                }
                Operation::SetRasterClip { id, .. } => {
                    require(id, SurfaceKind::Raster, index, &dimensions)?;
                }
                Operation::PatchRect { id, rect, rows } => {
                    let surface = require(id, SurfaceKind::Cells, index, &dimensions)?;
                    let (width, height) = surface.size;
                    let row_width_error = rows.iter().find_map(|row| {
                        let width = row
                            .iter()
                            .try_fold(0usize, |sum, cell| sum.checked_add(cell.width()));
                        (width != Some(rect.width)).then_some(if width.is_none() {
                            ImageError::RowWidthOverflow
                        } else {
                            ImageError::RowWidthMismatch
                        })
                    });
                    let error = if rect.width == 0
                        || rect.height == 0
                        || rect.right() > width
                        || rect.bottom() > height
                    {
                        Some(ImageError::OutOfBounds)
                    } else if rows.len() != rect.height {
                        Some(ImageError::WrongCellCount)
                    } else {
                        row_width_error
                    };
                    if let Some(error) = error {
                        return Err(FrameError::InvalidPatch { image: *id, error });
                    }
                }
                Operation::PatchCells { id, edits } => {
                    let surface = require(id, SurfaceKind::Cells, index, &dimensions)?;
                    let (width, height) = surface.size;
                    if edits.iter().any(|edit| {
                        edit.position.column >= width
                            || edit.position.line >= height
                            || (edit.cell.width() == 2
                                && edit.position.column.saturating_add(1) >= width)
                    }) {
                        return Err(FrameError::InvalidPatch {
                            image: *id,
                            error: ImageError::OutOfBounds,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn node_damage_for(node: &ImageNode) -> Damage {
        let (width, height) = surface_size(&node.surface);
        Damage::new(
            node.position.line as i64,
            node.position.column as i64,
            width,
            height,
        )
    }
    fn node_damage_for_rect(node: &ImageNode, rect: Rect) -> Damage {
        Damage::new(
            node.position.line as i64 + rect.line as i64,
            node.position.column as i64 + rect.column as i64,
            rect.width,
            rect.height,
        )
    }
    pub(super) fn add_damage(&mut self, damage: Damage) {
        if damage.width > 0 && damage.height > 0 {
            if self.damage.len() >= 4096 {
                self.damage.clear();
                self.full_redraw = true;
            } else {
                self.damage.push(damage);
            }
        }
    }

    pub(super) fn prepare_raster(&mut self, raster: &RasterPlacement) -> Option<TransformKey> {
        let key = self.transform_key(raster)?;
        self.cache_tick = self.cache_tick.saturating_add(1);
        if let Some(entry) = self.prepared.get_mut(&key) {
            entry.used = self.cache_tick;
            return Some(key);
        }
        let source = self.source_image(&raster.source)?;
        // A transform that cannot fit the configured budget is refused here,
        // before its pixel buffer is reserved.
        let pixels = render_rgba_with_cell_size(
            &source,
            raster.full_width,
            raster.full_height,
            raster.options,
            self.cell_pixels,
            &self.limits,
        )
        .ok()?;
        let mut alpha_cells =
            vec![false; usize::from(raster.full_width) * usize::from(raster.full_height)];
        for line in 0..usize::from(raster.full_height) {
            for column in 0..usize::from(raster.full_width) {
                let px_left = column * self.cell_pixels.width as usize;
                let px_top = line * self.cell_pixels.height as usize;
                let px_right =
                    (px_left + self.cell_pixels.width as usize).min(pixels.width as usize);
                let px_bottom = (px_top + self.cell_pixels.height as usize)
                    .min(pixels.pixels.len() / (pixels.width as usize * 4));
                alpha_cells[line * usize::from(raster.full_width) + column] = (px_top..px_bottom)
                    .any(|y| {
                        (px_left..px_right)
                            .any(|x| pixels.pixels[(y * pixels.width as usize + x) * 4 + 3] != 0)
                    });
            }
        }
        let bytes = pixels.pixels.len().saturating_add(alpha_cells.len());
        self.evict_cache(bytes);
        self.prepared_bytes = self.prepared_bytes.saturating_add(bytes);
        self.prepared.insert(
            key,
            PreparedRaster {
                pixels,
                alpha_cells,
                symbols: None,
                bytes,
                used: self.cache_tick,
            },
        );
        Some(key)
    }

    pub(super) fn prepare_symbols(&mut self, key: TransformKey, width: u16, height: u16) {
        let needs_symbols = self
            .prepared
            .get(&key)
            .is_some_and(|entry| entry.symbols.is_none());
        if !needs_symbols {
            return;
        }
        let image = self.prepared.get(&key).and_then(|entry| {
            symbols_from_pixels(&entry.pixels, width, height, key.cell_pixels).ok()
        });
        let Some(image) = image else { return };
        let bytes = image
            .width()
            .saturating_mul(image.height())
            .saturating_mul(48);
        self.evict_cache(bytes);
        if let Some(entry) = self.prepared.get_mut(&key) {
            entry.bytes = entry.bytes.saturating_add(bytes);
            entry.symbols = Some(image);
            entry.used = self.cache_tick;
            self.prepared_bytes = self.prepared_bytes.saturating_add(bytes);
        }
    }

    fn refresh_cell_pixels(&mut self) {
        if !self.detect_cell_pixels {
            return;
        }
        let Some(cell_pixels) = live_cell_pixels() else {
            return;
        };
        if cell_pixels != self.cell_pixels {
            self.cell_pixels = cell_pixels;
            self.full_redraw = true;
        }
    }

    pub(super) fn evict_cache(&mut self, incoming: usize) {
        // Encoded protocol payloads are cheapest to regenerate, so discard
        // them before transformed RGBA data. Current transforms are pinned:
        // a cache limit never causes the active scene to thrash itself.
        while self.cache_bytes().saturating_add(incoming) > self.native_cache_limit {
            let Some(key) = self
                .native_cache
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.remove_native_cache(key);
        }

        let pinned: HashSet<_> = self
            .images
            .values()
            .filter_map(|node| match &node.surface {
                Surface::Raster(raster) => self.transform_key(raster),
                Surface::Cells(_) => None,
            })
            .collect();
        while self.cache_bytes().saturating_add(incoming) > self.native_cache_limit {
            let Some(key) = self
                .prepared
                .iter()
                .filter(|(key, _)| !pinned.contains(key))
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(entry) = self.prepared.remove(&key) {
                self.prepared_bytes = self.prepared_bytes.saturating_sub(entry.bytes);
                self.kitty.evict_transform(key, &mut self.native_tiles);
            }
        }

        let pinned_sources: HashSet<crate::raster::ImageSourceKey> = self
            .images
            .values()
            .filter_map(|node| match &node.surface {
                Surface::Raster(raster) if self.source_should_load(node, raster) => {
                    Some(raster.source.cache_key())
                }
                _ => None,
            })
            .collect();
        while self.cache_bytes().saturating_add(incoming) > self.native_cache_limit {
            let Some(source) = self.image_manager.oldest_inactive_source(&pinned_sources) else {
                break;
            };
            self.image_manager.remove_cached_key(&source);
        }
    }

    fn cache_bytes(&self) -> usize {
        self.image_manager
            .source_cache_bytes()
            .saturating_add(self.prepared_bytes)
            .saturating_add(self.native_cache_bytes)
    }

    fn remove_native_cache(&mut self, key: NativeKey) {
        if let Some(value) = self.native_cache.remove(&key) {
            self.native_cache_bytes = self.native_cache_bytes.saturating_sub(value.value.len());
        }
    }

    fn raster_clip_contains(node: &ImageNode, local_line: usize, local_column: usize) -> bool {
        node.raster_clip.is_none_or(|clip| {
            local_line >= clip.line
                && local_column >= clip.column
                && local_line < clip.bottom()
                && local_column < clip.right()
        })
    }

    pub fn render_diff(&mut self) -> Result<Option<String>, FrameError> {
        let load_changed = self.drain_load_results();
        let replay_due = self.native_replay_at.is_some_and(|at| at <= Instant::now());
        if !self.full_redraw && self.damage.is_empty() && !replay_due && !load_changed {
            return Ok(None);
        }
        self.prepare_visible_rasters();
        if replay_due {
            self.symbols_for_native = false;
            self.native_replay_at = None;
        }
        let needs_native_tiles =
            !self.symbols_for_native && (self.full_redraw || replay_due || self.raster_scene_dirty);
        let mut native_tiles = if needs_native_tiles {
            self.collect_native_tiles()
        } else {
            Vec::new()
        };
        let native_changed = needs_native_tiles
            && self.native_tiles != native_tiles.iter().map(|tile| tile.key).collect();
        let replay_only = matches!(self.protocol, ImageProtocol::Sixel | ImageProtocol::Iterm2);
        let use_preview = replay_only
            && native_changed
            && !native_tiles.is_empty()
            && !self.native_tiles.is_empty()
            && self.image_update_policy == ImageUpdatePolicy::Adaptive
            && !replay_due;
        if use_preview {
            self.symbols_for_native = true;
            self.native_replay_at = Some(Instant::now() + ADAPTIVE_REPLAY_DELAY);
            self.prepare_visible_rasters();
            native_tiles.clear();
        } else if self.symbols_for_native && self.raster_scene_dirty {
            // A new mutation arrived before the quiet period elapsed. Render
            // the cached symbol scene now and restart the single replay timer.
            self.native_replay_at = Some(Instant::now() + ADAPTIVE_REPLAY_DELAY);
            native_tiles.clear();
        }
        let full_redraw = self.full_redraw || replay_due || (replay_only && native_changed);
        // Everything below is staged: `self.last` and every retained cache is
        // mutated only after the complete payload fits the frame output budget.
        // An over-budget frame therefore leaves the presented state untouched.
        let mut desired = std::mem::take(&mut self.scratch);
        if desired.capacity() < self.last.len()
            && desired
                .try_reserve_exact(self.last.len() - desired.capacity())
                .is_err()
        {
            self.scratch = desired;
            return Err(FrameError::AllocationFailed {
                cells: self.last.len(),
            });
        }
        self.normalize_damage(full_redraw);
        // Refresh only the rows this frame will touch. The scratch buffer keeps
        // a mirror of the presented frame for every row it is about to read, so
        // a one-cell change no longer copies the entire viewport. A viewport
        // change or full redraw touches every row and therefore rebuilds it.
        if desired.len() != self.last.len() {
            desired.clear();
            desired.extend_from_slice(&self.last);
        } else {
            self.refresh_desired_rows(&mut desired);
        }
        self.compose_damage(&mut desired);
        let ansi = match encode_diff(
            &self.last,
            &desired,
            self.viewport,
            &self.damage_rows,
            full_redraw,
            self.emoji_merging,
            &mut self.changed,
            &mut self.cells_examined,
        ) {
            Ok(ansi) => ansi,
            Err(error) => {
                self.clear_dirty_marks();
                self.scratch = desired;
                return Err(error);
            }
        };
        if full_redraw && self.protocol != ImageProtocol::Kitty {
            self.native_tiles.clear();
        }
        let native = if self.symbols_for_native || !needs_native_tiles {
            String::new()
        } else {
            self.native_output(native_tiles, full_redraw)
        };
        // Bound the assembled payload before it can reach the terminal. A
        // single valid render that exceeds the budget fails here, so no partial
        // escape sequence is ever written and presented state stays unchanged.
        let total = ansi
            .as_ref()
            .map_or(0usize, String::len)
            .saturating_add(native.len());
        super::metrics::note_output_bytes(total);
        if total > self.limits.max_output_bytes_per_frame {
            // Pending damage is deliberately retained so a later attempt with a
            // larger budget (or smaller scene) can re-encode it. Only the
            // staged buffers are returned; nothing presented changes.
            self.scratch = desired;
            return Err(FrameError::OutputTooLarge {
                limit: self.limits.max_output_bytes_per_frame,
                requested: total,
            });
        }
        // Apply only the rows that changed, then keep the composed buffer as
        // scratch. Non-damaged rows of the scratch buffer are refreshed from
        // the presented frame before they are next read.
        self.apply_desired_rows(&desired);
        self.scratch = desired;
        self.clear_dirty_marks();
        self.damage.clear();
        self.full_redraw = false;
        if self.symbols_for_native {
            self.raster_scene_dirty = true;
        } else if needs_native_tiles {
            self.raster_scene_dirty = false;
        }
        let output = match (ansi, native) {
            (None, native) if native.is_empty() => None,
            (Some(cells), native) if native.is_empty() => Some(cells),
            (Some(mut cells), native) => {
                cells.push_str(&native);
                Some(cells)
            }
            (None, native) => Some(native),
        };
        if output.is_some() {
            super::metrics::note_frame_presented();
        }
        Ok(output)
    }

    fn native_output(&mut self, tiles: Vec<NativeTile>, replay: bool) -> String {
        if self.protocol == ImageProtocol::Symbols {
            self.native_tiles.clear();
            return String::new();
        }
        match self.protocol {
            ImageProtocol::Kitty => self.kitty_output(tiles, replay),
            ImageProtocol::Sixel | ImageProtocol::Iterm2 | ImageProtocol::Auto => {
                let next: HashSet<_> = tiles.iter().map(|tile| tile.key).collect();
                if !replay && next == self.native_tiles {
                    return String::new();
                }
                let mut output = String::new();
                for tile in &tiles {
                    if let Some(payload) = self.native_payload(tile) {
                        let _ = write!(
                            output,
                            "{}{}",
                            MoveTo(tile.rect.column as u16, tile.rect.line as u16),
                            payload
                        );
                    }
                }
                self.native_tiles = next;
                surround_native(output)
            }
            ImageProtocol::Symbols => unreachable!(),
        }
    }

    fn native_replay_wait(&self) -> Option<Duration> {
        self.native_replay_at
            .map(|at| at.saturating_duration_since(Instant::now()))
    }

    fn native_payload(&mut self, tile: &NativeTile) -> Option<String> {
        let key = NativeKey::from_tile(tile.key, self.protocol);
        self.cache_tick = self.cache_tick.saturating_add(1);
        if let Some(value) = self.native_cache.get_mut(&key) {
            value.used = self.cache_tick;
            return Some(value.value.clone());
        }
        let prepared = self.prepared.get(&tile.key.transform)?;
        let value = encode_native_slice(
            &prepared.pixels,
            tile.key.source_column,
            tile.key.source_line,
            tile.key.width,
            tile.key.height,
            self.cell_pixels,
            self.protocol,
            self.passthrough,
        )?;
        if value.len() <= self.native_cache_limit {
            self.evict_cache(value.len());
            self.native_cache_bytes = self.native_cache_bytes.saturating_add(value.len());
            self.native_cache.insert(
                key,
                CachedPayload {
                    value: value.clone(),
                    used: self.cache_tick,
                },
            );
        }
        Some(value)
    }

    fn kitty_output(&mut self, tiles: Vec<NativeTile>, replace_all: bool) -> String {
        self.kitty.output(
            tiles,
            replace_all,
            &mut self.native_tiles,
            &self.prepared,
            self.cell_pixels,
            self.passthrough,
        )
    }

    fn shutdown_native(&mut self) -> Option<String> {
        if self.protocol != ImageProtocol::Kitty {
            return None;
        }
        self.native_tiles.clear();
        self.kitty.shutdown(self.passthrough)
    }

    fn collect_native_tiles(&mut self) -> Vec<NativeTile> {
        if self.symbols_for_native || self.protocol == ImageProtocol::Symbols {
            return Vec::new();
        }
        let owners = self.cell_owners();
        let mut nodes: Vec<_> = self
            .images
            .iter()
            .map(|(id, node)| (*id, node.clone()))
            .collect();
        nodes.sort_by_key(|(id, node)| (node.level, node.order, node.mutation, id.0));
        let mut result = Vec::new();
        for (id, node) in nodes {
            let Surface::Raster(raster) = &node.surface else {
                continue;
            };
            if raster.options.mode == ImageMode::Symbols || !self.raster_is_visible(&node, raster) {
                continue;
            }
            let Some(transform) = self.prepare_raster(raster) else {
                continue;
            };
            let Some(prepared) = self.prepared.get(&transform) else {
                continue;
            };
            for rect in self.raster_tiles(id, &node, &prepared.alpha_cells, &owners) {
                let source_column = rect.column.saturating_sub(node.position.column) as u16;
                let source_line = rect.line.saturating_sub(node.position.line) as u16;
                result.push(NativeTile {
                    key: TileKey {
                        raster: id,
                        transform,
                        source_column,
                        source_line,
                        destination_column: rect.column,
                        destination_line: rect.line,
                        width: rect.width as u16,
                        height: rect.height as u16,
                        level: node.level,
                        order: node.order,
                    },
                    rect,
                });
            }
        }
        result
    }

    fn cell_owners(&self) -> Vec<Option<LayerKey>> {
        let mut owners =
            vec![None; usize::from(self.viewport.width) * usize::from(self.viewport.height)];
        for (id, node) in &self.images {
            let Some(image) = self.cell_surface(node) else {
                continue;
            };
            let raster_alpha = match &node.surface {
                Surface::Raster(raster) => self
                    .transform_key(raster)
                    .and_then(|key| self.prepared.get(&key))
                    .map(|prepared| prepared.alpha_cells.as_slice()),
                Surface::Cells(_) => None,
            };
            let rank = LayerKey(node.level, node.order, node.mutation, id.0);
            for local_line in 0..image.height() {
                let line = node.position.line.saturating_add(local_line as i32);
                if !(0..i32::from(self.viewport.height)).contains(&line) {
                    continue;
                }
                for local_column in 0..image.width() {
                    if matches!(node.surface, Surface::Raster(_))
                        && !Self::raster_clip_contains(node, local_line, local_column)
                    {
                        continue;
                    }
                    if raster_alpha.is_some_and(|alpha| {
                        !alpha
                            .get(local_line * image.width() + local_column)
                            .copied()
                            .unwrap_or(false)
                    }) {
                        continue;
                    }
                    let column = node.position.column.saturating_add(local_column as i32);
                    if !(0..i32::from(self.viewport.width)).contains(&column) {
                        continue;
                    }
                    let index = line as usize * usize::from(self.viewport.width) + column as usize;
                    if owners[index].is_none_or(|current| rank > current) {
                        owners[index] = Some(rank);
                    }
                }
            }
        }
        owners
    }

    fn cell_surface<'a>(&'a self, node: &'a ImageNode) -> Option<&'a Image> {
        match &node.surface {
            Surface::Cells(image) => Some(image),
            Surface::Raster(raster)
                if self.symbols_for_native
                    || self.protocol == ImageProtocol::Symbols
                    || raster.options.mode == ImageMode::Symbols =>
            {
                self.transform_key(raster)
                    .and_then(|key| self.prepared.get(&key))
                    .and_then(|entry| entry.symbols.as_ref())
                    .or(node.fallback.as_ref())
            }
            Surface::Raster(_) => node.fallback.as_ref(),
        }
    }

    fn raster_tiles(
        &self,
        id: ImageId,
        node: &ImageNode,
        alpha_cells: &[bool],
        owners: &[Option<LayerKey>],
    ) -> Vec<ScreenRect> {
        let Surface::Raster(raster) = &node.surface else {
            return Vec::new();
        };
        let clip = node.raster_clip.unwrap_or(Rect::new(
            0,
            0,
            usize::from(raster.width),
            usize::from(raster.height),
        ));
        let left = node
            .position
            .column
            .saturating_add(clip.column as i32)
            .max(0);
        let top = node.position.line.saturating_add(clip.line as i32).max(0);
        let right = node
            .position
            .column
            .saturating_add(clip.right() as i32)
            .min(i32::from(self.viewport.width));
        let bottom = node
            .position
            .line
            .saturating_add(clip.bottom() as i32)
            .min(i32::from(self.viewport.height));
        if right <= left || bottom <= top {
            return Vec::new();
        }
        let mut spans = Vec::new();
        for line in top..bottom {
            let mut column = left;
            while column < right {
                while column < right
                    && !self.raster_cell_visible(
                        id,
                        node,
                        raster,
                        alpha_cells,
                        owners,
                        line,
                        column,
                    )
                {
                    column += 1;
                }
                let start = column;
                while column < right
                    && self.raster_cell_visible(id, node, raster, alpha_cells, owners, line, column)
                {
                    column += 1;
                }
                if column > start {
                    spans.push(ScreenRect::new(line, start, column - start, 1));
                }
            }
        }
        merge_rectangles(spans)
    }

    #[allow(clippy::too_many_arguments)]
    fn raster_cell_visible(
        &self,
        id: ImageId,
        node: &ImageNode,
        raster: &RasterPlacement,
        alpha_cells: &[bool],
        owners: &[Option<LayerKey>],
        line: i32,
        column: i32,
    ) -> bool {
        let local_line = line.saturating_sub(node.position.line) as usize;
        let local_column = column.saturating_sub(node.position.column) as usize;
        let alpha = alpha_cells
            .get(local_line * usize::from(raster.width) + local_column)
            .copied()
            .unwrap_or(false);
        let index = line as usize * usize::from(self.viewport.width) + column as usize;
        let rank = LayerKey(node.level, node.order, node.mutation, id.0);
        alpha && owners[index].is_none_or(|owner| owner <= rank)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NativeKey {
    transform: TransformKey,
    column: u16,
    line: u16,
    width: u16,
    height: u16,
    protocol: ImageProtocol,
}

impl NativeKey {
    fn from_tile(tile: TileKey, protocol: ImageProtocol) -> Self {
        Self {
            transform: tile.transform,
            column: tile.source_column,
            line: tile.source_line,
            width: tile.width,
            height: tile.height,
            protocol,
        }
    }
}

fn merge_rectangles(spans: Vec<ScreenRect>) -> Vec<ScreenRect> {
    // A row can contain many independent spans. Keep only rectangles that
    // touched the preceding row active, keyed by their horizontal extent.
    // This yields maximal, stable rectangles in linear time after span scan.
    let mut rectangles: Vec<ScreenRect> = Vec::new();
    let mut active: HashMap<(i32, i32), usize> = HashMap::new();
    let mut next: HashMap<(i32, i32), usize> = HashMap::new();
    let mut line = None;
    for span in spans {
        if line != Some(span.line) {
            active = std::mem::take(&mut next);
            next.clear();
            line = Some(span.line);
        }
        let key = (span.column, span.width);
        let index = match active.get(&key).copied() {
            Some(index)
                if rectangles[index]
                    .line
                    .saturating_add(rectangles[index].height)
                    == span.line =>
            {
                rectangles[index].height += span.height;
                index
            }
            _ => {
                rectangles.push(span);
                rectangles.len() - 1
            }
        };
        next.insert(key, index);
    }
    rectangles
}

fn surface_size(surface: &Surface) -> (usize, usize) {
    match surface {
        Surface::Cells(image) => (image.width(), image.height()),
        Surface::Raster(raster) => (usize::from(raster.width), usize::from(raster.height)),
    }
}

// Pure-Rust terminal capability detection. Without the native backend the
// framework still selects a correct protocol from the environment, so a build
// with no Chafa, pkg-config, or libclang remains fully functional for cell
// output and reports raster capability honestly.
#[cfg(not(feature = "native-raster"))]
fn detect_terminal(requested: ImageProtocol) -> (ImageProtocol, Passthrough) {
    let term = std::env::var("TERM").unwrap_or_default();
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let passthrough = if std::env::var_os("TMUX").is_some() {
        Passthrough::Tmux
    } else if term.starts_with("screen") {
        Passthrough::Screen
    } else {
        Passthrough::None
    };
    let protocol = match requested {
        ImageProtocol::Auto => {
            let kitty = std::env::var_os("KITTY_WINDOW_ID").is_some()
                || term.contains("kitty")
                || program.eq_ignore_ascii_case("kitty")
                || std::env::var("TERM_PROGRAM").is_ok_and(|value| value == "WezTerm");
            let sixel = term.contains("sixel") || term.contains("mlterm") || term.contains("yaft");
            let iterm = program == "iTerm.app";
            if kitty {
                ImageProtocol::Kitty
            } else if iterm {
                ImageProtocol::Iterm2
            } else if sixel {
                ImageProtocol::Sixel
            } else {
                ImageProtocol::Symbols
            }
        }
        value => value,
    };
    (protocol, passthrough)
}

#[cfg(feature = "native-raster")]
fn detect_terminal(requested: ImageProtocol) -> (ImageProtocol, Passthrough) {
    unsafe {
        let database = chafa_sys::chafa_term_db_get_default();
        let mut environment: Vec<CString> = std::env::vars()
            .filter_map(|(name, value)| CString::new(format!("{name}={value}")).ok())
            .collect();
        let mut pointers: Vec<_> = environment
            .iter_mut()
            .map(|value| value.as_ptr() as *mut _)
            .collect();
        pointers.push(std::ptr::null_mut());
        let info = if database.is_null() {
            std::ptr::null_mut()
        } else {
            chafa_sys::chafa_term_db_detect(database, pointers.as_mut_ptr())
        };
        let protocol = match requested {
            ImageProtocol::Auto if !info.is_null() => {
                match chafa_sys::chafa_term_info_get_best_pixel_mode(info) {
                    chafa_sys::ChafaPixelMode_CHAFA_PIXEL_MODE_KITTY => ImageProtocol::Kitty,
                    chafa_sys::ChafaPixelMode_CHAFA_PIXEL_MODE_SIXELS => ImageProtocol::Sixel,
                    chafa_sys::ChafaPixelMode_CHAFA_PIXEL_MODE_ITERM2 => ImageProtocol::Iterm2,
                    _ => ImageProtocol::Symbols,
                }
            }
            ImageProtocol::Auto => ImageProtocol::Symbols,
            value => value,
        };
        let passthrough = if info.is_null() {
            Passthrough::None
        } else {
            Passthrough::from_chafa(chafa_sys::chafa_term_info_get_passthrough_type(info))
        };
        // The term-info reference is released here; only the plain passthrough
        // value survives, so nothing native crosses a thread boundary.
        drop(TermInfo(info));
        (protocol, passthrough)
    }
}

fn live_cell_pixels() -> Option<Size> {
    let window = crossterm::terminal::window_size().ok()?;
    (window.columns > 0 && window.rows > 0 && window.width > 0 && window.height > 0).then(|| {
        Size::new(
            (window.width / window.columns).max(1),
            (window.height / window.rows).max(1),
        )
    })
}

// Without the native backend there is no encoder for Kitty/Sixel/iTerm2
// payloads, so those protocols report unsupported instead of emitting a
// malformed escape sequence. Symbols output is pure Rust and unaffected.
#[cfg(not(feature = "native-raster"))]
#[allow(clippy::too_many_arguments)]
fn encode_native_slice(
    _pixels: &RasterPixels,
    _column: u16,
    _line: u16,
    _width: u16,
    _height: u16,
    _cell_pixels: Size,
    _protocol: ImageProtocol,
    _passthrough: Passthrough,
) -> Option<String> {
    None
}

#[cfg(feature = "native-raster")]
#[allow(clippy::too_many_arguments)]
fn encode_native_slice(
    pixels: &RasterPixels,
    column: u16,
    line: u16,
    width: u16,
    height: u16,
    cell_pixels: Size,
    protocol: ImageProtocol,
    passthrough: Passthrough,
) -> Option<String> {
    let mode = match protocol {
        ImageProtocol::Kitty => chafa_sys::ChafaPixelMode_CHAFA_PIXEL_MODE_KITTY,
        ImageProtocol::Sixel => chafa_sys::ChafaPixelMode_CHAFA_PIXEL_MODE_SIXELS,
        ImageProtocol::Iterm2 => chafa_sys::ChafaPixelMode_CHAFA_PIXEL_MODE_ITERM2,
        ImageProtocol::Auto | ImageProtocol::Symbols => return None,
    };
    unsafe {
        let config = chafa_sys::chafa_canvas_config_new();
        if config.is_null() {
            return None;
        }
        chafa_sys::chafa_canvas_config_set_geometry(config, i32::from(width), i32::from(height));
        chafa_sys::chafa_canvas_config_set_cell_geometry(
            config,
            i32::from(cell_pixels.width.max(1)),
            i32::from(cell_pixels.height.max(1)),
        );
        chafa_sys::chafa_canvas_config_set_pixel_mode(config, mode);
        chafa_sys::chafa_canvas_config_set_passthrough(config, passthrough.to_chafa());
        let canvas = chafa_sys::chafa_canvas_new(config);
        chafa_sys::chafa_canvas_config_unref(config);
        if canvas.is_null() {
            return None;
        }
        let pixel_width = i32::from(width) * i32::from(cell_pixels.width.max(1));
        let pixel_height = i32::from(height) * i32::from(cell_pixels.height.max(1));
        let pixel_column = usize::from(column) * usize::from(cell_pixels.width.max(1));
        let pixel_line = usize::from(line) * usize::from(cell_pixels.height.max(1));
        let row_stride = pixels.width as usize * 4;
        let offset = pixel_line
            .checked_mul(row_stride)?
            .checked_add(pixel_column.checked_mul(4)?)?;
        if pixel_width <= 0
            || pixel_height <= 0
            || offset >= pixels.pixels.len()
            || pixel_column.saturating_add(pixel_width as usize) > pixels.width as usize
        {
            chafa_sys::chafa_canvas_unref(canvas);
            return None;
        }
        chafa_sys::chafa_canvas_draw_all_pixels(
            canvas,
            chafa_sys::ChafaPixelType_CHAFA_PIXEL_RGBA8_UNASSOCIATED,
            pixels.pixels[offset..].as_ptr(),
            pixel_width,
            pixel_height,
            row_stride.min(i32::MAX as usize) as i32,
        );
        // `build_ansi` does not depend on a terminfo entry that happens to
        // advertise the requested protocol. This matters for explicit
        // overrides in SSH/multiplexer sessions; the config still carries
        // Chafa passthrough when detection provided it.
        let value = chafa_sys::chafa_canvas_build_ansi(canvas);
        chafa_sys::chafa_canvas_unref(canvas);
        if value.is_null() {
            return None;
        }
        // chafa-sys currently makes GLib's public GString layout opaque, so
        // mirror that stable C ABI locally instead of assuming NUL-terminated
        // payloads (native graphics output is a byte stream).
        let string = &*(value as *const GStringLayout);
        let bytes = std::slice::from_raw_parts(string.data as *const u8, string.len);
        let output = String::from_utf8_lossy(bytes).into_owned();
        chafa_sys::g_string_free(value, 1);
        Some(output)
    }
}

#[repr(C)]
#[cfg(feature = "native-raster")]
struct GStringLayout {
    data: *mut std::ffi::c_char,
    len: usize,
    _allocated_len: usize,
}

fn validate_viewport(viewport: Size) -> Result<(), FrameError> {
    let cells = usize::from(viewport.width)
        .checked_mul(usize::from(viewport.height))
        .ok_or(FrameError::SurfaceTooLarge {
            width: viewport.width,
            height: viewport.height,
            cells: usize::MAX,
        })?;
    if cells > MAX_SURFACE_CELLS {
        return Err(FrameError::SurfaceTooLarge {
            width: viewport.width,
            height: viewport.height,
            cells,
        });
    }
    Ok(())
}

fn blank_cells(viewport: Size) -> Result<Vec<CellSlot>, FrameError> {
    validate_viewport(viewport)?;
    let count = usize::from(viewport.width) * usize::from(viewport.height);
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(count)
        .map_err(|_| FrameError::AllocationFailed { cells: count })?;
    cells.resize(count, CellSlot::Lead(Cell::blank()));
    Ok(cells)
}

impl PipelineComponent for Renderer {
    type Input = Frame;
    type Output = Result<String, FrameError>;

    const STAGE: crate::runtime::pipeline::Stage = crate::runtime::pipeline::Stage::Renderer;

    fn run(
        mut self,
        input: Receiver<Self::Input>,
        output: Sender<Self::Output>,
        _errors: Sender<crate::runtime::pipeline::RuntimeError>,
    ) -> Result<(), crate::runtime::pipeline::RuntimeError> {
        enum Wake {
            Frame(Frame),
            Load((ImageSource, Result<RasterImage, crate::RasterImageError>)),
            Timer,
            Closed,
        }

        loop {
            let wake = match self.native_replay_wait() {
                Some(wait) => crossbeam_channel::select! {
                    recv(input) -> message => message.map(Wake::Frame).unwrap_or(Wake::Closed),
                    recv(self.image_manager.results()) -> message => message.map(Wake::Load).unwrap_or(Wake::Closed),
                    default(wait) => Wake::Timer,
                },
                None => crossbeam_channel::select! {
                    recv(input) -> message => message.map(Wake::Frame).unwrap_or(Wake::Closed),
                    recv(self.image_manager.results()) -> message => message.map(Wake::Load).unwrap_or(Wake::Closed),
                },
            };
            let first = match wake {
                Wake::Closed => break,
                Wake::Timer => {
                    if !self.emit_render(&output) {
                        return Ok(());
                    }
                    continue;
                }
                Wake::Load(result) => {
                    self.image_manager.queue_result(result);
                    if !self.emit_render(&output) {
                        return Ok(());
                    }
                    continue;
                }
                Wake::Frame(frame) => frame,
            };
            let mut frames = vec![first];
            for _ in 1..64 {
                match input.try_recv() {
                    Ok(frame) => frames.push(frame),
                    Err(_) => break,
                }
            }

            for frame in frames {
                if let Err(error) = self.apply_frame(frame)
                    && output.send(Err(error)).is_err()
                {
                    return Ok(());
                }
            }
            if !self.emit_render(&output) {
                return Ok(());
            }
        }

        let _ = self.emit_render(&output);
        if let Some(cleanup) = self.shutdown_native() {
            let _ = output.send(Ok(cleanup));
        }
        Ok(())
    }
}

impl Renderer {
    fn emit_render(&mut self, output: &Sender<Result<String, FrameError>>) -> bool {
        match self.render_diff() {
            Ok(Some(diff)) => output.send(Ok(diff)).is_ok(),
            Ok(None) => true,
            Err(error) => output.send(Err(error)).is_ok(),
        }
    }
}

// A renderer whose output already carries the terminal payload. This is the
// shape the pipeline uses internally; it is exposed so a component that
// replaces the commit stage can still drive a real renderer.
pub struct ChannelRenderer(pub(crate) Renderer);

impl ChannelRenderer {
    pub fn new(viewport: Size) -> Result<Self, FrameError> {
        Ok(Self(Renderer::new(viewport)?))
    }

    pub fn with_config(viewport: Size, config: RendererConfig) -> Result<Self, FrameError> {
        Ok(Self(Renderer::with_config(viewport, config)?))
    }
}

impl PipelineComponent for ChannelRenderer {
    type Input = Frame;
    type Output = Result<String, FrameError>;

    const STAGE: crate::runtime::pipeline::Stage = crate::runtime::pipeline::Stage::Renderer;

    fn run(
        self,
        input: Receiver<Self::Input>,
        output: Sender<Self::Output>,
        errors: Sender<crate::runtime::pipeline::RuntimeError>,
    ) -> Result<(), crate::runtime::pipeline::RuntimeError> {
        self.0.run(input, output, errors)
    }
}

mod ansi;
mod compose;
use ansi::encode_diff;

#[cfg(test)]
mod tests;
