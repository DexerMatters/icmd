// Damage normalization, layer ordering, and the composition pass that
// turns retained surfaces into the desired cell frame.
#![allow(unused_imports)]

use super::*;

impl Renderer {
    pub(super) fn rebuild_layers(&mut self) {
        if !self.layers_dirty && self.layers.len() == self.images.len() {
            return;
        }
        self.layers.clear();
        self.layers.extend(self.images.keys().copied());
        self.layers.sort_by_key(|id| {
            let node = &self.images[id];
            (node.level, node.order, node.mutation, id.0)
        });
        self.layers_dirty = false;
    }

    pub(super) fn normalize_damage(&mut self, full: bool) {
        let width = usize::from(self.viewport.width);
        let height = usize::from(self.viewport.height);
        self.clear_dirty_marks();
        if self.damage_rows.len() != height {
            self.damage_rows.clear();
            self.damage_rows.resize_with(height, Vec::new);
        }
        for row in &mut self.damage_rows {
            row.clear();
        }
        if full {
            for line in 0..height {
                self.damage_rows[line].push(0..width);
            }
        } else {
            for damage in &self.damage {
                let left = damage.column.saturating_sub(2).max(0) as usize;
                let top = damage.line.saturating_sub(2).max(0) as usize;
                let right = (damage.column + damage.width + 2).clamp(0, width as i64) as usize;
                let bottom = (damage.line + damage.height + 2).clamp(0, height as i64) as usize;
                if left >= right {
                    continue;
                }
                for row in &mut self.damage_rows[top..bottom] {
                    row.push(left..right);
                }
            }
            for row in &mut self.damage_rows {
                if row.len() < 2 {
                    continue;
                }
                row.sort_unstable_by_key(|span| span.start);
                let mut merged: Vec<Range<usize>> = Vec::with_capacity(row.len());
                for span in std::mem::take(row) {
                    if let Some(previous) = merged.last_mut()
                        && span.start <= previous.end
                    {
                        previous.end = previous.end.max(span.end);
                    } else {
                        merged.push(span);
                    }
                }
                *row = merged;
            }
        }
        let mut marks = std::mem::take(&mut self.dirty_marks);
        marks.clear();
        for (line, spans) in self.damage_rows.iter().enumerate() {
            for span in spans {
                self.dirty[line * width + span.start..line * width + span.end].fill(true);
                marks.push((line, span.clone()));
            }
        }
        self.dirty_marks = marks;
    }

    // Clear exactly the cells marked dirty for the previous attempt.
    pub(super) fn clear_dirty_marks(&mut self) {
        if self.dirty_marks.is_empty() {
            return;
        }
        let width = usize::from(self.viewport.width);
        let marks = std::mem::take(&mut self.dirty_marks);
        for (line, span) in &marks {
            let start = line * width + span.start;
            let end = (line * width + span.end).min(self.dirty.len());
            if start < end {
                self.dirty[start..end].fill(false);
            }
        }
        self.dirty_marks = marks;
        self.dirty_marks.clear();
    }

    // Copy the presented frame into the scratch buffer for every row this frame
    // will read. Rows outside the damage are never inspected, so leaving them
    // stale is free.
    pub(super) fn refresh_desired_rows(&self, desired: &mut [CellSlot]) {
        let width = usize::from(self.viewport.width);
        for (line, spans) in self.damage_rows.iter().enumerate() {
            if spans.is_empty() {
                continue;
            }
            let start = line * width;
            let end = (start + width).min(self.last.len());
            if start < end && end <= desired.len() {
                desired[start..end].clone_from_slice(&self.last[start..end]);
            }
        }
    }

    // Apply only the rows that changed to the presented frame.
    pub(super) fn apply_desired_rows(&mut self, desired: &[CellSlot]) {
        let width = usize::from(self.viewport.width);
        for (line, spans) in self.damage_rows.iter().enumerate() {
            if spans.is_empty() {
                continue;
            }
            let start = line * width;
            let end = (start + width).min(self.last.len());
            if start < end && end <= desired.len() {
                self.last[start..end].clone_from_slice(&desired[start..end]);
            }
        }
    }

