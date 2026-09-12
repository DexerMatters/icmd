//! The pure edit model for `raw_input`.
//!
//! This module owns the durable state of a text field: its normalized value,
//! selection, caret, and the explicit controlled/uncontrolled value-ownership
//! state machine. It performs no rendering and knows nothing about `Node`,
//! `Theme`, events, or callbacks, so every rule below is unit-testable without
//! constructing a runtime.
//!
//! Coordinates are UTF-8 source byte offsets into the model's normalized value
//! and are always extended-grapheme boundaries.

use unicode_segmentation::UnicodeSegmentation;

/// Where the editable value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ValueOwnership {
    /// The model owns the value after the initial render.
    #[default]
    Uncontrolled,
    /// A render supplies the value; the model may still edit optimistically
    /// between renders but the owner is authoritative at every render.
    Controlled,
}

/// The model's selection and caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Caret {
    /// The fixed end of the selection.
    pub anchor: usize,
    /// The moving end of the selection (and the insertion point).
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

/// A draft emitted to a controlled owner that the owner has not yet
/// acknowledged with a render.
///
/// The draft carries only what acceptance needs: the value that was emitted and
/// the selection that produced it. Acceptance is decided by the owner's next
/// render republishing that exact value; there is no causality heuristic over
/// historical strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmittedDraft {
    /// The value emitted through `on_change`.
    pub(crate) value: String,
    /// The selection/caret that produced the draft, restored when the owner
    /// accepts it at the next render.
    pub(crate) caret: Caret,
}

/// What a render told the model about the value it supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The render was accepted and changed the model.
    Changed,
    /// The render matched the model exactly.
    Unchanged,
}

/// A typed editing command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditAction {
    Insert(String),
    InsertNewline,
    Backspace {
        word: bool,
    },
    Delete {
        word: bool,
    },
    MoveLeft {
        extend: bool,
        word: bool,
    },
    MoveRight {
        extend: bool,
        word: bool,
    },
    MoveUp {
        extend: bool,
    },
    MoveDown {
        extend: bool,
    },
    PageUp {
        extend: bool,
        rows: usize,
    },
    PageDown {
        extend: bool,
        rows: usize,
    },
    Home {
        extend: bool,
        document: bool,
    },
    End {
        extend: bool,
        document: bool,
    },
    SelectAll,
    /// Place the caret from a pointer hit: the boundary the hit resolved to,
    /// and whether the drag extension should keep the existing anchor.
    PlaceCaret {
        offset: usize,
        extend: bool,
    },
}

/// The visible effect of one edit action.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct EditOutcome {
    /// The action was recognized and consumed.
    pub(crate) handled: bool,
    /// The value changed and must be emitted through `on_change`.
    pub(crate) changed: bool,
    /// The value to emit; `Some` exactly when `changed` is true.
    pub(crate) value: Option<String>,
    /// Text removed by a cut, for `on_clipboard`.
    pub(crate) clipboard: Option<String>,
    /// The current value when the user submitted, for `on_submit`.
    pub(crate) submit: Option<String>,
    /// The caret moved or moved to a place that must be scrolled into view.
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

/// The model's configurable policy, derived from the component props.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct EditPolicy {
    pub(crate) multiline: bool,
    pub(crate) max_length: Option<usize>,
    pub(crate) read_only: bool,
    pub(crate) disabled: bool,
}

/// Normalize a value for the given mode.
///
/// Multiline canonicalizes CRLF/CR to LF and keeps newlines and tabs while
/// discarding other control characters. Single line maps CR, LF, and tab to
/// spaces and discards other control characters.
pub(crate) fn normalize(value: &str, multiline: bool) -> String {
    let canonical = value.replace("\r\n", "\n").replace('\r', "\n");
    if multiline {
        canonical
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
            .collect()
    } else {
        canonical
            .chars()
            .map(|ch| match ch {
                '\n' | '\t' => ' ',
                other => other,
            })
            .filter(|ch| !ch.is_control())
            .collect()
    }
}

