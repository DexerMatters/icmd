//! The one selection implementation.
//!
//! Every consumer of text selection - the editor controls and any selectable
//! region - describes its text as a [`SelectionDocument`] and calls these
//! operations rather than computing a boundary, clamp, word or row step, hit
//! test, or copied slice itself, which keeps both consumers in exact agreement.

mod clipboard;
mod document;
mod probe;

pub use clipboard::{
    clear as clipboard_clear, load as clipboard_load, set_system_writer, store as clipboard_store,
};
pub use document::{
    CaretPoint, DocumentBuilder, Run, Segment, SelectionDocument, block_separator, value_layout,
};
pub use probe::{CommittedSelection, SelectionConfig, SelectionOverlay, SelectionProbe};

use crossterm::event::{KeyCode, KeyModifiers};

use crate::basic::events::KeyboardEvent;
use crate::basic::props::TextStyle;
use crate::basic::text_layout::HitBias;
use crate::theme::Theme;

/// A selection over a document: two document byte offsets plus the preferred
/// cell a vertical motion is holding on to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    /// The fixed end of the selection, in document byte offsets.
    pub anchor: usize,
    /// The active end of the selection, in document byte offsets.
    pub cursor: usize,
    preferred: Option<usize>,
}

impl Selection {
    /// A collapsed selection at one document offset.
    pub const fn at(offset: usize) -> Self {
        Self {
            anchor: offset,
            cursor: offset,
            preferred: None,
        }
    }

    /// A selection with distinct ends, in document offsets; the preferred cell
    /// starts unset exactly as it does for a fresh placement.
    pub const fn between(anchor: usize, cursor: usize) -> Self {
        Self {
            anchor,
            cursor,
            preferred: None,
        }
    }

    /// Whether both ends sit on the same document offset.
    pub const fn is_collapsed(self) -> bool {
        self.anchor == self.cursor
    }

    /// The two ends ordered as `(start, end)`.
    pub fn range(self) -> (usize, usize) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// The preferred cell a vertical motion is holding, if any.
    pub fn preferred_column(self) -> Option<usize> {
        self.preferred
    }

    /// Place the active end from a point, collapsing to it unless `extend`.
    /// `bias` reads the boundary nearest the cell for a pointer press, while an
    /// editor keeps the trailing bias its own pointer path has always used.
    pub fn place(
        &mut self,
        document: &SelectionDocument,
        point: DocPoint,
        bias: HitBias,
        extend: bool,
    ) {
        let offset = document.hit(point, bias);
        self.place_offset(document, offset, extend);
    }

    /// Place the active end at a document offset, collapsing to it unless
    /// `extend`.
    pub fn place_offset(&mut self, document: &SelectionDocument, offset: usize, extend: bool) {
        let offset = document.clamp(offset);
        if extend {
            self.cursor = offset;
        } else {
            self.anchor = offset;
            self.cursor = offset;
        }
        self.preferred = None;
    }

    /// Resolve a pointer drag endpoint from `pressed`, the document offset of
    /// the press. A terminal pointer resolves to a whole cell, so the glyph
    /// under the pointer stays selected on either side of the press - to the
    /// right the pointer contributes its trailing edge, to the left its leading
    /// edge - and a pointer that never leaves the pressed cell selects nothing.
    pub fn drag_to(&mut self, document: &SelectionDocument, point: DocPoint, pressed: usize) {
        let leading = document.hit(point, HitBias::Leading);
        let trailing = document.hit(point, HitBias::Trailing);
        if leading == pressed {
            self.anchor = pressed;
            self.cursor = pressed;
        } else if leading > pressed {
            self.anchor = pressed;
            self.cursor = trailing;
        } else {
            let after = document
                .next_boundary(pressed)
                .min(document.line_end(pressed));
            self.anchor = after.max(pressed);
            self.cursor = leading;
        }
        self.preferred = None;
    }

    /// Collapse both ends onto a clamped document offset.
    pub fn collapse_to(&mut self, document: &SelectionDocument, offset: usize) {
        let offset = document.clamp(offset);
        self.anchor = offset;
        self.cursor = offset;
        self.preferred = None;
    }

    /// Select the whole document.
    pub fn select_all(&mut self, document: &SelectionDocument) {
        self.anchor = 0;
        self.cursor = document.len();
        self.preferred = None;
    }

    /// Set the vertical target directly: the resolved offset, whether to
    /// extend, and the preferred cell to keep. Callers that resolve rows from
    /// the committed layout already know all three.
    pub fn set_vertical(
        &mut self,
        document: &SelectionDocument,
        target: usize,
        extend: bool,
        preferred: Option<usize>,
    ) {
        let target = document.clamp(target);
        if extend {
            self.cursor = target;
        } else {
            self.anchor = target;
            self.cursor = target;
        }
        self.preferred = preferred;
    }

