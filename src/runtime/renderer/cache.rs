//! Raster transform cache, symbol preparation, and byte accounting. The
//! renderer's terminal output does not depend on this module.
#![allow(unused_imports)]

use super::*;

impl Renderer {
    /// Returns the transform key for `raster`, reusing a cached entry or
    /// rendering and caching a fresh RGBA buffer with its per-cell alpha mask.
    /// A transform that cannot fit the configured budget is refused here,
    /// before its pixel buffer is reserved.
    pub(in crate::runtime) fn prepare_raster(
        &mut self,
        raster: &RasterPlacement,
    ) -> Option<TransformKey> {
        let key = self.transform_key(raster)?;
        self.cache_tick = self.cache_tick.saturating_add(1);
        if let Some(entry) = self.prepared.get_mut(&key) {
            entry.used = self.cache_tick;
            return Some(key);
        }
        let source = self.source_image(&raster.source)?;
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

    /// Rasterizes the cached transform of `key` into a symbol image `width` by
    /// `height` cells and charges 48 bytes per cell against the cache; a no-op
    /// when the transform is absent or already has symbols.
    pub(in crate::runtime) fn prepare_symbols(
        &mut self,
        key: TransformKey,
        width: u16,
        height: u16,
    ) {
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

    /// Re-probes the terminal cell size when detection is enabled, forcing a
    /// full redraw if it changed.
    pub(in crate::runtime) fn refresh_cell_pixels(&mut self) {
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

    /// Evicts entries until the cache total plus `incoming` fits
    /// `native_cache_limit`: encoded native payloads first, then unpinned
    /// prepared transforms, then inactive decoded sources. Transforms of the
    /// current scene are pinned, so a cache limit never thrashes the active
    /// frame.
    pub(in crate::runtime) fn evict_cache(&mut self, incoming: usize) {
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

    /// Total cache bytes: decoded sources plus prepared rasters and native
    /// payloads.
    pub(in crate::runtime) fn cache_bytes(&self) -> usize {
        self.image_manager
            .source_cache_bytes()
            .saturating_add(self.prepared_bytes)
            .saturating_add(self.native_cache_bytes)
    }

    /// Removes one native payload and subtracts its bytes; kept beside the cache
    /// it mutates and called by eviction paths.
    #[allow(dead_code)]
    fn remove_native_cache(&mut self, key: NativeKey) {
        if let Some(value) = self.native_cache.remove(&key) {
            self.native_cache_bytes = self.native_cache_bytes.saturating_sub(value.value.len());
        }
    }
}
