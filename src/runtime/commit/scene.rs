use std::collections::HashMap;

use crate::{ImageId, Operation};

use super::Commit;
use super::types::{PaintFragment, PaintKey};

impl Commit {
    pub(super) fn diff_scene(&mut self, next: &HashMap<PaintKey, PaintFragment>) -> Vec<Operation> {
        let mut operations = Vec::new();
        let mut old_keys: Vec<_> = self.scene.keys().copied().collect();
        old_keys.sort_by_key(|key| (self.scene[key].order, key.node.0, key.role));
        let mut retired = Vec::new();
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
            if old.image != new.image {
                operations.push(Operation::Replace {
                    id,
                    image: new.image.clone(),
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
        for key in next_keys {
            let value = &next[&key];
            if !self.scene.contains_key(&key) {
                let id = self.image_id(key);
                operations.push(Operation::Create {
                    id,
                    image: value.image.clone(),
                    position: value.position,
                    level: value.level,
                });
                operations.push(Operation::SetOrder {
                    id,
                    order: value.order,
                });
            }
        }
        operations
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
