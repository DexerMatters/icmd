//! Editor surface state: the text, selection, caret, scroll offsets, and styles
//! a text editor renders from, plus the committed-layout snapshot the renderer
//! publishes back through a probe.

use std::sync::{Arc, Mutex};

use crate::basic::text_layout::TextLayout;
use crate::{TextStyle, TextWrap};

/// Mutable render state of one text editor: value, selection, caret, focus,
/// placeholder, wrap mode, styles, and viewport scroll offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSurface {
    /// Current editor text.
    pub value: String,
    /// Selected byte range in `value`, or `None` when nothing is selected.
    pub selection: Option<std::ops::Range<usize>>,
    /// Caret position as a byte offset into `value`.
    pub caret: usize,
    /// Whether the editor currently owns focus.
    pub focused: bool,
    /// Text shown in place of `value` while the value is empty.
    pub placeholder: String,
    /// Line-wrapping mode applied when laying out `value`.
    pub wrap: TextWrap,
    /// Style used to draw the placeholder text.
    pub placeholder_style: TextStyle,
    /// Style used to draw the selection while the editor is focused.
    pub selection_style: TextStyle,
    /// Style used to draw the selection while the editor is unfocused.
    pub selection_inactive_style: TextStyle,
    /// Style used to draw the caret.
    pub caret_style: TextStyle,
    /// Horizontal scroll offset in cells.
    pub scroll_x: usize,
    /// Vertical scroll offset in rows.
    pub scroll_y: usize,
}

/// A text layout already computed for a viewport, with the scroll offsets and
/// emoji-merging mode that produced it.
#[derive(Debug, Clone)]
pub struct CommittedLayout {
    /// Layout of the editor text for this commit.
    pub layout: Arc<TextLayout>,
    /// Viewport width in cells used for the layout.
    pub viewport_width: usize,
    /// Viewport height in rows used for the layout.
    pub viewport_height: usize,
    /// Horizontal scroll offset applied to the layout, in cells.
    pub applied_x: usize,
    /// Vertical scroll offset applied to the layout, in rows.
    pub applied_y: usize,
    /// Emoji-merging mode used for the layout.
    pub emoji_merging: crate::EmojiMerging,
}

/// Shared slot holding the most recently committed layout, so a probe can read
/// what the renderer actually laid out.
#[derive(Clone, Default)]
pub struct LayoutProbe(Arc<Mutex<Option<CommittedLayout>>>);

/// Opaque by design: the shared slot has no meaningful structural rendering.
impl std::fmt::Debug for LayoutProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutProbe").finish_non_exhaustive()
    }
}

/// Two probes are equal when they share one slot, not when their contents match.
impl PartialEq for LayoutProbe {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for LayoutProbe {}

impl LayoutProbe {
    /// An empty probe with no committed layout.
    pub fn new() -> Self {
        Self::default()
    }

    /// Store `committed` as the latest layout, replacing any earlier one.
    pub fn publish(&self, committed: CommittedLayout) {
        *self.0.lock().expect("layout probe poisoned") = Some(committed);
    }

    /// Latest committed layout, or `None` if nothing has been published.
    pub fn committed(&self) -> Option<CommittedLayout> {
        self.0.lock().expect("layout probe poisoned").clone()
    }

    /// Emoji-merging mode of the latest layout, defaulting to
    /// `EmojiMerging::Merge` before the first commit.
    pub fn emoji_merging(&self) -> crate::EmojiMerging {
        self.committed()
            .map_or(crate::EmojiMerging::Merge, |committed| {
                committed.emoji_merging
            })
    }
}
