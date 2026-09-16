//! Builder that tracks surface handles while assembling a [`Frame`].

use std::collections::HashMap;

use crate::{
    CellEdit, Frame, Image, ImageId, Operation, RasterPlacement, Rect, ScreenPosition, Size,
};

/// A typed handle to a cell surface created by this builder.
///
/// Cell and raster handles are distinct types, so a raster operation cannot be
/// written against a cell surface, or the reverse: the mismatch is a compile
/// error rather than a runtime validation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellSurfaceHandle(ImageId);

/// A typed handle to a raster surface created by this builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RasterSurfaceHandle(ImageId);

impl CellSurfaceHandle {
    /// Returns the underlying surface identifier.
    pub const fn id(self) -> ImageId {
        self.0
    }
}

impl RasterSurfaceHandle {
    /// Returns the underlying surface identifier.
    pub const fn id(self) -> ImageId {
        self.0
    }
}

/// A handle of either kind, for operations that are surface-agnostic such as
/// position, level, order, and removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceHandle {
    /// A cell surface handle.
    Cells(CellSurfaceHandle),
    /// A raster surface handle.
    Raster(RasterSurfaceHandle),
}

impl SurfaceHandle {
    /// Returns the underlying surface identifier.
    pub const fn id(self) -> ImageId {
        match self {
            Self::Cells(handle) => handle.0,
            Self::Raster(handle) => handle.0,
        }
    }
}

impl From<CellSurfaceHandle> for SurfaceHandle {
    fn from(handle: CellSurfaceHandle) -> Self {
        Self::Cells(handle)
    }
}

impl From<RasterSurfaceHandle> for SurfaceHandle {
    fn from(handle: RasterSurfaceHandle) -> Self {
        Self::Raster(handle)
    }
}

/// Why a builder operation was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    /// The handle was not created by this builder.
    UnknownHandle,
    /// The surface was already removed.
    Removed,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownHandle => write!(f, "handle was not created by this frame builder"),
            Self::Removed => write!(f, "surface was already removed"),
        }
    }
}

impl std::error::Error for BuildError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Kind {
    #[default]
    Cells,
    Raster,
}

#[derive(Debug, Default)]
struct Tracked {
    kind: Kind,
    removed: bool,
}

/// Builds a frame while tracking creation, kind, and removal within the batch.
///
/// The result is still a raw [`Frame`], so the renderer's transactional
/// validation remains the single authority; the builder only makes common
/// mistakes unrepresentable before that point.
#[derive(Debug, Default)]
pub struct FrameBuilder {
    operations: Vec<Operation>,
    tracked: HashMap<ImageId, Tracked>,
    next_id: u64,
    viewport: Option<Size>,
    force_redraw: bool,
}

impl FrameBuilder {
    /// Creates an empty builder whose first surface identifier is 1.
    pub fn new() -> Self {
        Self {
            next_id: 1,
            ..Self::default()
        }
    }

    /// Sets the terminal size the finished frame resizes to.
    pub fn with_viewport(mut self, viewport: Size) -> Self {
        self.viewport = Some(viewport);
        self
    }

    /// Requests a full repaint in the finished frame.
    pub fn invalidate(mut self) -> Self {
        self.force_redraw = true;
        self
    }

    fn allocate(&mut self, kind: Kind) -> ImageId {
        let id = ImageId(self.next_id);
        self.next_id = self.next_id.saturating_add(1).max(1);
        self.tracked.insert(
            id,
            Tracked {
                kind,
                removed: false,
            },
        );
        id
    }

    /// Rejects unknown, removed, or wrong-kind handles; the wrong-kind branch is
    /// unreachable through the typed API and kept as a defence in depth.
    fn check(&self, id: ImageId, expected: Kind) -> Result<(), BuildError> {
        let Some(tracked) = self.tracked.get(&id) else {
            return Err(BuildError::UnknownHandle);
        };
        if tracked.removed {
            return Err(BuildError::Removed);
        }
        if tracked.kind != expected {
            return Err(BuildError::UnknownHandle);
        }
        Ok(())
    }

    /// Adds a cell surface and returns its handle.
    pub fn create_cells(
        &mut self,
        image: Image,
        position: ScreenPosition,
        level: i32,
    ) -> CellSurfaceHandle {
        let id = self.allocate(Kind::Cells);
        self.operations.push(Operation::Create {
            id,
            image,
            position,
            level,
        });
        CellSurfaceHandle(id)
    }

