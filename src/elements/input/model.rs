//! Text editing model: value reconciliation, caret motion, and edit reduction.

use crate::basic::selection::{DocPoint, SelectionDocument, SelectionMotion};
use crate::runtime::limits::ResourceLimits;
use crate::{EmojiMerging, data::display_units};

/// The selection interval is shared with every other selectable surface: the
/// editor contributes its own editing semantics around it, but the interval
/// itself, its boundaries, and every motion are the selection engine's.
pub use crate::basic::selection::Selection as Caret;

/// Whether the owner or the control owns the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueOwnership {
    /// The control owns its value after initialization.
    #[default]
    Uncontrolled,
    /// The owner supplies the value on every render.
    Controlled,
}

/// An optimistic value emitted by a controlled edit and not yet acknowledged by
/// the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedDraft {
    /// The emitted value.
    pub value: String,
    /// The selection that produced the emitted value.
    pub caret: Caret,
}

/// Whether a render changed the model's observable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Something observable changed.
    Changed,
    /// Nothing observable changed.
    Unchanged,
}

/// Editor commands the reducer understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditAction {
    /// Insert the given text at the selection.
    Insert(String),
    /// Insert a newline.
    InsertNewline,
    /// Paste the given text at the selection.
    Paste(String),
    /// Cut the selection.
    Cut,
    /// Delete backwards, by a word when `word` is set.
    Backspace {
        /// Whether the deletion extends to a word boundary.
        word: bool,
    },
    /// Delete forwards, by a word when `word` is set.
    Delete {
        /// Whether the deletion extends to a word boundary.
        word: bool,
    },
    /// Move the caret left, by a word when `word` is set.
    MoveLeft {
        /// Whether to extend the selection instead of collapsing it.
        extend: bool,
        /// Whether to move by a word instead of one display unit.
        word: bool,
    },
    /// Move the caret right, by a word when `word` is set.
    MoveRight {
        /// Whether to extend the selection instead of collapsing it.
        extend: bool,
        /// Whether to move by a word instead of one display unit.
        word: bool,
    },
    /// Move the caret up one rendered row.
    MoveUp {
        /// Whether to extend the selection instead of collapsing it.
        extend: bool,
    },
    /// Move the caret down one rendered row.
    MoveDown {
        /// Whether to extend the selection instead of collapsing it.
        extend: bool,
    },
    /// Move the caret up by `rows` rendered rows.
    PageUp {
        /// Whether to extend the selection instead of collapsing it.
        extend: bool,
        /// Number of rendered rows to move.
        rows: usize,
    },
    /// Move the caret down by `rows` rendered rows.
    PageDown {
        /// Whether to extend the selection instead of collapsing it.
        extend: bool,
        /// Number of rendered rows to move.
        rows: usize,
    },
    /// Move to the row start, or the document start when `document` is set.
    Home {
        /// Whether to extend the selection instead of collapsing it.
        extend: bool,
        /// Whether to target the document start rather than the row start.
        document: bool,
    },
    /// Move to the row end, or the document end when `document` is set.
    End {
        /// Whether to extend the selection instead of collapsing it.
        extend: bool,
        /// Whether to target the document end rather than the row end.
        document: bool,
    },
    /// Select the whole value.
    SelectAll,
    /// Place the caret at a document offset.
    PlaceCaret {
        /// Document offset to place the caret at.
        offset: usize,
        /// Whether to extend the selection instead of collapsing it.
        extend: bool,
    },
}

/// Command intent carried with an edit outcome.
///
/// It is explicit so hosts can tell a genuine Cut from an ordinary deletion;
/// only `Cut` may cross the public clipboard boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditIntent {
    /// No specific intent.
    #[default]
    None,
    /// Text was inserted.
    Insert,
    /// Text was pasted.
    Paste,
    /// Text before the caret was deleted.
    DeleteBackward,
    /// Text after the caret was deleted.
    DeleteForward,
    /// Selected text was removed for the clipboard.
    Cut,
}

