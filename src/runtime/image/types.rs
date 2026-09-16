//! Image identities, source keys, and the decoded payloads the scheduler
//! moves between stages.

use crate::raster::RasterPixels;
use crate::{Image, ImageId, ImageRenderOptions, Size};

#[derive(Debug)]
pub(in crate::runtime) struct PreparedRaster {
    pub(in crate::runtime) pixels: RasterPixels,
    pub(in crate::runtime) alpha_cells: Vec<bool>,
    pub(in crate::runtime) symbols: Option<Image>,
    pub(in crate::runtime) bytes: usize,
    pub(in crate::runtime) used: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::runtime) struct TransformKey {
    pub(in crate::runtime) image: u64,
    pub(in crate::runtime) width: u16,
    pub(in crate::runtime) height: u16,
    pub(in crate::runtime) options: ImageRenderOptions,
    pub(in crate::runtime) cell_pixels: Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::runtime) struct TileKey {
    pub(in crate::runtime) raster: ImageId,
    pub(in crate::runtime) transform: TransformKey,
    pub(in crate::runtime) source_column: u16,
    pub(in crate::runtime) source_line: u16,
    pub(in crate::runtime) destination_column: i32,
    pub(in crate::runtime) destination_line: i32,
    pub(in crate::runtime) width: u16,
    pub(in crate::runtime) height: u16,
    pub(in crate::runtime) level: i32,
    pub(in crate::runtime) order: u64,
}

impl TileKey {
    pub(in crate::runtime) fn same_source(self, other: Self) -> bool {
        self.raster == other.raster
            && self.source_column == other.source_column
            && self.source_line == other.source_line
            && self.level == other.level
            && self.order == other.order
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime) struct ScreenRect {
    pub(in crate::runtime) line: i32,
    pub(in crate::runtime) column: i32,
    pub(in crate::runtime) width: i32,
    pub(in crate::runtime) height: i32,
}

impl ScreenRect {
    pub(in crate::runtime) const fn new(line: i32, column: i32, width: i32, height: i32) -> Self {
        Self {
            line,
            column,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::runtime) struct NativeTile {
    pub(in crate::runtime) key: TileKey,
    pub(in crate::runtime) rect: ScreenRect,
}
