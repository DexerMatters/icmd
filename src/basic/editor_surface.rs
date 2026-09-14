use std::sync::{Arc, Mutex};

use crate::basic::text_layout::TextLayout;
use crate::{TextStyle, TextWrap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSurface {
    pub value: String,
    pub selection: Option<(usize, usize)>,
    pub caret: usize,
    pub focused: bool,
    pub placeholder: String,
    pub wrap: TextWrap,
    pub placeholder_style: TextStyle,
    pub selection_style: TextStyle,
    pub selection_inactive_style: TextStyle,
    pub caret_style: TextStyle,
    pub scroll_x: usize,
    pub scroll_y: usize,
}

#[derive(Debug, Clone)]
pub struct CommittedLayout {
    pub layout: Arc<TextLayout>,
    pub viewport_width: usize,
    pub viewport_height: usize,
    pub applied_x: usize,
    pub applied_y: usize,
    pub emoji_merging: crate::EmojiMerging,
}

#[derive(Clone, Default)]
pub struct LayoutProbe(Arc<Mutex<Option<CommittedLayout>>>);

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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, committed: CommittedLayout) {
        *self.0.lock().expect("layout probe poisoned") = Some(committed);
    }

    pub fn committed(&self) -> Option<CommittedLayout> {
        self.0.lock().expect("layout probe poisoned").clone()
    }

    pub fn emoji_merging(&self) -> crate::EmojiMerging {
        self.committed()
            .map_or(crate::EmojiMerging::Merge, |committed| {
                committed.emoji_merging
            })
    }
}