    /// Move `steps` painted rows in `direction` (`-1` up, `1` down), holding the
    /// preferred cell and stopping when the target row resolves to the position
    /// already held.
    pub fn move_vertical(
        &mut self,
        document: &SelectionDocument,
        direction: i32,
        steps: usize,
        extend: bool,
    ) {
        let rows = document.row_count();
        if rows == 0 {
            self.set_vertical(document, self.cursor, extend, self.preferred);
            return;
        }
        let mut target = self.cursor;
        let mut preferred = self.preferred;
        for _ in 0..steps.max(1) {
            let row = document.row_of(target);
            let cell = preferred.unwrap_or_else(|| document.cell_of(target));
            let next_row = (row as i64 + i64::from(direction))
                .clamp(0, rows.saturating_sub(1) as i64) as usize;
            let next = document.hit_row(next_row, cell);
            if next == target {
                break;
            }
            target = next;
            preferred = Some(cell);
        }
        self.set_vertical(document, target, extend, preferred);
    }

    /// Apply a semantic motion. Directional motions collapse an existing
    /// selection before moving; edge and row motions resolve from the active
    /// end. `extend` keeps the anchor fixed.
    pub fn apply(&mut self, document: &SelectionDocument, motion: SelectionMotion, extend: bool) {
        match motion {
            SelectionMotion::CharLeft => {
                if !extend && !self.is_collapsed() {
                    self.collapse_to(document, self.range().0);
                } else {
                    let target = document.previous_boundary(self.cursor);
                    self.place_offset(document, target, extend);
                }
            }
            SelectionMotion::CharRight => {
                if !extend && !self.is_collapsed() {
                    self.collapse_to(document, self.range().1);
                } else {
                    let target = document.next_boundary(self.cursor);
                    self.place_offset(document, target, extend);
                }
            }
            SelectionMotion::WordLeft => {
                if !extend && !self.is_collapsed() {
                    self.collapse_to(document, self.range().0);
                } else {
                    let target = document.word_before(self.cursor);
                    self.place_offset(document, target, extend);
                }
            }
            SelectionMotion::WordRight => {
                if !extend && !self.is_collapsed() {
                    self.collapse_to(document, self.range().1);
                } else {
                    let target = document.word_after(self.cursor);
                    self.place_offset(document, target, extend);
                }
            }
            SelectionMotion::RowStart => {
                let target = document.line_start(self.cursor);
                self.place_offset(document, target, extend);
            }
            SelectionMotion::RowEnd => {
                let target = document.line_end(self.cursor);
                self.place_offset(document, target, extend);
            }
            SelectionMotion::DocStart => self.place_offset(document, 0, extend),
            SelectionMotion::DocEnd => {
                let end = document.len();
                self.place_offset(document, end, extend);
            }
            SelectionMotion::RowUp => self.move_vertical(document, -1, 1, extend),
            SelectionMotion::RowDown => self.move_vertical(document, 1, 1, extend),
            SelectionMotion::PageUp => {
                let steps = document.viewport_height();
                self.move_vertical(document, -1, steps, extend);
            }
            SelectionMotion::PageDown => {
                let steps = document.viewport_height();
                self.move_vertical(document, 1, steps, extend);
            }
        }
    }

    /// Snap both ends into the document, reporting whether anything moved. The
    /// preferred cell is preserved, since a re-render is not a motion.
    pub fn clamp(&mut self, document: &SelectionDocument) -> bool {
        let before = (self.anchor, self.cursor);
        self.anchor = document.clamp(self.anchor);
        self.cursor = document.clamp(self.cursor);
        (self.anchor, self.cursor) != before
    }

    /// The selected text, or `None` when the selection is collapsed.
    pub fn text(&self, document: &SelectionDocument) -> Option<String> {
        if self.is_collapsed() {
            return None;
        }
        let (start, end) = self.range();
        Some(document.slice(start..end))
    }

    /// The selected part of one segment, in that segment's own byte
    /// coordinates; `doc_start` is the segment's first document byte and
    /// `value_len` its byte length. This is the single projection every painter
    /// uses, and it is `None` when the selection misses the segment.
    pub fn local_range(
        &self,
        doc_start: usize,
        value_len: usize,
    ) -> Option<std::ops::Range<usize>> {
        let (start, end) = self.range();
        let from = start.max(doc_start);
        let to = end.min(doc_start.saturating_add(value_len));
        if from < to {
            Some(from - doc_start..to - doc_start)
        } else {
            None
        }
    }
}

