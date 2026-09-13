use std::collections::HashMap;

use crate::{
    CellEdit, Frame, Image, ImageId, Operation, RasterPlacement, Rect, ScreenPosition, Size,
};

// Typed handles produced by the builder. Cell and raster handles are distinct
// types, so a raster operation cannot be written against a cell surface (or the
// reverse): the mismatch is a compile error, not a runtime validation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellSurfaceHandle(ImageId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RasterSurfaceHandle(ImageId);

impl CellSurfaceHandle {
    pub const fn id(self) -> ImageId {
        self.0
    }
}

impl RasterSurfaceHandle {
    pub const fn id(self) -> ImageId {
        self.0
    }
}

// A handle of either kind, for operations that are surface-agnostic (position,
// level, order, removal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceHandle {
    Cells(CellSurfaceHandle),
    Raster(RasterSurfaceHandle),
}

impl SurfaceHandle {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    UnknownHandle,
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

// Builds a frame while tracking creation, kind, and removal within the batch.
// The result is still a raw `Frame`, so the renderer's transactional validation
// remains the single authority; the builder only makes common mistakes
// unrepresentable before that point.
#[derive(Debug, Default)]
pub struct FrameBuilder {
    operations: Vec<Operation>,
    tracked: HashMap<ImageId, Tracked>,
    next_id: u64,
    viewport: Option<Size>,
    force_redraw: bool,
}

impl FrameBuilder {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            ..Self::default()
        }
    }

    pub fn with_viewport(mut self, viewport: Size) -> Self {
        self.viewport = Some(viewport);
        self
    }

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

    fn check(&self, id: ImageId, expected: Kind) -> Result<(), BuildError> {
        let Some(tracked) = self.tracked.get(&id) else {
            return Err(BuildError::UnknownHandle);
        };
        if tracked.removed {
            return Err(BuildError::Removed);
        }
        if tracked.kind != expected {
            // Unreachable through the typed API; kept as a defence in depth.
            return Err(BuildError::UnknownHandle);
        }
        Ok(())
    }

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

    // Remove is idempotent-safe within the batch: a second removal is rejected
    // instead of emitting a use-after-remove operation.
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