/// Result of reducing one [`EditAction`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditOutcome {
    /// Whether the action was consumed by the editor.
    pub handled: bool,
    /// Whether the value changed.
    pub changed: bool,
    /// The new value when it changed.
    pub value: Option<String>,
    /// Text removed by a deletion or cut.
    pub removed_text: Option<String>,
    /// Command intent behind the edit.
    pub intent: EditIntent,
    /// Whether the caret should be scrolled into view.
    pub reveal_caret: bool,
}

impl EditOutcome {
    /// An outcome that consumed the action without changing anything.
    fn handled() -> Self {
        Self {
            handled: true,
            ..Self::default()
        }
    }
}

/// Policy constraints applied to every edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditPolicy {
    /// Whether newlines may be inserted.
    pub multiline: bool,
    /// Maximum value length in display units; unbounded when `None`.
    pub max_length: Option<usize>,
    /// Whether edits are refused while selection still works.
    pub read_only: bool,
    /// Whether the control is disabled entirely.
    pub disabled: bool,
    /// Emoji-merging mode used as the unit measure.
    pub emoji_merging: EmojiMerging,
}

/// Canonicalize line endings (`\r\n` and `\r` to `\n`) and apply the control
/// policy in one pass.
///
/// In single-line mode newlines and tabs become spaces; other control
/// characters are dropped.
pub fn normalize(value: &str, multiline: bool) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if multiline {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
            '\n' => out.push(if multiline { '\n' } else { ' ' }),
            '\t' => out.push(if multiline { '\t' } else { ' ' }),
            other => {
                if !other.is_control() {
                    out.push(other);
                }
            }
        }
    }
    out
}

/// Test-only access to the editor's normalization policy so integration tests
/// can compare the optimized pass against a reference model.
#[doc(hidden)]
pub fn normalize_for_test(value: &str, multiline: bool) -> String {
    normalize(value, multiline)
}

/// Editing model: the value, the caret, and reconciliation with a controlled
/// owner.
#[derive(Debug, Clone, Default)]
pub struct EditModel {
    /// The current value, canonicalized.
    pub value: String,
    /// The current selection interval.
    pub caret: Caret,
    /// Whether the owner or the control owns the value.
    ownership: ValueOwnership,
    /// The last value the owner rendered.
    authoritative: String,
    /// Whether the first render has initialized the model.
    initialized: bool,
    /// The controlled draft awaiting the owner's echo.
    draft: Option<EmittedDraft>,
    /// Every value emitted since the current draft opened, oldest first. An
    /// asynchronous owner can answer with an earlier link of this chain while
    /// it is still catching up; that is not a rejection, and the model keeps
    /// its optimistic value and caret until the owner republishes the latest
    /// one.
    pending_emissions: Vec<String>,
    /// Emoji-merging mode used as the unit measure.
    emoji_merging: EmojiMerging,
    /// The document every selection operation is expressed against. It is kept
    /// in step with `value` and `emoji_merging` by the setters below, so the
    /// read-only accessors never need to rebuild it.
    document: SelectionDocument,
    /// The merging mode `document` was built with.
    document_merging: EmojiMerging,
}

/// The owner can be several renders behind a fast typist, so the chain keeps
/// enough links to cover a long burst before it degrades to the owner's value.
/// It is bounded, because a runaway chain must not retain memory.
const MAX_PENDING_EMISSIONS: usize = 64;

impl EditModel {
    /// Current value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Current selection interval.
    pub fn caret(&self) -> Caret {
        self.caret
    }

    /// Whether the value is controlled or uncontrolled.
    #[allow(dead_code)]
    pub fn ownership(&self) -> ValueOwnership {
        self.ownership
    }

