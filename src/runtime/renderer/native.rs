//! Native tile planning: which raster tiles a frame needs, their cached
//! payloads, replay timing, and Kitty shutdown cleanup.
#![allow(unused_imports)]

use super::*;

impl Renderer {
    pub(super) fn native_output(&mut self, tiles: Vec<NativeTile>, replay: bool) -> String {
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

    pub(super) fn native_replay_wait(&self) -> Option<Duration> {
        self.native_replay_at
            .map(|at| at.saturating_duration_since(Instant::now()))
    }

    pub(super) fn native_payload(&mut self, tile: &NativeTile) -> Option<String> {
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

    pub(super) fn kitty_output(&mut self, tiles: Vec<NativeTile>, replace_all: bool) -> String {
        self.kitty.output(
            tiles,
            replace_all,
            &mut self.native_tiles,
            &self.prepared,
            self.cell_pixels,
            self.passthrough,
        )
    }

    pub(super) fn shutdown_native(&mut self) -> Option<String> {
        if self.protocol != ImageProtocol::Kitty {
            return None;
        }
        self.native_tiles.clear();
        self.kitty.shutdown(self.passthrough)
    }

    pub(super) fn collect_native_tiles(&mut self) -> Vec<NativeTile> {
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
}
