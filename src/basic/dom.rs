use crate::Image;

use super::props::DomProps;

/// Stable identity of a concrete node in a lowered DOM tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DomId(pub u64);

/// A host-only DOM node. Component types, hooks, keys, and user-defined props
/// are intentionally absent from this representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomNode {
    Element {
        id: DomId,
        props: DomProps,
        children: Vec<DomNode>,
    },
    Image {
        id: DomId,
        image: Image,
    },
}

impl DomNode {
    pub fn id(&self) -> DomId {
        match self {
            Self::Element { id, .. } | Self::Image { id, .. } => *id,
        }
    }

    pub fn children(&self) -> &[Self] {
        match self {
            Self::Element { children, .. } => children,
            Self::Image { .. } => &[],
        }
    }
}