    /// The value-derived document. `NoWrap` at a width no terminal reaches
    /// keeps every logical line in one row, so its boundary table is exactly
    /// the value's grapheme edges: horizontal motion and word steps are
    /// independent of how the committed frame happens to wrap.
    fn document(&self) -> &SelectionDocument {
        &self.document
    }

    /// Rebuild the document from the current value and merging mode.
    fn refresh_document(&mut self) {
        self.document = SelectionDocument::from_value(self.value.clone(), self.emoji_merging);
        self.document_merging = self.emoji_merging;
    }

    /// Assign the value and keep the document in step. Every value mutation
    /// goes through here so no accessor can observe a stale boundary table.
    fn set_value(&mut self, value: String) {
        self.value = value;
        self.refresh_document();
    }

    /// Refresh the document after an in-place value mutation.
    fn mark_value_changed(&mut self) {
        self.refresh_document();
    }

    /// Grapheme-edge offsets of the current value.
    pub fn boundaries(&self) -> Vec<usize> {
        self.document().boundaries().to_vec()
    }

    /// Snap an offset to the nearest display-unit boundary.
    pub fn clamp(&self, offset: usize) -> usize {
        self.document().clamp(offset)
    }

    /// Reconcile the owner's value with the model and adopt the layout's
    /// emoji-merging mode; returns whether anything observable changed.
    ///
    /// Acceptance is exactly "the owner republished what we emitted", and then
    /// the selection that produced the draft is kept. An owner that is still
    /// catching up answers with an earlier link of the emitted chain: that is
    /// not a rejection, so the optimistic value, caret, and draft are kept
    /// until the owner republishes the latest emission. Any other value is a
    /// rejection or an external replacement, and the live selection is clamped
    /// instead. Switching to uncontrolled retains the last authoritative
    /// rendered value rather than a speculative draft.
    pub fn render(
        &mut self,
        value: Option<&str>,
        default_value: Option<&str>,
        multiline: bool,
        emoji_merging: EmojiMerging,
    ) -> Outcome {
        let mode_changed = self.emoji_merging != emoji_merging;
        self.emoji_merging = emoji_merging;
        let controlled = value.map(|value| normalize(value, multiline));
        if !self.initialized {
            self.initialized = true;
            let initial = controlled
                .clone()
                .unwrap_or_else(|| normalize(default_value.unwrap_or_default(), multiline));
            self.caret = Caret::at(initial.len());
            self.authoritative = initial.clone();
            self.ownership = if controlled.is_some() {
                ValueOwnership::Controlled
            } else {
                ValueOwnership::Uncontrolled
            };
            self.draft = None;
            self.pending_emissions.clear();
            self.set_value(initial);
            return Outcome::Changed;
        }

        let previous = self.value.clone();
        let previous_caret = self.caret;
        let previous_ownership = self.ownership;

        match controlled {
            Some(controlled) => {
                let draft = self.draft.take();
                let accepted = draft
                    .as_ref()
                    .is_some_and(|draft| draft.value == controlled);
                let catching_up = !accepted
                    && draft.is_some()
                    && self
                        .pending_emissions
                        .iter()
                        .any(|emitted| emitted.as_str() == controlled.as_str());
                if catching_up {
                    self.draft = draft;
                } else {
                    let caret = match (accepted, draft) {
                        (true, Some(draft)) => draft.caret,
                        _ => self.caret,
                    };
                    self.set_value(controlled);
                    self.caret = self.snap_caret(caret);
                    self.authoritative = self.value.clone();
                    self.pending_emissions.clear();
                }
                self.ownership = ValueOwnership::Controlled;
            }
            None => {
                if self.draft.is_some() {
                    self.set_value(self.authoritative.clone());
                }
                self.ownership = ValueOwnership::Uncontrolled;
                self.draft = None;
                self.pending_emissions.clear();
                self.authoritative = self.value.clone();
                self.caret = self.snap_caret(self.caret);
            }
        }

        if self.value == previous
            && self.caret == previous_caret
            && self.ownership == previous_ownership
            && !mode_changed
        {
            Outcome::Unchanged
        } else {
            Outcome::Changed
        }
    }