    pub(super) fn compose_damage(&mut self, desired: &mut [CellSlot]) {
        self.rebuild_layers();
        let width = usize::from(self.viewport.width);
        let height = usize::from(self.viewport.height);
        for (line, spans) in self.damage_rows.iter().enumerate() {
            for span in spans {
                let range = line * width + span.start..line * width + span.end;
                desired[range.clone()].fill(CellSlot::Lead(Cell::blank()));
                self.owners[range].fill(0);
            }
        }

        // Split immutable scene/cache access from mutable scratch access. The
        // composition pass is deliberately bottom-to-top, so an assignment is
        // the exact deterministic equivalent of selecting the highest rank at
        // every cell.
        // The layer order is moved out for the duration of the pass instead of
        // cloned: iteration only needs to read it, and it is restored before
        // returning. This removes a per-frame allocation of the whole layer
        // list for scenes with many images.
        let layers = std::mem::take(&mut self.layers);
        let symbols_for_native = self.symbols_for_native;
        let protocol = self.protocol;
        let cell_pixels = self.cell_pixels;
        let images = &self.images;
        let prepared = &self.prepared;
        let manager = &self.image_manager;
        let rows = &self.damage_rows;
        let owners = &mut self.owners;

        for (ordinal, id) in layers.iter().copied().enumerate() {
            let Some(node) = images.get(&id) else {
                continue;
            };
            let (image, alpha_cells, is_raster) = match &node.surface {
                Surface::Cells(image) => (image, None, false),
                Surface::Raster(raster) => {
                    let transform =
                        manager
                            .source_image(&raster.source)
                            .map(|source| TransformKey {
                                image: source.id(),
                                width: raster.full_width,
                                height: raster.full_height,
                                options: raster.options,
                                cell_pixels,
                            });
                    match transform {
                        Some(key)
                            if symbols_for_native
                                || protocol == ImageProtocol::Symbols
                                || raster.options.mode == ImageMode::Symbols =>
                        {
                            let Some(entry) = prepared.get(&key) else {
                                continue;
                            };
                            let Some(image) = entry.symbols.as_ref() else {
                                continue;
                            };
                            (image, Some(entry.alpha_cells.as_slice()), true)
                        }
                        Some(_) => continue,
                        None => {
                            let Some(image) = node.fallback.as_ref() else {
                                continue;
                            };
                            (image, None, true)
                        }
                    }
                }
            };
            let top = node.position.line.max(0) as usize;
            let bottom = node
                .position
                .line
                .saturating_add(image.height() as i32)
                .clamp(0, height as i32) as usize;
            if top >= bottom {
                continue;
            }
            for (line, row) in rows.iter().enumerate().take(bottom).skip(top) {
                let local_line = (line as i32 - node.position.line) as usize;
                for span in row {
                    let left = span.start.max(node.position.column.max(0) as usize);
                    let right = span.end.min(
                        node.position
                            .column
                            .saturating_add(image.width() as i32)
                            .max(0) as usize,
                    );
                    if left >= right {
                        continue;
                    }
                    for column in left..right {
                        let local_column = (column as i32 - node.position.column) as usize;
                        if is_raster && !Self::raster_clip_contains(node, local_line, local_column)
                        {
                            continue;
                        }
                        if alpha_cells.is_some_and(|alpha| {
                            !alpha
                                .get(local_line * image.width() + local_column)
                                .copied()
                                .unwrap_or(false)
                        }) {
                            continue;
                        }
                        let index = line * width + column;
                        desired[index] = image.cell_at(local_line, local_column).clone();
                        owners[index] = ordinal.saturating_add(1);
                    }
                }
            }
        }

        for (line, spans) in rows.iter().enumerate() {
            for span in spans {
                for column in span.clone() {
                    let index = line * width + column;
                    match &desired[index] {
                        CellSlot::Lead(cell) if cell.width() == 2 => {
                            let valid = column + 1 < width
                                && owners[index] != 0
                                && owners[index] == owners[index + 1]
                                && matches!(desired[index + 1], CellSlot::Continuation(_));
                            if !valid {
                                desired[index] = CellSlot::Lead(cell.as_blank());
                            }
                        }
                        CellSlot::Continuation(cell) => {
                            let valid = column > 0
                                && owners[index] != 0
                                && owners[index] == owners[index - 1]
                                && matches!(desired[index - 1], CellSlot::Lead(ref previous) if previous.width() == 2);
                            if !valid {
                                desired[index] = CellSlot::Lead(cell.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Restore the layer order that was moved out for the pass.
        self.layers = layers;
    }
}
