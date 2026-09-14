use crate::runtime::limits::ResourceLimits;
use crate::{EmojiMerging, data::display_units};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ValueOwnership {
    #[default]
    Uncontrolled,
    Controlled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Caret {
    pub anchor: usize,
    pub cursor: usize,
}

impl Caret {
    pub(crate) const fn at(offset: usize) -> Self {
        Self {
            anchor: offset,
            cursor: offset,
        }
    }

    pub(crate) const fn is_collapsed(self) -> bool {
        self.anchor == self.cursor
    }

    pub(crate) fn range(self) -> (usize, usize) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmittedDraft {
    pub(crate) value: String,
    pub(crate) caret: Caret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Changed,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditAction {
    Insert(String),
    InsertNewline,
    Paste(String),
    Cut,
    Backspace { word: bool },
    Delete { word: bool },
    MoveLeft { extend: bool, word: bool },
    MoveRight { extend: bool, word: bool },
    MoveUp { extend: bool },
    MoveDown { extend: bool },
    PageUp { extend: bool, rows: usize },
    PageDown { extend: bool, rows: usize },
    Home { extend: bool, document: bool },
    End { extend: bool, document: bool },
    SelectAll,
    PlaceCaret { offset: usize, extend: bool },
}

// Command intent is carried explicitly so hosts can tell a genuine Cut from an
// ordinary deletion. Only `Cut` may cross the public clipboard boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EditIntent {
    #[default]
    None,
    Insert,
    Paste,
    DeleteBackward,
    DeleteForward,
    Cut,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct EditOutcome {
    pub(crate) handled: bool,
    pub(crate) changed: bool,
    pub(crate) value: Option<String>,
    pub(crate) removed_text: Option<String>,
    pub(crate) intent: EditIntent,
    pub(crate) reveal_caret: bool,
}

impl EditOutcome {
    fn handled() -> Self {
        Self {
            handled: true,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct EditPolicy {
    pub(crate) multiline: bool,
    pub(crate) max_length: Option<usize>,
    pub(crate) read_only: bool,
    pub(crate) disabled: bool,
    pub(crate) emoji_merging: EmojiMerging,
}

// One pass that canonicalizes line endings (`\r\n` and `\r` to `\n`) and
// applies the control policy. The previous form allocated an intermediate
// string per `replace` before filtering, so a pasted document was copied three
// times.
pub(crate) fn normalize(value: &str, multiline: bool) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                // Fold `\r\n` into a single newline.
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

// Test-only access to the editor's normalization policy so integration tests
// can compare the optimized pass against a reference model.
#[doc(hidden)]
pub fn normalize_for_test(value: &str, multiline: bool) -> String {
    normalize(value, multiline)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EditModel {
    pub(crate) value: String,
    pub(crate) caret: Caret,
    preferred_column: Option<usize>,
    ownership: ValueOwnership,
    authoritative: String,
    initialized: bool,
    draft: Option<EmittedDraft>,
    emoji_merging: EmojiMerging,
}

impl EditModel {
    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    pub(crate) fn caret(&self) -> Caret {
        self.caret
    }

    #[allow(dead_code)] // Ownership inspection is part of the model's contract.
    pub(crate) fn ownership(&self) -> ValueOwnership {
        self.ownership
    }

    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.value.len()
    }

    #[allow(dead_code)]
    pub(crate) fn boundaries(&self) -> Vec<usize> {
        let mut boundaries = vec![0usize];
        boundaries.extend(
            display_units(&self.value, self.emoji_merging)
                .into_iter()
                .map(|unit| unit.end),
        );
        boundaries
    }

    fn units(&self) -> Vec<std::ops::Range<usize>> {
        display_units(&self.value, self.emoji_merging)
    }

    pub(crate) fn clamp(&self, offset: usize) -> usize {
        let offset = offset.min(self.value.len());
        if offset == 0 || offset == self.value.len() {
            return offset;
        }
        let mut previous = 0usize;
        for unit in self.units() {
            if offset >= unit.end {
                previous = unit.end;
                continue;
            }
            // `offset` lies inside this unit; pick the nearer boundary.
            return if offset - unit.start <= unit.end - offset {
                unit.start
            } else {
                unit.end
            };
        }
        previous
    }

    pub(crate) fn render(
        &mut self,
        value: Option<&str>,
        default_value: Option<&str>,
        multiline: bool,
        emoji_merging: EmojiMerging,
    ) -> Outcome {
        // The layout that paints this render is the authority on which
        // codepoints are separate cells, so the model adopts its mode before
        // snapping any selection into the value.
        let mode_changed = self.emoji_merging != emoji_merging;
        self.emoji_merging = emoji_merging;
        let controlled = value.map(|value| normalize(value, multiline));
        if !self.initialized {
            self.initialized = true;
            self.value = controlled
                .clone()
                .unwrap_or_else(|| normalize(default_value.unwrap_or_default(), multiline));
            self.caret = Caret::at(self.value.len());
            self.authoritative = self.value.clone();
            self.ownership = if controlled.is_some() {
                ValueOwnership::Controlled
            } else {
                ValueOwnership::Uncontrolled
            };
            self.draft = None;
            return Outcome::Changed;
        }

        let previous = self.value.clone();
        let previous_caret = self.caret;
        let previous_ownership = self.ownership;

        match controlled {
            Some(controlled) => {
                // The owner is authoritative at every render. A draft exists
                // only for edits made since the render now being superseded, so
                // this render is always the owner's answer to it: acceptance is
                // exactly "the owner republished what we emitted", and every
                // other value is rejection or external replacement. No causality
                // heuristic over historical strings participates.
                let draft = self.draft.take();
                if let Some(draft) = draft {
                    if draft.value == controlled {
                        // Acceptance: keep the selection that produced it.
                        self.value = controlled;
                        self.caret = self.snap_caret(draft.caret);
                    } else {
                        // Rejection or external replacement: the owner's value
                        // wins and the optimistic draft is discarded.
                        self.value = controlled;
                        self.caret = self.snap_caret(self.caret);
                    }
                } else {
                    self.value = controlled;
                    self.caret = self.snap_caret(self.caret);
                }
                self.authoritative = self.value.clone();
                self.ownership = ValueOwnership::Controlled;
            }
            None => {
                // Switching to uncontrolled retains the last authoritative
                // rendered value - not a speculative draft the owner never
                // accepted - and then resumes local ownership.
                if self.draft.is_some() {
                    self.value = self.authoritative.clone();
                }
                self.ownership = ValueOwnership::Uncontrolled;
                self.draft = None;
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

    fn snap_caret(&self, caret: Caret) -> Caret {
        Caret {
            anchor: self.clamp(caret.anchor),
            cursor: self.clamp(caret.cursor),
        }
    }

    pub(crate) fn reduce(&mut self, action: EditAction, policy: EditPolicy) -> EditOutcome {
        if policy.disabled {
            return EditOutcome::default();
        }
        // Edit on the units the committed layout painted. The component reads
        // this from the layout probe, so an event that arrives before the next
        // render still edits the cells the user can see.
        self.emoji_merging = policy.emoji_merging;
        // Continue an unacknowledged controlled draft rather than the
        // authoritative echo, so rapid events accumulate.
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

    fn resume_draft(&mut self) {
        if let Some(draft) = &self.draft {
            self.value = draft.value.clone();
            self.caret = draft.caret;
        }
    }

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
                        word_left(&self.value, self.caret.cursor, self.emoji_merging)
                    } else {
                        self.previous_boundary(self.caret.cursor)
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
                        word_right(&self.value, self.caret.cursor, self.emoji_merging)
                    } else {
                        self.next_boundary(self.caret.cursor)
                    };
                    (self.caret.cursor, target)
                };
                self.delete_range(start, end, EditIntent::DeleteForward)
            }
            EditAction::MoveLeft { extend, word } => {
                let (start, end) = self.caret.range();
                let target = if !extend && start != end {
                    start
                } else if word {
                    word_left(&self.value, self.caret.cursor, self.emoji_merging)
                } else {
                    self.previous_boundary(self.caret.cursor)
                };
                self.move_to(target, extend);
                EditOutcome::handled()
            }
            EditAction::MoveRight { extend, word } => {
                let (start, end) = self.caret.range();
                let target = if !extend && start != end {
                    end
                } else if word {
                    word_right(&self.value, self.caret.cursor, self.emoji_merging)
                } else {
                    self.next_boundary(self.caret.cursor)
                };
                self.move_to(target, extend);
                EditOutcome::handled()
            }
            EditAction::Home { extend, document } => {
                let target = if document {
                    0
                } else {
                    self.line_start(self.caret.cursor)
                };
                self.move_to(target, extend);
                EditOutcome::handled()
            }
            EditAction::End { extend, document } => {
                let target = if document {
                    self.value.len()
                } else {
                    self.line_end(self.caret.cursor)
                };
                self.move_to(target, extend);
                EditOutcome::handled()
            }
            EditAction::SelectAll => {
                self.caret = Caret {
                    anchor: 0,
                    cursor: self.value.len(),
                };
                self.preferred_column = None;
                EditOutcome::handled()
            }
            EditAction::PlaceCaret { offset, extend } => {
                let offset = self.clamp(offset);
                if extend {
                    self.caret.cursor = offset;
                } else {
                    self.caret = Caret::at(offset);
                }
                self.preferred_column = None;
                EditOutcome::handled()
            }
            // Vertical movement needs the rendered row table, so the component
            // resolves the target boundary from the canonical layout and calls
            // `move_vertical`. Reporting it as unhandled here would let the key
            // bubble while the caret silently stayed put.
            EditAction::MoveUp { .. }
            | EditAction::MoveDown { .. }
            | EditAction::PageUp { .. }
            | EditAction::PageDown { .. } => EditOutcome::handled(),
        }
    }

    pub(crate) fn move_vertical(&mut self, target: usize, extend: bool, preferred: Option<usize>) {
        let target = self.clamp(target);
        if extend {
            self.caret.cursor = target;
        } else {
            self.caret = Caret::at(target);
        }
        self.preferred_column = preferred;
    }

    #[allow(dead_code)] // Read by the model's own preferred-column tests.
    pub(crate) fn preferred_column(&self) -> Option<usize> {
        self.preferred_column
    }

    pub(crate) fn vertical_move(
        &mut self,
        layout: &crate::basic::text_layout::TextLayout,
        direction: i32,
        steps: usize,
        extend: bool,
        policy: EditPolicy,
    ) -> EditOutcome {
        if policy.disabled || !policy.multiline {
            return EditOutcome::default();
        }
        let mut target = self.caret.cursor;
        let mut preferred = self.preferred_column;
        for _ in 0..steps.max(1) {
            let (next, column) = layout.vertical(target, direction, preferred);
            if next == target {
                break;
            }
            target = next;
            preferred = column;
        }
        self.move_vertical(target, extend, preferred);
        EditOutcome {
            handled: true,
            reveal_caret: true,
            ..EditOutcome::default()
        }
    }

    fn insert(&mut self, inserted: &str, policy: EditPolicy) -> EditOutcome {
        let inserted = normalize(inserted, policy.multiline);
        if inserted.is_empty() {
            return EditOutcome::handled();
        }
        // Input size budget: a paste or keystroke that would push the value past
        // the shared ceiling is refused rather than accepted in part, so the
        // editor can never grow without bound.
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
        self.value = next;
        self.caret = Caret::at(self.clamp(start + inserted.len()));
        self.preferred_column = None;
        EditOutcome {
            handled: true,
            changed: true,
            value: Some(self.value.clone()),
            intent: EditIntent::Insert,
            ..EditOutcome::default()
        }
    }

    fn delete_range(&mut self, start: usize, end: usize, intent: EditIntent) -> EditOutcome {
        if start >= end {
            return EditOutcome::handled();
        }
        let removed = self.value[start..end].to_string();
        self.value.replace_range(start..end, "");
        self.caret = Caret::at(self.clamp(start));
        self.preferred_column = None;
        EditOutcome {
            handled: true,
            changed: true,
            value: Some(self.value.clone()),
            removed_text: Some(removed),
            intent,
            ..EditOutcome::default()
        }
    }

    fn move_to(&mut self, target: usize, extend: bool) {
        let target = self.clamp(target);
        if extend {
            self.caret.cursor = target;
        } else {
            self.caret = Caret::at(target);
        }
        self.preferred_column = None;
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        self.units()
            .into_iter()
            .filter(|unit| unit.end <= offset)
            .map(|unit| unit.start)
            .next_back()
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        self.units()
            .into_iter()
            .map(|unit| unit.end)
            .find(|end| *end > offset)
            .unwrap_or(self.value.len())
    }

    fn line_bounds(&self, offset: usize) -> (usize, usize) {
        let offset = self.clamp(offset);
        let start = self.value[..offset]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let end = self.value[offset..]
            .find('\n')
            .map_or(self.value.len(), |index| offset + index);
        (start, end)
    }

    fn line_start(&self, offset: usize) -> usize {
        self.line_bounds(offset).0
    }

    fn line_end(&self, offset: usize) -> usize {
        self.line_bounds(offset).1
    }

    fn open_draft(&mut self) {
        if self.ownership != ValueOwnership::Controlled {
            return;
        }
        // Extending an open draft keeps its original value/selection: the draft
        // bridges every event until the owner's next render answers it.
        self.draft = Some(EmittedDraft {
            value: self.value.clone(),
            caret: self.caret,
        });
    }
}

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
            // Joining the first and last inserted units to their neighbors can
            // recover at most two independently counted units.
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

fn word_left(value: &str, offset: usize, merging: EmojiMerging) -> usize {
    let offset = offset.min(value.len());
    let mut boundary = 0usize;
    let mut seen_word = false;
    for unit in display_units(value, merging) {
        if unit.start >= offset {
            break;
        }
        let is_word = is_word_unit(&value[unit.clone()]);
        if is_word {
            if !seen_word {
                // The start of the word we are leaving.
                boundary = unit.start;
            }
            seen_word = true;
        } else {
            seen_word = false;
        }
    }
    if seen_word { boundary } else { offset }
}

fn word_right(value: &str, offset: usize, merging: EmojiMerging) -> usize {
    let offset = offset.min(value.len());
    let mut seen_word = false;
    for unit in display_units(value, merging) {
        if unit.end <= offset {
            continue;
        }
        let is_word = is_word_unit(&value[unit.clone()]);
        if is_word {
            seen_word = true;
        } else if seen_word {
            // The start of the first unit after the word we just crossed.
            return unit.start;
        }
    }
    value.len()
}

fn is_word_unit(unit: &str) -> bool {
    unit.chars().any(char::is_alphanumeric)
}

#[cfg(test)]
mod tests;