/// The pure editor model.
#[derive(Debug, Clone, Default)]
pub(crate) struct EditModel {
    pub(crate) value: String,
    pub(crate) caret: Caret,
    /// Preferred terminal-cell column for vertical movement.
    preferred_column: Option<usize>,
    ownership: ValueOwnership,
    /// The value the owner last published (or the uncontrolled value).
    authoritative: String,
    /// Set once the first render has initialized the model.
    initialized: bool,
    /// The open draft for a controlled field, if any.
    draft: Option<EmittedDraft>,
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

    /// The model's normalized value is the selection coordinate space.
    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.value.len()
    }

    /// Grapheme boundaries in the current value, including 0 and `len`.
    #[allow(dead_code)]
    pub(crate) fn boundaries(&self) -> Vec<usize> {
        let mut boundaries = vec![0usize];
        boundaries.extend(
            self.value
                .grapheme_indices(true)
                .map(|(index, grapheme)| index + grapheme.len()),
        );
        boundaries
    }

    /// Snap an arbitrary byte offset to the nearest grapheme boundary.
    pub(crate) fn clamp(&self, offset: usize) -> usize {
        let offset = offset.min(self.value.len());
        if offset == 0 || offset == self.value.len() {
            return offset;
        }
        let mut previous = 0usize;
        for (index, grapheme) in self.value.grapheme_indices(true) {
            let end = index + grapheme.len();
            if offset >= end {
                previous = end;
                continue;
            }
            // `offset` lies inside this grapheme; pick the nearer boundary.
            return if offset - index <= end - offset {
                index
            } else {
                end
            };
        }
        previous
    }

    /// Reconcile the model with a render.
    ///
    /// - `value` is the controlled value when the caller supplied one.
    /// - `default_value` seeds an uncontrolled model exactly once.
    ///
    /// The owner is authoritative: on every render of a controlled field the
    /// supplied value wins. When it equals the emitted draft the draft's
    /// selection is restored (acceptance); otherwise the selection is clamped
    /// into the authoritative value (rejection or external replacement).
    pub(crate) fn render(
        &mut self,
        value: Option<&str>,
        default_value: Option<&str>,
        multiline: bool,
    ) -> Outcome {
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

    /// Reduce one edit action against the model.
    ///
    /// For a controlled field the reduction is optimistic: it produces a draft
    /// that bridges rapid events until the owner's next render.
    pub(crate) fn reduce(&mut self, action: EditAction, policy: EditPolicy) -> EditOutcome {
        if policy.disabled {
            return EditOutcome::default();
        }
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

    /// Restore an open draft's speculative value and selection into the model.
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
            EditAction::Backspace { word } => {
                if !editable {
                    return EditOutcome::handled();
                }
                let (start, end) = self.caret.range();
                let (start, end) = if start != end {
                    (start, end)
                } else {
                    let target = if word {
                        word_left(&self.value, self.caret.cursor)
                    } else {
                        self.previous_boundary(self.caret.cursor)
                    };
                    (target, self.caret.cursor)
                };
                self.delete_range(start, end)
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
                        word_right(&self.value, self.caret.cursor)
                    } else {
                        self.next_boundary(self.caret.cursor)
                    };
                    (self.caret.cursor, target)
                };
                self.delete_range(start, end)
            }
            EditAction::MoveLeft { extend, word } => {
                let (start, end) = self.caret.range();
                let target = if !extend && start != end {
                    start
                } else if word {
                    word_left(&self.value, self.caret.cursor)
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
                    word_right(&self.value, self.caret.cursor)
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

    /// Apply a vertical move whose target boundary was resolved by the caller
    /// from the canonical layout, carrying the preferred terminal-cell column
    /// forward so a caret crossing short rows returns to its column.
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

    /// Resolve a vertical movement against the row table and apply it.
    ///
    /// `direction` is a signed row delta and `page` repeats the move for
    /// PageUp/PageDown. Returns the outcome the caller should act on.
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
        let (start, end) = self.caret.range();
        let before = &self.value[..start];
        let after = &self.value[end..];
        let inserted = match policy.max_length {
            Some(limit) => constrain_insertion(before, &inserted, after, limit),
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
            ..EditOutcome::default()
        }
    }

    fn delete_range(&mut self, start: usize, end: usize) -> EditOutcome {
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
            clipboard: Some(removed),
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
        if offset == 0 {
            return 0;
        }
        self.value[..offset]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        if offset >= self.value.len() {
            return self.value.len();
        }
        self.value[offset..]
            .graphemes(true)
            .next()
            .map_or(self.value.len(), |grapheme| offset + grapheme.len())
    }

    /// The byte range of the logical line containing `offset`.
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

    /// Open (or extend) the optimistic draft for a controlled field.
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

/// Insert `inserted` at the selection while respecting `limit` graphemes.
///
/// Graphemes can merge across either insertion boundary (for example, a
/// combining mark inserted after its base), so candidate ends are tested in
/// context rather than by subtracting independent grapheme counts.
fn constrain_insertion(before: &str, inserted: &str, after: &str, limit: usize) -> String {
    let with = |inserted: &str| {
        let mut value = String::with_capacity(before.len() + inserted.len() + after.len());
        value.push_str(before);
        value.push_str(inserted);
        value.push_str(after);
        value
    };
    if with(inserted).graphemes(true).count() <= limit {
        return inserted.to_owned();
    }
    let base_count = with("").graphemes(true).count();
    let available = limit.saturating_sub(base_count);
    let mut candidate_ends = vec![0usize];
    candidate_ends.extend(
        inserted
            .grapheme_indices(true)
            .map(|(index, grapheme)| index + grapheme.len())
            // Joining the first and last inserted clusters to their neighbors
            // can recover at most two independently counted clusters.
            .take(available.saturating_add(2)),
    );
    let mut accepted_end = 0usize;
    let first_candidate = available
        .saturating_sub(2)
        .min(candidate_ends.len().saturating_sub(1));
    for &end in &candidate_ends[first_candidate..] {
        if with(&inserted[..end]).graphemes(true).count() <= limit {
            accepted_end = end;
        }
    }
    inserted[..accepted_end].to_owned()
}

fn word_left(value: &str, offset: usize) -> usize {
    let offset = offset.min(value.len());
    let mut boundary = 0usize;
    let mut seen_word = false;
    for (index, grapheme) in value.grapheme_indices(true) {
        if index >= offset {
            break;
        }
        let is_word = is_word_grapheme(grapheme);
        if is_word {
            if !seen_word {
                // The start of the word we are leaving.
                boundary = index;
            }
            seen_word = true;
        } else {
            seen_word = false;
        }
    }
    if seen_word { boundary } else { offset }
}

fn word_right(value: &str, offset: usize) -> usize {
    let offset = offset.min(value.len());
    let mut seen_word = false;
    for (index, grapheme) in value[offset..].grapheme_indices(true) {
        let is_word = is_word_grapheme(grapheme);
        if is_word {
            seen_word = true;
        } else if seen_word {
            // The end of the word we just crossed.
            return offset + index;
        }
    }
    value.len()
}

fn is_word_grapheme(grapheme: &str) -> bool {
    grapheme.chars().any(char::is_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uncontrolled(value: &str, multiline: bool) -> EditModel {
        let mut model = EditModel::default();
        model.render(None, Some(value), multiline);
        model
    }

    fn controlled(value: &str, multiline: bool) -> EditModel {
        let mut model = EditModel::default();
        model.render(Some(value), None, multiline);
        model
    }

    fn policy(multiline: bool) -> EditPolicy {
        EditPolicy {
            multiline,
            ..EditPolicy::default()
        }
    }

    fn insert(model: &mut EditModel, text: &str, multiline: bool) -> EditOutcome {
        model.reduce(EditAction::Insert(text.to_string()), policy(multiline))
    }

    #[test]
    fn insert_replaces_the_selection() {
        let mut model = uncontrolled("hello", false);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 1,
                extend: false,
            },
            policy(false),
        );
        model.reduce(
            EditAction::PlaceCaret {
                offset: 4,
                extend: true,
            },
            policy(false),
        );
        let outcome = insert(&mut model, "i", false);
        assert_eq!(model.value(), "hio");
        assert_eq!(outcome.value.as_deref(), Some("hio"));
        assert_eq!(model.caret().cursor, 2);
    }

    #[test]
    fn backspace_and_delete_remove_one_grapheme() {
        let mut model = uncontrolled("a界b", false);
        // Caret after the wide grapheme (offset 4).
        model.reduce(
            EditAction::PlaceCaret {
                offset: 4,
                extend: false,
            },
            policy(false),
        );
        insert(&mut model, "X", false);
        assert_eq!(model.value(), "a界Xb");

        let outcome = model.reduce(EditAction::Backspace { word: false }, policy(false));
        assert_eq!(model.value(), "a界b");
        assert_eq!(outcome.clipboard.as_deref(), Some("X"));

        let outcome = model.reduce(EditAction::Delete { word: false }, policy(false));
        assert_eq!(model.value(), "a界");
        assert_eq!(outcome.clipboard.as_deref(), Some("b"));
    }

    #[test]
    fn home_and_end_move_within_the_logical_line() {
        let mut model = uncontrolled("abc\ndef", true);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 6,
                extend: false,
            },
            policy(true),
        );
        model.reduce(
            EditAction::Home {
                extend: false,
                document: false,
            },
            policy(true),
        );
        assert_eq!(model.caret().cursor, 4);
        model.reduce(
            EditAction::End {
                extend: false,
                document: false,
            },
            policy(true),
        );
        assert_eq!(model.caret().cursor, 7);
        model.reduce(
            EditAction::Home {
                extend: false,
                document: true,
            },
            policy(true),
        );
        assert_eq!(model.caret().cursor, 0);
        model.reduce(
            EditAction::End {
                extend: false,
                document: true,
            },
            policy(true),
        );
        assert_eq!(model.caret().cursor, 7);
    }

    #[test]
    fn select_all_covers_the_whole_value() {
        let mut model = uncontrolled("abc", false);
        model.reduce(EditAction::SelectAll, policy(false));
        assert_eq!(model.caret().range(), (0, 3));
        insert(&mut model, "z", false);
        assert_eq!(model.value(), "z");
    }

    #[test]
    fn left_and_right_collapse_an_existing_selection() {
        let mut model = uncontrolled("abcdef", false);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 1,
                extend: false,
            },
            policy(false),
        );
        model.reduce(
            EditAction::PlaceCaret {
                offset: 4,
                extend: true,
            },
            policy(false),
        );
        model.reduce(
            EditAction::MoveLeft {
                extend: false,
                word: false,
            },
            policy(false),
        );
        assert_eq!(model.caret().cursor, 1);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 1,
                extend: false,
            },
            policy(false),
        );
        model.reduce(
            EditAction::PlaceCaret {
                offset: 4,
                extend: true,
            },
            policy(false),
        );
        model.reduce(
            EditAction::MoveRight {
                extend: false,
                word: false,
            },
            policy(false),
        );
        assert_eq!(model.caret().cursor, 4);
    }

    #[test]
    fn word_movement_skips_whole_words() {
        let mut model = uncontrolled("hello brave world", false);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 0,
                extend: false,
            },
            policy(false),
        );
        model.reduce(
            EditAction::MoveRight {
                extend: false,
                word: true,
            },
            policy(false),
        );
        assert_eq!(model.caret().cursor, 5);
        model.reduce(
            EditAction::MoveRight {
                extend: false,
                word: true,
            },
            policy(false),
        );
        assert_eq!(model.caret().cursor, 11);
        model.reduce(
            EditAction::MoveLeft {
                extend: false,
                word: true,
            },
            policy(false),
        );
        assert_eq!(model.caret().cursor, 6);
    }

    #[test]
    fn vertical_targets_are_valid_grapheme_boundaries() {
        let mut model = uncontrolled("abcdef\nxy\nabcdef", true);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 5,
                extend: false,
            },
            policy(true),
        );
        // The component resolves the target from the layout; the model only
        // validates it.
        model.move_vertical(9, false, Some(2));
        assert_eq!(model.caret().cursor, 9);
        assert_eq!(model.preferred_column(), Some(2));
    }

    #[test]
    fn single_line_normalization_maps_newlines_and_tabs_to_spaces() {
        assert_eq!(normalize("a\r\nb\tc\rd", false), "a b c d");
        assert_eq!(normalize("a\u{7}b", false), "ab");
    }

    #[test]
    fn multiline_normalization_keeps_newlines_and_tabs() {
        assert_eq!(normalize("a\r\nb", true), "a\nb");
        assert_eq!(normalize("a\rb", true), "a\nb");
        assert_eq!(normalize("a\tb", true), "a\tb");
        assert_eq!(normalize("a\u{7}b", true), "ab");
    }

    #[test]
    fn paste_is_normalized_before_insertion() {
        let mut model = uncontrolled("", true);
        insert(&mut model, "界\r\n🙂", true);
        assert_eq!(model.value(), "界\n🙂");

        let mut model = uncontrolled("", false);
        insert(&mut model, "a\nb\tc", false);
        assert_eq!(model.value(), "a b c");
    }

    #[test]
    fn max_length_counts_graphemes_and_respects_merging() {
        let mut model = uncontrolled("", false);
        let outcome = model.reduce(
            EditAction::Insert("界界界".into()),
            EditPolicy {
                max_length: Some(2),
                ..policy(false)
            },
        );
        assert_eq!(outcome.value.as_deref(), Some("界界"));

        // Inserting a combining mark after its base must not be rejected when
        // the merged cluster still fits.
        let mut model = uncontrolled("e", false);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 1,
                extend: false,
            },
            policy(false),
        );
        model.reduce(
            EditAction::Insert("\u{301}".into()),
            EditPolicy {
                max_length: Some(1),
                ..policy(false)
            },
        );
        assert_eq!(model.value(), "e\u{301}");
        assert_eq!(model.value().graphemes(true).count(), 1);
    }

    #[test]
    fn read_only_and_disabled_do_not_mutate() {
        let mut model = uncontrolled("abc", false);
        let read_only = EditPolicy {
            read_only: true,
            ..policy(false)
        };
        model.reduce(EditAction::Insert("x".into()), read_only);
        model.reduce(EditAction::Backspace { word: false }, read_only);
        assert_eq!(model.value(), "abc");

        let disabled = EditPolicy {
            disabled: true,
            ..policy(false)
        };
        let outcome = model.reduce(EditAction::Insert("x".into()), disabled);
        assert!(!outcome.handled);
        assert_eq!(model.value(), "abc");
    }

    #[test]
    fn controlled_acceptance_restores_the_draft_selection() {
        let mut model = controlled("a", false);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 1,
                extend: false,
            },
            policy(false),
        );
        let outcome = insert(&mut model, "b", false);
        assert_eq!(outcome.value.as_deref(), Some("ab"));
        // The owner accepts by rendering exactly the emitted value.
        let outcome = model.render(Some("ab"), None, false);
        assert_eq!(outcome, Outcome::Unchanged);
        assert_eq!(model.caret().cursor, 2, "accepted draft keeps its caret");
    }

    #[test]
    fn controlled_rejection_clamps_the_selection() {
        let mut model = controlled("abc", false);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 3,
                extend: false,
            },
            policy(false),
        );
        insert(&mut model, "xy", false);
        assert_eq!(model.value(), "abcxy");
        // A different value is a rejection/external replacement.
        model.render(Some("ab"), None, false);
        assert_eq!(model.value(), "ab");
        assert_eq!(model.caret().cursor, 2);
    }

    #[test]
    fn controlled_rejection_clamps_the_selection_into_the_owner_value() {
        let mut model = controlled("abcd", false);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 4,
                extend: false,
            },
            policy(false),
        );
        insert(&mut model, "xy", false);
        assert_eq!(model.value(), "abcdxy");
        // The owner republishes a shorter value: the draft is discarded and the
        // selection is clamped into the value the owner supplied.
        model.render(Some("ab"), None, false);
        assert_eq!(model.value(), "ab");
        assert_eq!(model.caret().cursor, 2);
        // The next keystroke edits the owner's value, never the rejected draft.
        let outcome = insert(&mut model, "c", false);
        assert_eq!(outcome.value.as_deref(), Some("abc"));
    }

    #[test]
    fn controlled_rejection_is_deterministic_per_render() {
        // The owner ignores every change, so each render republishes "a". The
        // first keystroke produces "ab"; the next render rejects it back to "a"
        // and clamps the caret to the end of "a". The following keystroke must
        // therefore produce "ac", not resurrect the rejected "ab".
        let mut model = controlled("a", false);
        let mut emitted = Vec::new();
        for ch in ["b", "c"] {
            if let Some(value) = insert(&mut model, ch, false).value {
                emitted.push(value);
            }
            // The owner rejects: it republishes its own authoritative value.
            model.render(Some("a"), None, false);
            assert_eq!(model.value(), "a", "rejection must restore the owner value");
        }
        assert_eq!(
            emitted,
            vec!["ab".to_string(), "ac".to_string()],
            "a rejected draft must not survive into the next keystroke"
        );
    }

    #[test]
    fn acceptance_then_extension_keeps_the_caret() {
        let mut model = controlled("a", false);
        // Click places the caret after 'a'.
        model.reduce(
            EditAction::PlaceCaret {
                offset: 1,
                extend: false,
            },
            policy(false),
        );
        assert_eq!(insert(&mut model, "x", false).value.as_deref(), Some("ax"));
        assert_eq!(model.caret().cursor, 2);
        // The owner accepts by republishing exactly the draft.
        model.render(Some("ax"), None, false);
        assert_eq!(model.value(), "ax");
        assert_eq!(model.caret().cursor, 2, "acceptance restores the caret");
        // The next keystroke extends the accepted value.
        assert_eq!(insert(&mut model, "y", false).value.as_deref(), Some("axy"));
    }

    #[test]
    fn controlled_external_replacement_discards_the_draft() {
        let mut model = controlled("a", false);
        insert(&mut model, "b", false);
        assert_eq!(model.value(), "ab");
        // The owner publishes an unrelated value. The caret clamps into it at
        // its previous offset (2), and the rejected draft is gone.
        model.render(Some("zzz"), None, false);
        assert_eq!(model.value(), "zzz");
        assert_eq!(model.caret().cursor, 2);
        let outcome = insert(&mut model, "!", false);
        assert_eq!(
            outcome.value.as_deref(),
            Some("zz!z"),
            "editing continues in the owner's value, not the draft"
        );
        assert!(!model.value().contains("ab"), "the draft must be discarded");
    }

    #[test]
    fn rapid_events_before_a_render_accumulate_on_one_draft() {
        let mut model = controlled("", false);
        for ch in ["h", "e", "l", "l", "o"] {
            insert(&mut model, ch, false);
        }
        assert_eq!(model.value(), "hello");
        // A single acceptance render keeps the whole draft.
        model.render(Some("hello"), None, false);
        assert_eq!(model.caret().cursor, 5);
    }

    #[test]
    fn switching_from_uncontrolled_to_controlled_adopts_the_value() {
        let mut model = uncontrolled("local", false);
        model.render(Some("owner"), None, false);
        assert_eq!(model.value(), "owner");
        assert_eq!(model.ownership(), ValueOwnership::Controlled);
    }

    #[test]
    fn switching_to_uncontrolled_discards_an_unaccepted_draft() {
        // The owner published "a" and never accepted the speculative "ab".
        // Switching to uncontrolled must resume from "a", not from a draft the
        // owner rejected by staying silent.
        let mut model = controlled("a", false);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 1,
                extend: false,
            },
            policy(false),
        );
        insert(&mut model, "b", false);
        assert_eq!(model.value(), "ab");
        // The component stops supplying a value before the owner responded.
        model.render(None, None, false);
        assert_eq!(
            model.value(),
            "a",
            "the unaccepted draft must not become the uncontrolled value"
        );
        assert_eq!(model.ownership(), ValueOwnership::Uncontrolled);
        // Local ownership resumes from that value.
        let outcome = insert(&mut model, "!", false);
        assert_eq!(outcome.value.as_deref(), Some("a!"));
    }

    #[test]
    fn switching_from_controlled_to_uncontrolled_keeps_the_last_value() {
        let mut model = controlled("owner", false);
        model.render(None, None, false);
        assert_eq!(model.value(), "owner");
        assert_eq!(model.ownership(), ValueOwnership::Uncontrolled);
        // Local ownership resumes.
        let outcome = insert(&mut model, "!", false);
        assert_eq!(outcome.value.as_deref(), Some("owner!"));
    }

    #[test]
    fn the_render_revision_decides_whether_a_draft_was_answerable() {
        // A draft records the render it was produced from. Only a later render
        // can have been the owner's response to it, so the caret snapshot is
        // restored on that render and never on the render that produced it.
        let mut model = controlled("a", false);
        model.reduce(
            EditAction::PlaceCaret {
                offset: 1,
                extend: false,
            },
            policy(false),
        );
        assert_eq!(insert(&mut model, "b", false).value.as_deref(), Some("ab"));

        // The owner republishes the emitted value at the NEXT render: accepted,
        // and the selection that produced the draft is restored.
        let outcome = model.render(Some("ab"), None, false);
        assert_eq!(outcome, Outcome::Unchanged);
        assert_eq!(model.caret().cursor, 2);

        // The owner then replaces the value entirely: no draft is open, so this
        // is a plain replacement rather than an acceptance, and the selection is
        // clamped into the new value rather than restored from a snapshot.
        model.render(Some("xyz"), None, false);
        assert_eq!(model.value(), "xyz");
        assert_eq!(
            model.caret().cursor,
            2,
            "the selection is clamped into the replacement, not reset"
        );
        assert!(model.caret().cursor <= model.len());
    }

    #[test]
    fn unrelated_rerender_is_stable() {
        let mut model = controlled("same", false);
        assert_eq!(model.render(Some("same"), None, false), Outcome::Unchanged);
    }

    #[test]
    fn clamp_snaps_to_the_nearest_grapheme_boundary() {
        // "e" is one byte; the combining mark spans bytes 1..3; "x" is byte 3.
        let model = uncontrolled("e\u{301}x", false);
        assert_eq!(model.clamp(0), 0);
        assert_eq!(model.clamp(1), 0, "inside the cluster, nearer the start");
        assert_eq!(model.clamp(2), 3, "inside the cluster, nearer the end");
        assert_eq!(model.clamp(3), 3, "the cluster's trailing boundary");
        assert_eq!(model.clamp(4), 4);
        assert_eq!(model.clamp(usize::MAX), 4);
    }
}
