//! Public references to concrete host elements and their committed state.
//!
//! An [`ElementRef`] is intentionally a snapshot-oriented counterpart to a
//! browser DOM ref: the ref is stable across renders, while [`ElementSnapshot`]
//! is replaced whenever the commit stage observes a meaningful change.

use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

use crossterm::style::{Attributes as TerminalAttributes, Color};

use crate::{
    Align, BorderKind, Edges, Fill, Justify, Layout, Overflow, ScrollAxes, ScrollOffset, Visibility,
};

/// A rectangle in viewport-relative terminal-cell coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementRect {
    /// Row of the top edge. Negative values mean the box starts above the viewport.
    pub line: i32,
    /// Column of the left edge. Negative values mean the box starts left of the viewport.
    pub column: i32,
    /// Width in terminal columns.
    pub width: u32,
    /// Height in terminal rows.
    pub height: u32,
}

impl ElementRect {
    /// Builds a rectangle, clamping negative dimensions to zero.
    pub const fn new(line: i32, column: i32, width: i32, height: i32) -> Self {
        Self {
            line,
            column,
            width: if width < 0 { 0 } else { width as u32 },
            height: if height < 0 { 0 } else { height as u32 },
        }
    }

    /// Column just past the right edge, saturating on overflow.
    pub fn right(self) -> i32 {
        self.column
            .saturating_add(i32::try_from(self.width).unwrap_or(i32::MAX))
    }

    /// Row just past the bottom edge, saturating on overflow.
    pub fn bottom(self) -> i32 {
        self.line
            .saturating_add(i32::try_from(self.height).unwrap_or(i32::MAX))
    }

    pub(crate) fn from_rect(rect: crate::runtime::commit::geometry::RectI) -> Self {
        Self::new(rect.line, rect.column, rect.width, rect.height)
    }
}

/// Resolved scroll state for a scrollable host element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementScrollState {
    /// Axes enabled for this scroll host.
    pub axes: ScrollAxes,
    /// Current horizontal and vertical offset in cells.
    pub offset: ScrollOffset,
    /// Largest legal horizontal and vertical offset in cells.
    pub max_offset: ScrollOffset,
    /// Logical content extent before viewport clipping and scrolling.
    pub content_width: u32,
    /// Logical content extent before viewport clipping and scrolling.
    pub content_height: u32,
}

impl ElementScrollState {
    /// Current horizontal scroll offset.
    pub const fn scroll_left(self) -> u32 {
        self.offset.x
    }

    /// Current vertical scroll offset.
    pub const fn scroll_top(self) -> u32 {
        self.offset.y
    }

    /// Returns the current offset in both axes.
    pub const fn current_offset(self) -> ScrollOffset {
        self.offset
    }

    /// Returns the largest legal offset in both axes.
    pub const fn max_scroll_offset(self) -> ScrollOffset {
        self.max_offset
    }

    /// Returns the logical content extent as an origin-anchored rectangle.
    pub const fn content_extent(self) -> ElementRect {
        ElementRect::new(0, 0, self.content_width as i32, self.content_height as i32)
    }
}

/// Effective border styling used by the committed element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedBorderStyle {
    /// Border shape, or `None` when no border is configured.
    pub kind: Option<BorderKind>,
    /// Whether each edge is painted.
    pub edges: Edges<bool>,
    /// Foreground colour used for the border.
    pub foreground: Color,
    /// Optional border background colour.
    pub background: Option<Color>,
    /// Effective terminal attributes.
    pub attributes: TerminalAttributes,
}

/// Effective text styling after inheritance and defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedTextStyle {
    /// Effective foreground colour.
    pub foreground: Color,
    /// Effective background colour, if one is set.
    pub background: Option<Color>,
    /// Effective terminal attributes.
    pub attributes: TerminalAttributes,
}