    /// Clamp a caret into the current document.
    fn snap_caret(&self, caret: Caret) -> Caret {
        let mut caret = caret;
        caret.clamp(self.document());
        caret
    }

    /// Apply one edit action under `policy` and return the outcome.
    ///
    /// Edits use the units the committed layout painted, read from the layout
    /// probe, so an event that arrives before the next render still edits the
    /// cells the user can see. Unacknowledged controlled drafts are continued
    /// rather than replaced by the authoritative echo, so rapid events
    /// accumulate.
    pub fn reduce(&mut self, action: EditAction, policy: EditPolicy) -> EditOutcome {
        if policy.disabled {
            return EditOutcome::default();
        }
        if self.emoji_merging != policy.emoji_merging {
            self.emoji_merging = policy.emoji_merging;
            self.refresh_document();
        }
        self.resume_draft();
        let mut outcome = self.reduce_inner(action, policy);
        if outcome.handled && !policy.read_only && outcome.value.is_some() {
            self.open_draft();
        }
        if outcome.handled {
            outcome.reveal_caret = true;
        }
        outcome
    }

    /// Continue an unacknowledged controlled draft rather than the
    /// authoritative echo, so rapid events accumulate.
    fn resume_draft(&mut self) {
        if let Some(draft) = &self.draft {
            let value = draft.value.clone();
            self.caret = draft.caret;
            self.set_value(value);
        }
    }

    /// Dispatch one action.
    ///
    /// Every navigation action is the selection engine's motion applied to the
    /// model's own document; the editor contributes only the action vocabulary
    /// and the read-only policy. Vertical movement needs the rendered row table,
    /// so it is reported as handled here and resolved by the caller through
    /// `vertical_move`; reporting it as unhandled would let the key bubble while
    /// the caret silently stayed put.
    fn reduce_inner(&mut self, action: EditAction, policy: EditPolicy) -> EditOutcome {
        let editable = !policy.read_only;
        match action {
            EditAction::Insert(text) => {
                if !editable {
                    return EditOutcome::handled();
                }
                self.insert(&text, policy)
            }
            EditAction::InsertNewline => {
                if !policy.multiline || !editable {
                    return EditOutcome::handled();
                }
                self.insert("\n", policy)
            }
            EditAction::Paste(text) => {
                if !editable {
                    return EditOutcome::handled();
                }
                let mut outcome = self.insert(&text, policy);
                if outcome.changed {
                    outcome.intent = EditIntent::Paste;
                }
                outcome
            }
            EditAction::Cut => {
                if !editable {
                    return EditOutcome::handled();
                }
                let (start, end) = self.caret.range();
                if start == end {
                    return EditOutcome::handled();
                }
                self.delete_range(start, end, EditIntent::Cut)
            }
            EditAction::Backspace { word } => {
                if !editable {
                    return EditOutcome::handled();
                }
                let (start, end) = self.caret.range();
                let (start, end) = if start != end {
                    (start, end)
                } else {
                    let target = if word {
                        self.document().word_before(self.caret.cursor)
                    } else {
                        self.document().previous_boundary(self.caret.cursor)
                    };
                    (target, self.caret.cursor)
                };
                self.delete_range(start, end, EditIntent::DeleteBackward)
            }
            EditAction::Delete { word } => {
                if !editable {
                    return EditOutcome::handled();
                }
                let (start, end) = self.caret.range();
                let (start, end) = if start != end {
                    (start, end)
                } else {
                    let target = if word {
                        self.document().word_after(self.caret.cursor)
                    } else {
                        self.document().next_boundary(self.caret.cursor)
                    };
                    (self.caret.cursor, target)
                };
                self.delete_range(start, end, EditIntent::DeleteForward)
            }
            EditAction::MoveLeft { extend, word } => {
                let motion = if word {
                    SelectionMotion::WordLeft
                } else {
                    SelectionMotion::CharLeft
                };
                self.caret.apply(&self.document, motion, extend);
                EditOutcome::handled()
            }
            EditAction::MoveRight { extend, word } => {
                let motion = if word {
                    SelectionMotion::WordRight
                } else {
                    SelectionMotion::CharRight
                };
                self.caret.apply(&self.document, motion, extend);
                EditOutcome::handled()
            }
            EditAction::Home { extend, document } => {
                let motion = if document {
                    SelectionMotion::DocStart
                } else {
                    SelectionMotion::RowStart
                };
                self.caret.apply(&self.document, motion, extend);
                EditOutcome::handled()
            }
            EditAction::End { extend, document } => {
                let motion = if document {
                    SelectionMotion::DocEnd
                } else {
                    SelectionMotion::RowEnd
                };
                self.caret.apply(&self.document, motion, extend);
                EditOutcome::handled()
            }
            EditAction::SelectAll => {
                self.caret.select_all(&self.document);
                EditOutcome::handled()
            }
            EditAction::PlaceCaret { offset, extend } => {
                self.caret.place_offset(&self.document, offset, extend);
                EditOutcome::handled()
            }
            EditAction::MoveUp { .. }
            | EditAction::MoveDown { .. }
            | EditAction::PageUp { .. }
            | EditAction::PageDown { .. } => EditOutcome::handled(),
        }
    }