/// A position to resolve against a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocPoint {
    /// An already-known document byte offset.
    Offset(usize),
    /// A point in the segment coordinate space.
    Point {
        /// The painted line.
        line: i32,
        /// The painted column.
        column: i32,
    },
}

/// A selection motion, independent of which key produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMotion {
    /// One grapheme toward the document start.
    CharLeft,
    /// One grapheme toward the document end.
    CharRight,
    /// One word toward the document start.
    WordLeft,
    /// One word toward the document end.
    WordRight,
    /// One painted row up.
    RowUp,
    /// One painted row down.
    RowDown,
    /// The start of the logical line.
    RowStart,
    /// The end of the logical line.
    RowEnd,
    /// The document start.
    DocStart,
    /// The document end.
    DocEnd,
    /// One viewport height up.
    PageUp,
    /// One viewport height down.
    PageDown,
}

/// The motion a key event implies: Left/Right plus Control give a word step,
/// Home/End plus Control give a document edge, and others are `None`.
pub fn motion_for(event: &KeyboardEvent) -> Option<SelectionMotion> {
    let modifiers = event.key.modifiers;
    let control = modifiers.contains(KeyModifiers::CONTROL);
    match event.key.code {
        KeyCode::Left => Some(if control {
            SelectionMotion::WordLeft
        } else {
            SelectionMotion::CharLeft
        }),
        KeyCode::Right => Some(if control {
            SelectionMotion::WordRight
        } else {
            SelectionMotion::CharRight
        }),
        KeyCode::Up => Some(SelectionMotion::RowUp),
        KeyCode::Down => Some(SelectionMotion::RowDown),
        KeyCode::Home => Some(if control {
            SelectionMotion::DocStart
        } else {
            SelectionMotion::RowStart
        }),
        KeyCode::End => Some(if control {
            SelectionMotion::DocEnd
        } else {
            SelectionMotion::RowEnd
        }),
        KeyCode::PageUp => Some(SelectionMotion::PageUp),
        KeyCode::PageDown => Some(SelectionMotion::PageDown),
        _ => None,
    }
}

/// Whether a key event extends the selection rather than collapsing it.
pub fn extends_selection(event: &KeyboardEvent) -> bool {
    event.key.modifiers.contains(KeyModifiers::SHIFT)
}

/// A clipboard command implied by a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardIntent {
    /// Copy the selection.
    Copy,
    /// Copy the selection and clear it.
    Cut,
    /// Paste the clipboard buffer.
    Paste,
    /// Select the whole document.
    SelectAll,
}

/// The clipboard or select-all command a key event implies, if any.
///
/// Every chord is Shift-qualified - Shift+Ctrl+C/X/V - because Ctrl+C is the
/// runtime's exit key and Ctrl+V is literal-next in several shells, so a widget
/// that consumed the plain chords would quit the application or break the
/// terminal's own editing; plain Ctrl+C is deliberately left unclaimed. An
/// uppercase chord letter also matches, since terminals disagree on whether
/// they set the SHIFT modifier for Ctrl+Shift+C.
pub fn clipboard_intent(event: &KeyboardEvent) -> Option<ClipboardIntent> {
    let modifiers = event.key.modifiers;
    if !modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    match event.key.code {
        KeyCode::Char('c') if shift => Some(ClipboardIntent::Copy),
        KeyCode::Char('C') => Some(ClipboardIntent::Copy),
        KeyCode::Char('x') if shift => Some(ClipboardIntent::Cut),
        KeyCode::Char('X') => Some(ClipboardIntent::Cut),
        KeyCode::Char('v') if shift => Some(ClipboardIntent::Paste),
        KeyCode::Char('V') => Some(ClipboardIntent::Paste),
        KeyCode::Char('a' | 'A') => Some(ClipboardIntent::SelectAll),
        _ => None,
    }
}

/// The selection colours, in one place so an editor and a selectable region
/// cannot drift apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionStyles {
    /// Style for a focused selection.
    pub active: TextStyle,
    /// Style for an unfocused selection.
    pub inactive: TextStyle,
}

impl Default for SelectionStyles {
    fn default() -> Self {
        Self {
            active: TextStyle::default().reverse(),
            inactive: TextStyle::default().dim(),
        }
    }
}

impl SelectionStyles {
    /// The selection styles a theme defines: primary colours when focused,
    /// muted colours when not.
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            active: TextStyle::default()
                .foreground(theme.colors.primary_foreground)
                .background(theme.colors.primary),
            inactive: TextStyle::default()
                .foreground(theme.colors.muted_foreground)
                .background(theme.colors.muted),
        }
    }
}
