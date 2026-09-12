//! The crate-private declarative editor surface.
//!
//! An editor surface is a text value plus the interaction decorations that the
//! canonical layout must place: selection, caret, and placeholder. It contains
//! no mutable editor behavior. The commit pipeline builds the canonical layout
//! at the *committed* content width, so painted rows, the caret, pointer hits,
//! and scroll extents all share one coordinate space even when a parent clamps
//! the requested width.
//!
//! The commit pass publishes the layout it painted into a passive
//! [`LayoutProbe`]. Pointer handlers read that exact layout instead of asking
//! for another render, so no settling loop is required for correct wrapping.

use std::sync::{Arc, Mutex};

use crate::basic::text_layout::TextLayout;
use crate::{TextStyle, TextWrap};

/// Editor-only paint instructions attached to a [`crate::Text`] surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorSurface {
    /// The normalized value the editor is displaying.
    pub(crate) value: String,
    /// Selected source byte range, if any.
    pub(crate) selection: Option<(usize, usize)>,
    /// Caret source byte offset.
    pub(crate) caret: usize,
    pub(crate) focused: bool,
    pub(crate) placeholder: String,
    pub(crate) wrap: TextWrap,
    pub(crate) placeholder_style: TextStyle,
    pub(crate) selection_style: TextStyle,
    pub(crate) selection_inactive_style: TextStyle,
    pub(crate) caret_style: TextStyle,
    /// The scroll offsets this render requested. The commit pass records them
    /// so pointer coordinates can be reconciled with the layout.
    pub(crate) scroll_x: usize,
    pub(crate) scroll_y: usize,
}

/// One committed frame's layout, as painted.
///
/// `width` and `height` are the content box the commit pass granted, and
/// `applied_x`/`applied_y` are the scroll offsets that frame was laid out with.
/// Pointer hit-testing and caret reveal read them so both resolve against
/// exactly the row table that produced the visible frame.
#[derive(Debug, Clone)]
pub(crate) struct CommittedLayout {
    pub(crate) layout: Arc<TextLayout>,
    /// The visible viewport the scroll host could actually paint. Caret reveal
    /// and scroll extent use this rather than the surface's own document box,
    /// so a parent that constrains the host cannot put the caret outside the
    /// viewport.
    pub(crate) viewport_width: usize,
    pub(crate) viewport_height: usize,
    /// The scroll offsets this frame was laid out with.
    pub(crate) applied_x: usize,
    pub(crate) applied_y: usize,
}

/// A passive channel from the commit pipeline back to the component that owns a
/// surface. The commit writes; the component reads. It never requests a render.
#[derive(Clone, Default)]
pub(crate) struct LayoutProbe(Arc<Mutex<Option<CommittedLayout>>>);

impl std::fmt::Debug for LayoutProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutProbe").finish_non_exhaustive()
    }
}

impl PartialEq for LayoutProbe {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for LayoutProbe {}

impl LayoutProbe {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Publish the layout that produced the current frame.
    pub(crate) fn publish(&self, committed: CommittedLayout) {
        *self.0.lock().expect("layout probe poisoned") = Some(committed);
    }

    /// The last committed layout, if this surface has painted a frame.
    pub(crate) fn committed(&self) -> Option<CommittedLayout> {
        self.0.lock().expect("layout probe poisoned").clone()
    }
}
