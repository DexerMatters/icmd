//! Commit-published geometry for a selectable region, and the live selection a
//! region bakes into its own node each render.
//!
//! This is the same contract the editor's `LayoutProbe` uses, in both
//! directions: the paint pass publishes where the text ended up, and the
//! component publishes which bytes are selected, each side reading the other's
//! value rather than recomputing it.

use std::sync::{Arc, Mutex};

use crate::ScreenPosition;

use super::{Selection, SelectionDocument, SelectionStyles};

/// A selection range projected onto one text leaf, in that leaf's own byte
/// coordinates. This is what a painter reads to decide which cells are selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionOverlay {
    /// The selected byte range within the leaf.
    pub range: std::ops::Range<usize>,
    /// Whether the host region currently holds focus.
    pub focused: bool,
    /// The colours to paint the selected cells with.
    pub styles: SelectionStyles,
}

/// Where a region's text was painted, and the document it forms. The runs are
/// already tiled into document offsets, so a consumer only has to query.
#[derive(Debug, Clone)]
pub struct CommittedSelection {
    /// The painted content-box origin.
    pub origin: ScreenPosition,
    /// The painted content-box width in cells.
    pub width: usize,
    /// The painted content-box height in rows.
    pub height: usize,
    /// The tiled document built from the painted leaves.
    pub document: SelectionDocument,
}

/// The region's published geometry. `PartialEq` is pointer identity, exactly
/// like `LayoutProbe`: two frames of the same region carry the same probe even
/// though the document inside it changed, so a node comparison never reports a
/// spurious difference.
#[derive(Clone, Default)]
pub struct SelectionProbe(Arc<Mutex<Option<CommittedSelection>>>);

impl std::fmt::Debug for SelectionProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionProbe").finish_non_exhaustive()
    }
}

impl PartialEq for SelectionProbe {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SelectionProbe {}

impl SelectionProbe {
    /// A probe with no committed geometry yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish the geometry of the frame just committed.
    pub fn publish(&self, committed: CommittedSelection) {
        *self.0.lock().expect("selection probe poisoned") = Some(committed);
    }

    /// The most recently published geometry, if any frame has been committed.
    pub fn committed(&self) -> Option<CommittedSelection> {
        self.0.lock().expect("selection probe poisoned").clone()
    }
}

/// The live selection state a region attaches to its own host node. It is a
/// plain value, not shared mutable state: the component rebuilds it every
/// render, so a selection change is a change to the lowered tree and the
/// renderer cannot miss it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionConfig {
    /// The region's published geometry.
    pub probe: SelectionProbe,
    /// The current selection.
    pub selection: Selection,
    /// Whether the region currently holds focus.
    pub focused: bool,
    /// The colours to paint the selected cells with.
    pub styles: SelectionStyles,
}
