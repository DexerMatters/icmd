use std::collections::HashMap;

use crossterm::style::{Attributes, Color};

use crate::{
    Align, AxisPosition, BorderKind, Dimension, DomId, Edges, Fill, Image, Justify, Layout,
    Overflow, OverflowScrollbarStyle, ScreenPosition, Visibility,
};

use super::geometry::RectI;

type AxisBounds = Option<(i32, i32)>;

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
    pub(super) overflow_x: Overflow,
    pub(super) overflow_y: Overflow,
    pub(super) scroll: ComputedScrollStyle,
    pub(super) visibility: Visibility,
    pub(super) z_index: i32,
    pub(super) background: Option<Color>,
    pub(super) fill: Option<Fill>,
    pub(super) border: ComputedBorder,
    pub(super) text: ComputedText,
}

pub(super) struct ScrollSpec<'a> {
    pub(super) horizontal: bool,
    pub(super) vertical: bool,
    pub(super) always_horizontal: bool,
    pub(super) always_vertical: bool,
    pub(super) draw_scrollbar: bool,
    pub(super) wheel_step: u16,
    pub(super) wheel: bool,
    pub(super) enable_mouse: bool,
    pub(super) scrollbar: &'a OverflowScrollbarStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComputedScrollStyle {
    pub(super) wheel: bool,
    pub(super) enable_mouse: bool,
    pub(super) wheel_step: u16,
    pub(super) draw_scrollbar: bool,
    pub(super) scrollbar: OverflowScrollbarStyle,
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
    Axes {
        horizontal: Option<(i32, i32)>,
        vertical: Option<(i32, i32)>,
    },
    Empty,
}

impl Clip {
    pub(super) fn is_empty(self) -> bool {
        match self {
            Self::Empty => true,
            Self::Unbounded | Self::Bounded(_) => false,
            Self::Axes {
                horizontal,
                vertical,
            } => {
                horizontal.is_some_and(|(start, end)| end <= start)
                    || vertical.is_some_and(|(start, end)| end <= start)
            }
        }
    }

    pub(super) fn intersection(self, rect: RectI) -> Option<RectI> {
        if rect.width <= 0 || rect.height <= 0 || self.is_empty() {
            return None;
        }
        let (horizontal, vertical) = self.bounds();
        let mut visible = rect;
        if let Some((start, end)) = vertical {
            let top = visible.line.max(start);
            let bottom = visible.bottom().min(end);
            visible.line = top;
            visible.height = bottom.saturating_sub(top);
        }
        if let Some((start, end)) = horizontal {
            let left = visible.column.max(start);
            let right = visible.right().min(end);
            visible.column = left;
            visible.width = right.saturating_sub(left);
        }
        (visible.width > 0 && visible.height > 0).then_some(visible)
    }

    pub(super) fn restrict(self, rect: RectI) -> Self {
        self.restrict_axes(rect, true, true)
    }

    pub(super) fn restrict_axes(self, rect: RectI, horizontal: bool, vertical: bool) -> Self {
        if rect.width <= 0 || rect.height <= 0 || self.is_empty() {
            return Self::Empty;
        }
        let (mut current_horizontal, mut current_vertical) = self.bounds();
        if horizontal {
            let next = intersect_bounds(current_horizontal, (rect.column, rect.right()));
            if current_horizontal.is_some() && next.is_none() {
                return Self::Empty;
            }
            current_horizontal = next;
        }
        if vertical {
            let next = intersect_bounds(current_vertical, (rect.line, rect.bottom()));
            if current_vertical.is_some() && next.is_none() {
                return Self::Empty;
            }
            current_vertical = next;
        }
        match (current_horizontal, current_vertical) {
            (None, None) => Self::Unbounded,
            (Some(horizontal), Some(vertical)) => Self::Bounded(RectI::new(
                vertical.0,
                horizontal.0,
                horizontal.1.saturating_sub(horizontal.0),
                vertical.1.saturating_sub(vertical.0),
            )),
            (horizontal, vertical) => Self::Axes {
                horizontal,
                vertical,
            },
        }
    }

    fn bounds(self) -> (AxisBounds, AxisBounds) {
        match self {
            Self::Unbounded => (None, None),
            Self::Bounded(rect) => (
                Some((rect.column, rect.right())),
                Some((rect.line, rect.bottom())),
            ),
            Self::Axes {
                horizontal,
                vertical,
            } => (horizontal, vertical),
            Self::Empty => (Some((0, 0)), Some((0, 0))),
        }
    }
}

fn intersect_bounds(left: Option<(i32, i32)>, right: (i32, i32)) -> Option<(i32, i32)> {
    let result = match left {
        Some((start, end)) => (start.max(right.0), end.min(right.1)),
        None => right,
    };
    (result.1 > result.0).then_some(result)
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
