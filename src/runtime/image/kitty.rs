use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;

use crossterm::cursor::MoveTo;
use flate2::{Compression, write::ZlibEncoder};

use crate::Size;

use super::passthrough::Passthrough;
use super::surround_native;
use super::types::{NativeTile, PreparedRaster, TileKey, TransformKey};

#[derive(Debug, Default)]
pub(in crate::runtime) struct KittyBackend {
    images: HashMap<TransformKey, u32>,
    invalid_images: HashSet<TransformKey>,
    placements: HashMap<TileKey, u32>,
    releases: Vec<u32>,
    next_id: u32,
}

impl KittyBackend {
    pub(in crate::runtime) fn new() -> Self {
        Self {
            next_id: 1,
            ..Self::default()
        }
    }

    fn mark_images_invalid(&mut self) {
        self.invalid_images.extend(self.images.keys().copied());
    }

    pub(in crate::runtime) fn evict_transform(
        &mut self,
        key: TransformKey,
        native_tiles: &mut HashSet<TileKey>,
    ) {
        if let Some(image) = self.images.remove(&key) {
            self.invalid_images.remove(&key);
            self.placements.retain(|tile, _| tile.transform != key);
            native_tiles.retain(|tile| tile.transform != key);
            self.releases.push(image);
        }
    }

    pub(in crate::runtime) fn output(
        &mut self,
        tiles: Vec<NativeTile>,
        replace_all: bool,
        native_tiles: &mut HashSet<TileKey>,
        prepared: &HashMap<TransformKey, PreparedRaster>,
        cell_pixels: Size,
        passthrough: Passthrough,
    ) -> String {
        let desired: HashSet<_> = tiles.iter().map(|tile| tile.key).collect();
        let desired_transforms: HashSet<_> = tiles.iter().map(|tile| tile.key.transform).collect();
        let mut output = String::new();

        if replace_all {
            self.mark_images_invalid();
        }

        for image in std::mem::take(&mut self.releases) {
            output.push_str(&self.command(&format!("a=d,d=I,i={image},q=2"), passthrough));
        }

        let mut old = std::mem::take(&mut self.placements);
        let mut old_transform_counts: HashMap<TransformKey, usize> = HashMap::new();
        for key in old.keys() {
            *old_transform_counts.entry(key.transform).or_default() += 1;
        }

        for tile in tiles {
            let (matched_key, placement) = if let Some(placement) = old.remove(&tile.key) {
                (Some(tile.key), Some(placement))
            } else if let Some(key) = old.keys().copied().find(|key| key.same_source(tile.key)) {
                (Some(key), old.remove(&key))
            } else {
                (None, None)
            };
            if let Some(key) = matched_key
                && let Some(count) = old_transform_counts.get_mut(&key.transform)
            {
                *count = count.saturating_sub(1);
            }

            // A resize changes transformed pixel dimensions, but Kitty lets
            // us retransmit under the existing image id and replace its
            // placement with the existing placement id.
            let mut force_upload = self.invalid_images.remove(&tile.key.transform);
            let preferred_image = if let Some(key) = matched_key
                && key.transform != tile.key.transform
                && old_transform_counts
                    .get(&key.transform)
                    .copied()
                    .unwrap_or(0)
                    == 0
                && !desired_transforms.contains(&key.transform)
            {
                force_upload |= self.invalid_images.remove(&key.transform);
                self.images.remove(&key.transform)
            } else {
                None
            };
            let image = self.image(
                tile.key.transform,
                preferred_image,
                force_upload,
                prepared,
                passthrough,
                &mut output,
            );
            let placement = placement.unwrap_or_else(|| self.allocate_id());
            let unchanged =
                !replace_all && matched_key == Some(tile.key) && native_tiles.contains(&tile.key);
            self.placements.insert(tile.key, placement);
            if unchanged {
                continue;
            }
            let x = u32::from(tile.key.source_column) * u32::from(cell_pixels.width);
            let y = u32::from(tile.key.source_line) * u32::from(cell_pixels.height);
            let width = u32::from(tile.key.width) * u32::from(cell_pixels.width);
            let height = u32::from(tile.key.height) * u32::from(cell_pixels.height);
            let _ = write!(
                output,
                "{}{}",
                MoveTo(tile.rect.column as u16, tile.rect.line as u16),
                self.command(
                    &format!(
                        "a=p,i={image},p={placement},x={x},y={y},w={width},h={height},c={},r={},C=1,z={},q=2",
                        tile.key.width, tile.key.height, tile.key.level
                    ),
                    passthrough,
                )
            );
        }

        // A placement is independent terminal state. Remove only the
        // placement that disappeared; use Kitty's image/placement delete
        // operation rather than an image-only or unsupported form.
        for (key, placement) in old {
            if let Some(image) = self.images.get(&key.transform) {
                output.push_str(
                    &self.command(&format!("a=d,d=i,i={image},p={placement},q=2"), passthrough),
                );
            }
        }
        *native_tiles = desired;
        surround_native(output)
    }