    /// Move the caret to a caller-resolved target offset, extending when
    /// `extend` is set and remembering the preferred column.
    pub fn move_vertical(&mut self, target: usize, extend: bool, preferred: Option<usize>) {
        self.caret
            .set_vertical(&self.document, target, extend, preferred);
    }

    /// The remembered preferred column for vertical motion, if any.
    #[allow(dead_code)]
    pub fn preferred_column(&self) -> Option<usize> {
        self.caret.preferred_column()
    }

    /// Apply a single- or multi-step vertical motion against the rendered
    /// document.
    pub fn vertical_move(
        &mut self,
        document: &SelectionDocument,
        direction: i32,
        steps: usize,
        extend: bool,
        policy: EditPolicy,
    ) -> EditOutcome {
        if policy.disabled || !policy.multiline {
            return EditOutcome::default();
        }
        self.caret.move_vertical(document, direction, steps, extend);
        EditOutcome {
            handled: true,
            reveal_caret: true,
            ..EditOutcome::default()
        }
    }

    /// Insert normalized text at the selection, honoring the shared input-size
    /// budget and `max_length`.
    ///
    /// A paste or keystroke that would push the value past the shared ceiling is
    /// refused rather than accepted in part, so the editor can never grow
    /// without bound. Returns a changed outcome, or a handled no-op when the
    /// insertion is empty or refused.
    fn insert(&mut self, inserted: &str, policy: EditPolicy) -> EditOutcome {
        let inserted = normalize(inserted, policy.multiline);
        if inserted.is_empty() {
            return EditOutcome::handled();
        }
        if ResourceLimits::default()
            .check_input_bytes(self.value.len().saturating_add(inserted.len()))
            .is_err()
        {
            return EditOutcome::handled();
        }
        let (start, end) = self.caret.range();
        let before = &self.value[..start];
        let after = &self.value[end..];
        let inserted = match policy.max_length {
            Some(limit) => constrain_insertion(before, &inserted, after, limit, self.emoji_merging),
            None => inserted,
        };
        if inserted.is_empty() {
            return EditOutcome::handled();
        }
        let mut next = String::with_capacity(before.len() + inserted.len() + after.len());
        next.push_str(before);
        next.push_str(&inserted);
        next.push_str(after);
        let cursor = start + inserted.len();
        self.set_value(next);
        self.caret = Caret::at(self.clamp(cursor));
        EditOutcome {
            handled: true,
            changed: true,
            value: Some(self.value.clone()),
            intent: EditIntent::Insert,
            ..EditOutcome::default()
        }
    }

