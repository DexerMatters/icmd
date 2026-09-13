use std::collections::HashMap;

use crate::{ImageId, Operation};

use super::Commit;
use super::types::{PaintContent, PaintFragment, PaintKey};

impl Commit {
    // Returns the operations for this commit plus the one canonical sorted
    // order for the new scene. Callers store that order instead of collecting
    // and sorting a second time.
    pub(super) fn diff_scene(
        &mut self,
        next: &HashMap<PaintKey, PaintFragment>,
    ) -> (Vec<Operation>, Vec<PaintKey>) {
        let mut operations = Vec::new();
        let mut retired = Vec::new();
        // The previous order is replaced by the new one, so it can be moved out
        // instead of cloned.
        let old_keys = std::mem::take(&mut self.scene_order);
        for key in old_keys {
            let Some(new) = next.get(&key) else {
                operations.push(Operation::Remove {
                    id: self.image_id(key),
                });
                retired.push(key);
                continue;
            };
            let id = self.image_id(key);
            let old = &self.scene[&key];
            if old.content != new.content {
                match (&old.content, &new.content) {
                    (PaintContent::Cells(previous), PaintContent::Cells(image)) => {
                        if let Some((rect, rows)) = previous.diff_patch_rect(image) {
                            operations.push(Operation::PatchRect { id, rect, rows });
                        } else {
                            operations.push(Operation::Replace {
                                id,
                                image: image.clone(),
                            });
                        }
                    }
                    (_, PaintContent::Raster(raster)) => {
                        operations.push(Operation::ReplaceRaster {
                            id,
                            raster: raster.clone(),
                        })
                    }
                    // Cell/raster transitions cannot preserve a patchable
                    // footprint and retain the existing replacement contract.
                    (_, PaintContent::Cells(image)) => operations.push(Operation::Replace {
                        id,
                        image: image.clone(),
                    }),
                }
            }
            if old.raster_clip != new.raster_clip && matches!(new.content, PaintContent::Raster(_))
            {
                operations.push(Operation::SetRasterClip {
                    id,
                    clip: new.raster_clip,
                });
            }
            if old.position != new.position {
                operations.push(Operation::Move {
                    id,
                    position: new.position,
                });
            }
            if old.level != new.level {
                operations.push(Operation::SetLevel {
                    id,
                    level: new.level,
                });
            }
            if old.order != new.order {
                operations.push(Operation::SetOrder {
                    id,
                    order: new.order,
                });
            }
        }
        for key in retired {
            self.image_ids.remove(&key);
        }

        let mut next_keys: Vec<_> = next.keys().copied().collect();
        next_keys.sort_by_key(|key| (next[key].order, key.node.0, key.role));
        for key in &next_keys {
            let key = *key;
            let value = &next[&key];
            if !self.scene.contains_key(&key) {
                let id = self.image_id(key);
                match &value.content {
                    PaintContent::Cells(image) => operations.push(Operation::Create {
                        id,
                        image: image.clone(),
                        position: value.position,
                        level: value.level,
                    }),
                    PaintContent::Raster(raster) => operations.push(Operation::CreateRaster {
                        id,
                        raster: raster.clone(),
                        position: value.position,
                        level: value.level,
                    }),
                }
                if let PaintContent::Raster(_) = value.content
                    && value.raster_clip.is_some()
                {
                    operations.push(Operation::SetRasterClip {
                        id,
                        clip: value.raster_clip,
                    });
                }
                operations.push(Operation::SetOrder {
                    id,
                    order: value.order,
                });
            }
        }
        (operations, next_keys)
    }

    fn image_id(&mut self, key: PaintKey) -> ImageId {
        if let Some(id) = self.image_ids.get(&key) {
            return *id;
        }
        let id = ImageId(self.next_image_id);
        self.next_image_id = self
            .next_image_id
            .checked_add(1)
            .expect("commit exhausted image IDs");
        self.image_ids.insert(key, id);
        id
    }
}
