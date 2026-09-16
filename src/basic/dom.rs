//! Render-tree node model: opaque region identity plus the element, text,
//! image, and raster node kinds that make up a node tree.

use crate::{Image, RasterPlacement};

use super::{props::DomProps, text::Text};

/// Opaque element identity. The numeric value is framework-owned: callers can
/// read it for diagnostics but cannot fabricate one, so a stale or invented ID
/// can never be mistaken for a live region.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DomId(u64);

impl DomId {
    /// The runtime root: a fixed sentinel rather than a fabricated region id,
    /// and never focusable.
    pub const ROOT: Self = Self(0);

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn root() -> Self {
        Self::ROOT
    }

    /// The raw identifier value.
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

/// One node in the render tree: an element carrying props and children, or a
/// leaf holding text, an image, or a raster placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomNode {
    /// An element node with layout props and child nodes.
    Element {
        /// Identity of this element.
        id: DomId,
        /// Layout, style, and event props applied to this element.
        props: DomProps,
        /// Child nodes in render order.
        children: Vec<DomNode>,
    },
    /// A leaf node holding styled text.
    Text {
        /// Identity of this text node.
        id: DomId,
        /// Text content and style.
        text: Text,
    },
    /// A leaf node holding a decoded image.
    Image {
        /// Identity of this image node.
        id: DomId,
        /// Image resource and its placement.
        image: Image,
    },
    /// A leaf node holding a raster surface placement.
    Raster {
        /// Identity of this raster node.
        id: DomId,
        /// Raster surface and its placement.
        raster: RasterPlacement,
    },
}

impl DomNode {
    /// Identity of this node.
    pub fn id(&self) -> DomId {
        match self {
            Self::Element { id, .. }
            | Self::Text { id, .. }
            | Self::Image { id, .. }
            | Self::Raster { id, .. } => *id,
        }
    }

    /// Child nodes in render order, or an empty slice for leaf nodes.
    pub fn children(&self) -> &[Self] {
        match self {
            Self::Element { children, .. } => children,
            Self::Text { .. } | Self::Image { .. } | Self::Raster { .. } => &[],
        }
    }
}
