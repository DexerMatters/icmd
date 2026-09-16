//! Integer cell geometry for the commit stage: the `RectI` rectangle plus the
//! resolvers that turn dimension, percent, position, and justify values into
//! concrete cell counts.

use crate::{AxisPosition, Dimension, Edges, Justify, Percent, PercentBasis};

/// Rectangle in terminal cells, positioned by its top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectI {
    /// Top row, in cells, measured from the viewport origin.
    pub line: i32,
    /// Left column, in cells, measured from the viewport origin.
    pub column: i32,
    /// Width in cells; never negative.
    pub width: i32,
    /// Height in cells; never negative.
    pub height: i32,
}

impl RectI {
    /// Builds a rectangle, clamping negative widths and heights to zero cells.
    pub fn new(line: i32, column: i32, width: i32, height: i32) -> Self {
        Self {
            line,
            column,
            width: width.max(0),
            height: height.max(0),
        }
    }

    pub(super) fn right(self) -> i32 {
        self.column.saturating_add(self.width)
    }

    pub(super) fn bottom(self) -> i32 {
        self.line.saturating_add(self.height)
    }

    pub(super) fn inset(self, edges: Edges<u16>) -> Self {
        Self::new(
            self.line.saturating_add(edges.top as i32),
            self.column.saturating_add(edges.left as i32),
            self.width
                .saturating_sub(edges.left as i32)
                .saturating_sub(edges.right as i32),
            self.height
                .saturating_sub(edges.top as i32)
                .saturating_sub(edges.bottom as i32),
        )
    }

    pub(super) fn intersection(self, other: Self) -> Option<Self> {
        let line = self.line.max(other.line);
        let column = self.column.max(other.column);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > column && bottom > line).then(|| {
            Self::new(
                line,
                column,
                right.saturating_sub(column),
                bottom.saturating_sub(line),
            )
        })
    }
}

pub(super) fn resolve_percent(
    value: Percent,
    available: Option<i32>,
    viewport: i32,
) -> Option<i32> {
    match value.basis() {
        PercentBasis::Available => available.map(|size| value.resolve(size)),
        PercentBasis::Viewport => Some(value.resolve(viewport)),
    }
}

pub(super) fn resolve_dimension(
    value: Dimension,
    available: Option<i32>,
    viewport: i32,
    intrinsic: i32,
) -> i32 {
    match value {
        Dimension::Cells(value) => value as i32,
        Dimension::Percent(value) => {
            resolve_percent(value, available, viewport).unwrap_or(intrinsic)
        }
        Dimension::Max => available.unwrap_or(intrinsic),
        Dimension::Auto => intrinsic.min(available.unwrap_or(intrinsic)),
    }
    .max(0)
}

pub(super) fn definite_dimension(
    value: Dimension,
    available: Option<i32>,
    viewport: i32,
) -> Option<i32> {
    match value {
        Dimension::Cells(value) => Some(value as i32),
        Dimension::Percent(value) => {
            resolve_percent(value, available, viewport).map(|size| size.max(0))
        }
        Dimension::Max => available.map(|size| size.max(0)),
        Dimension::Auto => None,
    }
}

pub(super) fn resolve_position(
    value: AxisPosition,
    start: i32,
    available: i32,
    viewport: i32,
    child: i32,
) -> i32 {
    let travel = available.saturating_sub(child);
    start.saturating_add(match value {
        AxisPosition::Start => 0,
        AxisPosition::Cells(value) => value,
        AxisPosition::Percent(value) => resolve_percent(value, Some(travel), viewport)
            .expect("a percentage always has a layout reference"),
        AxisPosition::Center => travel / 2,
        AxisPosition::End => travel,
    })
}

pub(super) fn justify_offset(value: Justify, remaining: i32) -> i32 {
    match value {
        Justify::Center => remaining / 2,
        Justify::End => remaining,
        _ => 0,
    }
}