    pub(in crate::runtime) fn shutdown(&mut self, passthrough: Passthrough) -> Option<String> {
        let mut ids: Vec<_> = self.images.values().copied().collect();
        ids.sort_unstable();
        ids.dedup();
        self.images.clear();
        self.invalid_images.clear();
        self.placements.clear();
        self.releases.clear();
        let mut output = String::new();
        for image in ids {
            output.push_str(&self.command(&format!("a=d,d=I,i={image},q=2"), passthrough));
        }
        (!output.is_empty()).then(|| surround_native(output))
    }

    fn allocate_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(1);
        id
    }

    fn image(
        &mut self,
        key: TransformKey,
        preferred_image: Option<u32>,
        force_upload: bool,
        prepared: &HashMap<TransformKey, PreparedRaster>,
        passthrough: Passthrough,
        output: &mut String,
    ) -> u32 {
        let existing = self.images.get(&key).copied();
        if let Some(image) = existing
            && !force_upload
        {
            return image;
        }
        let image = existing
            .or(preferred_image)
            .unwrap_or_else(|| self.allocate_id());
        let Some(prepared) = prepared.get(&key) else {
            return image;
        };
        let compressed = zlib(&prepared.pixels.pixels);
        let (payload, compression): (&[u8], &str) = match compressed.as_deref() {
            Some(payload) if payload.len() < prepared.pixels.pixels.len() => (payload, "o=z,"),
            _ => (&prepared.pixels.pixels, ""),
        };
        // Keep chunks small enough to stay below conservative terminal input
        // limits. Only the first chunk carries upload metadata.
        for (index, chunk) in payload.chunks(3 * 1024).enumerate() {
            let more = usize::from((index + 1) * 3 * 1024 < payload.len());
            if index == 0 {
                output.push_str(&self.command(
                    &format!(
                        "a=t,f=32,{compression}s={},v={},i={image},m={more},q=2;{}",
                        prepared.pixels.width,
                        prepared.pixels.pixels.len() / (prepared.pixels.width as usize * 4),
                        base64(chunk),
                    ),
                    passthrough,
                ));
            } else {
                output.push_str(&self.command(&format!("m={more};{}", base64(chunk)), passthrough));
            }
        }
        self.images.insert(key, image);
        image
    }

    fn command(&self, payload: &str, passthrough: Passthrough) -> String {
        native_escape(&format!("\x1b_G{payload}\x1b\\"), passthrough)
    }
}

// Kitty/Sixel payloads must be wrapped for a multiplexer, and every embedded
// escape has to be doubled inside the passthrough envelope.
fn native_escape(command: &str, passthrough: Passthrough) -> String {
    match passthrough.escape(command) {
        Some(envelope) => envelope
            .replace('\x1b', "\x1b\x1b")
            .replacen("\x1b\x1b", "\x1b", 1),
        None => command.to_owned(),
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(first >> 2) as usize] as char);
        output.push(TABLE[((first & 0x03) << 4 | second >> 4) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((second & 0x0f) << 2 | third >> 6) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(third & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn zlib(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes).ok()?;
    encoder.finish().ok()
}
