use std::collections::HashMap;

use crossterm::style::{Attributes, Color};

use crate::{
    Align, AxisPosition, BorderKind, Dimension, DomId, Edges, Fill, Image, Justify, Layout,
    Overflow, OverflowScrollbarStyle, ScreenPosition, ScrollAxes, Visibility,
};

use super::geometry::RectI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ComputedText {
    pub(super) foreground: Color,
    pub(super) background: Option<Color>,
    pub(super) attributes: Attributes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ComputedBorder {
    pub(super) kind: Option<BorderKind>,
    pub(super) edges: Edges<bool>,
    pub(super) foreground: Color,
    pub(super) background: Option<Color>,
    pub(super) attributes: Attributes,
}

impl ComputedBorder {
    pub(super) fn insets(self) -> Edges<u16> {
        if self.kind.is_none() {
            return Edges::all(0);
        }
        Edges {
            top: self.edges.top as u16,
            right: self.edges.right as u16,
            bottom: self.edges.bottom as u16,
            left: self.edges.left as u16,
        }
    }

    pub(super) fn visible(self) -> bool {
        self.kind.is_some()
            && (self.edges.top || self.edges.right || self.edges.bottom || self.edges.left)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComputedStyle {
    pub(super) layout: Layout,
    pub(super) width: Dimension,
    pub(super) height: Dimension,
    pub(super) line: AxisPosition,
    pub(super) column: AxisPosition,
    pub(super) margin: Edges<u16>,
    pub(super) padding: Edges<u16>,
    pub(super) gap: u16,
    pub(super) justify: Justify,
    pub(super) align: Align,
    pub(super) overflow: Overflow,
    pub(super) visibility: Visibility,
    pub(super) z_index: i32,
    pub(super) background: Option<Color>,
    pub(super) fill: Option<Fill>,
    pub(super) border: ComputedBorder,
    pub(super) text: ComputedText,
}

pub(super) struct ScrollSpec<'a> {
    pub(super) axes: ScrollAxes,
    pub(super) draw_scrollbar: bool,
    pub(super) wheel_step: u16,
    pub(super) scrollbar: &'a OverflowScrollbarStyle,
    pub(super) always: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum PaintRole {
    Background,
    BorderTop,
    BorderRight,
    BorderBottom,
    BorderLeft,
    Content,
    ScrollbarVertical,
    ScrollbarHorizontal,
    ScrollbarCorner,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Clip {
    Unbounded,
    Bounded(RectI),
    Empty,
}

impl Clip {
    pub(super) fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }

    pub(super) fn intersection(self, rect: RectI) -> Option<RectI> {
        match self {
            Self::Unbounded => (rect.width > 0 && rect.height > 0).then_some(rect),
            Self::Bounded(clip) => clip.intersection(rect),
            Self::Empty => None,
        }
    }

    pub(super) fn restrict(self, rect: RectI) -> Self {
        self.intersection(rect).map_or(Self::Empty, Self::Bounded)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct PaintKey {
    pub(super) node: DomId,
    pub(super) role: PaintRole,
}

#[derive(Clone)]
pub(super) struct PaintFragment {
    pub(super) image: Image,
    pub(super) position: ScreenPosition,
    pub(super) level: i32,
    pub(super) order: u64,
}

pub(super) struct BorderPainter<'a> {
    pub(super) scene: &'a mut HashMap<PaintKey, PaintFragment>,
    pub(super) node: DomId,
    pub(super) level: i32,
    pub(super) clip: Clip,
    pub(super) order: &'a mut u64,
}

pub(super) fn make_frame(
    operations: Vec<crate::Operation>,
    viewport: Option<crate::Size>,
) -> crate::Frame {
    crate::Frame {
        operations,
        viewport,
        force_redraw: false,
    }
}
