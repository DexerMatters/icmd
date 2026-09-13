use std::sync::{Arc, Mutex};

use crate::basic::text_layout::TextLayout;
use crate::{TextStyle, TextWrap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorSurface {
    pub(crate) value: String,
    pub(crate) selection: Option<(usize, usize)>,
    pub(crate) caret: usize,
    pub(crate) focused: bool,
    pub(crate) placeholder: String,
    pub(crate) wrap: TextWrap,
    pub(crate) placeholder_style: TextStyle,
    pub(crate) selection_style: TextStyle,
    pub(crate) selection_inactive_style: TextStyle,
    pub(crate) caret_style: TextStyle,
    pub(crate) scroll_x: usize,
    pub(crate) scroll_y: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CommittedLayout {
    pub(crate) layout: Arc<TextLayout>,
    pub(crate) viewport_width: usize,
    pub(crate) viewport_height: usize,
    pub(crate) applied_x: usize,
    pub(crate) applied_y: usize,
    pub(crate) emoji_merging: crate::EmojiMerging,
}

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

    pub(crate) fn publish(&self, committed: CommittedLayout) {
        *self.0.lock().expect("layout probe poisoned") = Some(committed);
    }

    pub(crate) fn committed(&self) -> Option<CommittedLayout> {
        self.0.lock().expect("layout probe poisoned").clone()
    }

    pub(crate) fn emoji_merging(&self) -> crate::EmojiMerging {
        self.committed()
            .map_or(crate::EmojiMerging::Merge, |committed| {
                committed.emoji_merging
            })
    }
}