    /// Adds a raster surface and returns its handle.
    pub fn create_raster(
        &mut self,
        raster: RasterPlacement,
        position: ScreenPosition,
        level: i32,
    ) -> RasterSurfaceHandle {
        let id = self.allocate(Kind::Raster);
        self.operations.push(Operation::CreateRaster {
            id,
            raster,
            position,
            level,
        });
        RasterSurfaceHandle(id)
    }

    /// Adds cell edits to a cell surface.
    pub fn patch_cells(
        &mut self,
        handle: CellSurfaceHandle,
        edits: Vec<CellEdit>,
    ) -> Result<(), BuildError> {
        self.check(handle.0, Kind::Cells)?;
        self.operations.push(Operation::PatchCells {
            id: handle.0,
            edits,
        });
        Ok(())
    }

    /// Replaces a rectangular region of a cell surface.
    pub fn patch_rect(
        &mut self,
        handle: CellSurfaceHandle,
        rect: Rect,
        rows: Vec<Vec<crate::Cell>>,
    ) -> Result<(), BuildError> {
        self.check(handle.0, Kind::Cells)?;
        self.operations.push(Operation::PatchRect {
            id: handle.0,
            rect,
            rows,
        });
        Ok(())
    }

    /// Replaces a cell surface's image.
    pub fn replace_cells(
        &mut self,
        handle: CellSurfaceHandle,
        image: Image,
    ) -> Result<(), BuildError> {
        self.check(handle.0, Kind::Cells)?;
        self.operations.push(Operation::Replace {
            id: handle.0,
            image,
        });
        Ok(())
    }

    /// Replaces a raster surface's placement.
    pub fn replace_raster(
        &mut self,
        handle: RasterSurfaceHandle,
        raster: RasterPlacement,
    ) -> Result<(), BuildError> {
        self.check(handle.0, Kind::Raster)?;
        self.operations.push(Operation::ReplaceRaster {
            id: handle.0,
            raster,
        });
        Ok(())
    }

    /// Sets or clears a raster surface's clip rectangle.
    pub fn set_raster_clip(
        &mut self,
        handle: RasterSurfaceHandle,
        clip: Option<Rect>,
    ) -> Result<(), BuildError> {
        self.check(handle.0, Kind::Raster)?;
        self.operations
            .push(Operation::SetRasterClip { id: handle.0, clip });
        Ok(())
    }

    /// Moves a surface to a new position.
    pub fn place(
        &mut self,
        handle: impl Into<SurfaceHandle>,
        position: ScreenPosition,
    ) -> Result<(), BuildError> {
        let id = handle.into().id();
        self.check_any(id)?;
        self.operations.push(Operation::Move { id, position });
        Ok(())
    }

    /// Sets a surface's stacking level.
    pub fn set_level(
        &mut self,
        handle: impl Into<SurfaceHandle>,
        level: i32,
    ) -> Result<(), BuildError> {
        let id = handle.into().id();
        self.check_any(id)?;
        self.operations.push(Operation::SetLevel { id, level });
        Ok(())
    }

    /// Sets a surface's order within its level.
    pub fn set_order(
        &mut self,
        handle: impl Into<SurfaceHandle>,
        order: u64,
    ) -> Result<(), BuildError> {
        let id = handle.into().id();
        self.check_any(id)?;
        self.operations.push(Operation::SetOrder { id, order });
        Ok(())
    }

    /// Removes a surface; a second removal in the same batch is rejected as
    /// [`BuildError::Removed`] instead of emitting a use-after-remove operation.
    pub fn remove(&mut self, handle: impl Into<SurfaceHandle>) -> Result<(), BuildError> {
        let id = handle.into().id();
        match self.tracked.get_mut(&id) {
            None => return Err(BuildError::UnknownHandle),
            Some(tracked) if tracked.removed => return Err(BuildError::Removed),
            Some(tracked) => tracked.removed = true,
        }
        self.operations.push(Operation::Remove { id });
        Ok(())
    }

    fn check_any(&self, id: ImageId) -> Result<(), BuildError> {
        match self.tracked.get(&id) {
            None => Err(BuildError::UnknownHandle),
            Some(tracked) if tracked.removed => Err(BuildError::Removed),
            Some(_) => Ok(()),
        }
    }

    /// Consumes the builder and returns the assembled frame.
    pub fn finish(self) -> Frame {
        let mut frame = Frame::new(self.operations);
        if let Some(viewport) = self.viewport {
            frame = frame.resize(viewport);
        }
        if self.force_redraw {
            frame = frame.invalidate();
        }
        frame
    }
}
