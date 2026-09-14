// Retained-scene geometry: which surface owns each viewport cell, and the
// visible raster tile decomposition a frame needs.
#![allow(unused_imports)]

use super::*;

impl Renderer {
    pub(super) fn cell_owners(&self) -> Vec<Option<LayerKey>> {
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

    pub(super) fn cell_surface<'a>(&'a self, node: &'a ImageNode) -> Option<&'a Image> {
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

    pub(super) fn raster_tiles(
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
    pub(super) fn raster_cell_visible(
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