    /// Remove `start..end` and report the removed text and intent.
    fn delete_range(&mut self, start: usize, end: usize, intent: EditIntent) -> EditOutcome {
        if start >= end {
            return EditOutcome::handled();
        }
        let removed = self.value[start..end].to_string();
        self.value.replace_range(start..end, "");
        self.mark_value_changed();
        self.caret = Caret::at(self.clamp(start));
        EditOutcome {
            handled: true,
            changed: true,
            value: Some(self.value.clone()),
            removed_text: Some(removed),
            intent,
            ..EditOutcome::default()
        }
    }

    /// Boundary immediately before `offset`.
    pub fn previous_boundary(&self, offset: usize) -> usize {
        self.document().previous_boundary(offset)
    }

    /// The selected text, sliced by the selection engine.
    ///
    /// This is the editor's clipboard source, and the same call a selectable
    /// region makes.
    pub fn selected_text(&self) -> Option<String> {
        self.caret.text(self.document())
    }

    /// Extend the selection to a pointer position.
    ///
    /// The engine's one drag rule applies: the glyph under the pointer stays
    /// selected on either side of the press, and `pressed` is the offset the
    /// press landed on. The editor and a selectable region therefore resolve a
    /// drag identically.
    pub fn drag_selection(
        &mut self,
        document: &SelectionDocument,
        point: DocPoint,
        pressed: usize,
    ) {
        self.caret.drag_to(document, point, pressed);
    }

    /// Boundary immediately after `offset`.
    pub fn next_boundary(&self, offset: usize) -> usize {
        self.document().next_boundary(offset)
    }

    /// Open a controlled draft for the latest emission.
    ///
    /// Extending an open draft replaces it with the latest emission: the draft
    /// bridges every event until the owner's next render answers it. The chain
    /// also records the link, so an owner that answers with an earlier emission
    /// is recognized as catching up rather than rejecting.
    fn open_draft(&mut self) {
        if self.ownership != ValueOwnership::Controlled {
            return;
        }
        self.draft = Some(EmittedDraft {
            value: self.value.clone(),
            caret: self.caret,
        });
        if self.pending_emissions.last() != Some(&self.value) {
            if self.pending_emissions.len() >= MAX_PENDING_EMISSIONS {
                self.pending_emissions.remove(0);
            }
            self.pending_emissions.push(self.value.clone());
        }
    }
}

/// Trim an insertion so the resulting value fits `limit` display units.
///
/// Candidate ends are the display-unit boundaries of the insertion; joining the
/// first and last inserted units to their neighbors can recover at most two
/// independently counted units.
fn constrain_insertion(
    before: &str,
    inserted: &str,
    after: &str,
    limit: usize,
    merging: EmojiMerging,
) -> String {
    let with = |inserted: &str| {
        let mut value = String::with_capacity(before.len() + inserted.len() + after.len());
        value.push_str(before);
        value.push_str(inserted);
        value.push_str(after);
        value
    };
    let count = |value: &str| display_units(value, merging).len();
    if count(&with(inserted)) <= limit {
        return inserted.to_owned();
    }
    let base_count = count(&with(""));
    let available = limit.saturating_sub(base_count);
    let mut candidate_ends = vec![0usize];
    candidate_ends.extend(
        display_units(inserted, merging)
            .into_iter()
            .map(|unit| unit.end)
            .take(available.saturating_add(2)),
    );
    let mut accepted_end = 0usize;
    let first_candidate = available
        .saturating_sub(2)
        .min(candidate_ends.len().saturating_sub(1));
    for &end in &candidate_ends[first_candidate..] {
        if count(&with(&inserted[..end])) <= limit {
            accepted_end = end;
        }
    }
    inserted[..accepted_end].to_owned()
}
