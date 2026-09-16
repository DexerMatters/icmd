//! Image load lifecycle for the renderer: requests visible sources, drains results, and prepares rasters.
//! Owns source-request fallbacks, raster visibility checks, and native/symbol preparation.

use std::collections::HashSet;

use crate::runtime::renderer::{ImageNode, Renderer, Surface};
use crate::{ImageMode, ImageProtocol, ImageSource, RasterImage, RasterPlacement, Rect};

use super::manager::{SourceRequest, placeholder as placeholder_image};

/// Which placeholder a source change installs.
#[derive(Debug, Clone, Copy)]
enum Placeholder {
    /// The source is on its way, so the box shows `…`.
    Loading,
    /// The source cannot be shown, so the box shows the placement's alternative
    /// text, or `×` when it has none.
    Unavailable,
}

impl Placeholder {
    /// The text this placeholder paints in `raster`'s box.
    fn text(self, raster: &RasterPlacement) -> String {
        match self {
            Self::Loading => String::from("…"),
            Self::Unavailable => raster.unavailable_text().to_string(),
        }
    }
}
use super::types::TransformKey;

impl Renderer {
    pub(in crate::runtime) fn source_image(&self, source: &ImageSource) -> Option<RasterImage> {
        self.image_manager.source_image(source)
    }

    pub(in crate::runtime) fn transform_key(
        &self,
        raster: &RasterPlacement,
    ) -> Option<TransformKey> {
        let image = self.source_image(&raster.source)?;
        Some(TransformKey {
            image: image.id(),
            width: raster.full_width,
            height: raster.full_height,
            options: raster.options,
            cell_pixels: self.cell_pixels,
        })
    }

    pub(in crate::runtime) fn source_should_load(
        &self,
        node: &ImageNode,
        raster: &RasterPlacement,
    ) -> bool {
        !raster.invalid_source
            && (raster.source.loaded_image().is_some()
                || raster.loading == crate::ImageLoading::Eager
                || self.raster_is_visible(node, raster)
                || node.raster_clip.is_some())
    }

    fn request_source(&mut self, source: &ImageSource) {
        match self.image_manager.request(source) {
            SourceRequest::Queued => self.set_source_fallback(source, Placeholder::Loading),
            SourceRequest::Backpressured => {}
            SourceRequest::Closed => self.set_source_fallback(source, Placeholder::Unavailable),
            SourceRequest::AlreadyAvailable => {}
        }
    }

    fn set_source_fallback(&mut self, source: &ImageSource, placeholder: Placeholder) {
        let ids: Vec<_> = self
            .images
            .iter()
            .filter_map(|(id, node)| {
                matches!(&node.surface, Surface::Raster(raster) if raster.source == *source)
                    .then_some(*id)
            })
            .collect();
        for id in ids {
            let damage = if let Some(node) = self.images.get_mut(&id) {
                if let Surface::Raster(raster) = &node.surface {
                    let text = placeholder.text(raster);
                    node.fallback = placeholder_image(raster.width, raster.height, &text);
                    Some(Self::node_damage_for(node))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(damage) = damage {
                self.add_damage(damage);
            }
        }
    }

    fn request_visible_sources(&mut self) {
        let sources: HashSet<_> = self
            .images
            .values()
            .filter_map(|node| match &node.surface {
                Surface::Raster(raster) if self.source_should_load(node, raster) => {
                    Some(raster.source.clone())
                }
                _ => None,
            })
            .collect();
        for source in sources {
            self.request_source(&source);
        }
    }

    pub(in crate::runtime) fn drain_load_results(&mut self) -> bool {
        let mut changed = false;
        for (source, result) in self.image_manager.take_results() {
            let bytes = result
                .as_ref()
                .map(|image| image.rgba8().len())
                .unwrap_or(0);
            self.image_manager.remove_cached(&source);
            self.evict_cache(bytes);
            let ready = self.image_manager.store_result(source.clone(), result);
            let ids: Vec<_> = self
                .images
                .iter()
                .filter_map(|(id, node)| {
                    matches!(&node.surface, Surface::Raster(raster) if raster.source == source)
                        .then_some(*id)
                })
                .collect();
            for id in ids {
                let damage = if let Some(node) = self.images.get_mut(&id) {
                    if let Surface::Raster(raster) = &node.surface {
                        let text = raster.unavailable_text().to_string();
                        node.fallback = (!ready)
                            .then(|| placeholder_image(raster.width, raster.height, &text))
                            .flatten();
                    }
                    Some(Self::node_damage_for(node))
                } else {
                    None
                };
                if let Some(damage) = damage {
                    self.add_damage(damage);
                }
            }
            self.raster_scene_dirty = true;
            changed = true;
        }
        changed
    }

    pub(in crate::runtime) fn raster_is_visible(
        &self,
        node: &ImageNode,
        raster: &RasterPlacement,
    ) -> bool {
        let clip = node.raster_clip.unwrap_or(Rect::new(
            0,
            0,
            usize::from(raster.width),
            usize::from(raster.height),
        ));
        let left = node.position.column.saturating_add(clip.column as i32);
        let top = node.position.line.saturating_add(clip.line as i32);
        let right = left.saturating_add(clip.width as i32);
        let bottom = top.saturating_add(clip.height as i32);
        right > 0
            && bottom > 0
            && left < i32::from(self.viewport.width)
            && top < i32::from(self.viewport.height)
    }

    pub(in crate::runtime) fn prepare_visible_rasters(&mut self) {
        self.request_visible_sources();
        let rasters: Vec<_> = self
            .images
            .values()
            .filter_map(|node| match &node.surface {
                Surface::Raster(raster) if self.raster_is_visible(node, raster) => {
                    Some(raster.clone())
                }
                _ => None,
            })
            .collect();
        for raster in rasters {
            let Some(key) = self.prepare_raster(&raster) else {
                continue;
            };
            if self.symbols_for_native
                || self.protocol == ImageProtocol::Symbols
                || raster.options.mode == ImageMode::Symbols
            {
                self.prepare_symbols(key, raster.full_width, raster.full_height);
            }
        }
    }
}
