mod kitty;
mod lifecycle;
mod manager;
mod types;

use crossterm::cursor::{RestorePosition, SavePosition};

pub(in crate::runtime) use kitty::KittyBackend;
pub(in crate::runtime) use manager::ImageManager;
pub(in crate::runtime) use types::{NativeTile, PreparedRaster, ScreenRect, TileKey, TransformKey};

pub(in crate::runtime) fn surround_native(output: String) -> String {
    if output.is_empty() {
        String::new()
    } else {
        format!("{}{}{}", SavePosition, output, RestorePosition)
    }
}