/// Effective non-geometric styling for a committed host element.
///
/// The actual resolved size and position are exposed by the snapshot's
/// rectangles. The original `Dimension` and `AxisPosition` requests are not
/// repeated here because they are not computed geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedElementStyle {
    /// Child layout direction.
    pub layout: Layout,
    /// Outside spacing in cells.
    pub margin: Edges<u16>,
    /// Inside spacing in cells.
    pub padding: Edges<u16>,
    /// Child gap in cells.
    pub gap: u16,
    /// Main-axis distribution.
    pub justify: Justify,
    /// Cross-axis alignment.
    pub align: Align,
    /// Effective horizontal overflow policy.
    pub overflow_x: Overflow,
    /// Effective vertical overflow policy.
    pub overflow_y: Overflow,
    /// Effective overflow shorthand before axis-specific overrides.
    pub overflow: Overflow,
    /// Effective visibility policy.
    pub visibility: Visibility,
    /// Effective paint-order offset.
    pub z_index: i32,
    /// Background colour, if set.
    pub background: Option<Color>,
    /// Background fill, if set.
    pub fill: Option<Fill>,
    /// Effective border styling.
    pub border: ResolvedBorderStyle,
    /// Effective inherited text styling.
    pub text: ResolvedTextStyle,
}

/// The latest committed state of one concrete host element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementSnapshot {
    id: crate::DomId,
    bounding_rect: ElementRect,
    content_rect: ElementRect,
    visible_rect: Option<ElementRect>,
    style: ResolvedElementStyle,
    scroll: Option<ElementScrollState>,
}

impl ElementSnapshot {
    /// Stable DOM identity of the host element.
    pub const fn id(&self) -> crate::DomId {
        self.id
    }

    /// Alias for [`Self::id`].
    pub const fn dom_id(&self) -> crate::DomId {
        self.id
    }

    /// Viewport-relative border box, excluding margins.
    pub const fn bounding_rect(&self) -> ElementRect {
        self.bounding_rect
    }

    /// Child viewport after border, padding, and reserved scrollbar cells.
    pub const fn content_rect(&self) -> ElementRect {
        self.content_rect
    }

    /// Portion of the border box visible after viewport and ancestor clipping.
    pub const fn visible_rect(&self) -> Option<ElementRect> {
        self.visible_rect
    }

    /// Whether the element currently has a visible painted portion.
    pub const fn is_visible(&self) -> bool {
        self.visible_rect.is_some()
    }

    /// Effective resolved style.
    pub const fn resolved_style(&self) -> &ResolvedElementStyle {
        &self.style
    }

    /// Alias for [`Self::resolved_style`].
    pub const fn style(&self) -> &ResolvedElementStyle {
        &self.style
    }

    /// Current scroll state, when this host is scrollable.
    pub const fn scroll(&self) -> Option<ElementScrollState> {
        self.scroll
    }

    /// Alias for [`Self::scroll`].
    pub const fn scroll_state(&self) -> Option<ElementScrollState> {
        self.scroll
    }

    pub(crate) fn new(
        id: crate::DomId,
        bounding_rect: ElementRect,
        content_rect: ElementRect,
        visible_rect: Option<ElementRect>,
        style: ResolvedElementStyle,
        scroll: Option<ElementScrollState>,
    ) -> Self {
        Self {
            id,
            bounding_rect,
            content_rect,
            visible_rect,
            style,
            scroll,
        }
    }
}

/// A stable handle to the latest committed state of one host element.
#[derive(Clone, Default)]
pub struct ElementRef {
    state: Arc<Mutex<Option<ElementSnapshot>>>,
}

impl fmt::Debug for ElementRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElementRef").finish_non_exhaustive()
    }
}

impl PartialEq for ElementRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for ElementRef {}

impl Hash for ElementRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.state).hash(state);
    }
}

impl ElementRef {
    /// Creates an unattached ref whose `current()` value is `None`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the latest committed snapshot, or `None` before attachment and
    /// after the host is hidden or unmounted.
    pub fn current(&self) -> Option<ElementSnapshot> {
        self.state.lock().expect("element ref poisoned").clone()
    }

    pub(crate) fn publish(&self, snapshot: Option<ElementSnapshot>) -> bool {
        let mut current = self.state.lock().expect("element ref poisoned");
        if *current == snapshot {
            return false;
        }
        *current = snapshot;
        true
    }

    pub(crate) fn clear(&self) -> bool {
        self.publish(None)
    }
}
