//! Image scheduling: request queuing, decode and transform budgets, and the
//! per-frame lifecycle of a placement.

mod kitty;
mod lifecycle;
pub(crate) mod manager;
pub(in crate::runtime) mod passthrough;
mod types;

use crossterm::cursor::{RestorePosition, SavePosition};

pub(in crate::runtime) use kitty::KittyBackend;
pub(in crate::runtime) use manager::ImageManager;
pub(in crate::runtime) use passthrough::Passthrough;
pub(in crate::runtime) use types::{NativeTile, PreparedRaster, ScreenRect, TileKey, TransformKey};

pub(in crate::runtime) fn surround_native(output: String) -> String {
    if output.is_empty() {
        String::new()
    } else {
        format!("{}{}{}", SavePosition, output, RestorePosition)
    }
}
