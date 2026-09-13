use crate::{Image, RasterPlacement};

use super::{props::DomProps, text::Text};

// Opaque element identity. The numeric value is framework-owned: callers can
// read it for diagnostics but cannot fabricate one, so a stale or invented ID
// can never be mistaken for a live region.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DomId(u64);

impl DomId {
    // The runtime root. It is a fixed sentinel rather than a fabricated region
    // id, and it is never focusable.
    pub const ROOT: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn root() -> Self {
        Self::ROOT
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Debug for DomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DomId({})", self.0)
    }
}

impl std::fmt::Display for DomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

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
    Raster {
        id: DomId,
        raster: RasterPlacement,
    },
}

impl DomNode {
    pub fn id(&self) -> DomId {
        match self {
            Self::Element { id, .. }
            | Self::Text { id, .. }
            | Self::Image { id, .. }
            | Self::Raster { id, .. } => *id,
        }
    }

    pub fn children(&self) -> &[Self] {
        match self {
            Self::Element { children, .. } => children,
            Self::Text { .. } | Self::Image { .. } | Self::Raster { .. } => &[],
        }
    }
}
