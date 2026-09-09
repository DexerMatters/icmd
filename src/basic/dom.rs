use crate::Image;

use super::{props::DomProps, text::Text};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DomId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomNode {
    Element {
        id: DomId,
        props: DomProps,
        children: Vec<DomNode>,
    },
    Text {
        id: DomId,
        text: Text,
    },
    Image {
        id: DomId,
        image: Image,
    },
}

impl DomNode {
    pub fn id(&self) -> DomId {
        match self {
            Self::Element { id, .. } | Self::Text { id, .. } | Self::Image { id, .. } => *id,
        }
    }

    pub fn children(&self) -> &[Self] {
        match self {
            Self::Element { children, .. } => children,
            Self::Text { .. } | Self::Image { .. } => &[],
        }
    }
}
