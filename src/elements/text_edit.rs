use std::{
    fmt, ops,
    sync::{Arc, Mutex},
};

use crossterm::event::{KeyCode, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::basic::text_layout::TextLayout;
use crate::{
    Align, Attr, Dimension, DomProps, Edges, EventHandlers, FocusEvent, Justify, Layout, Node,
    Overflow, Props, ScrollAxes, ScrollEvent, ScrollOffset, ScrollbarVisibility, Span, Style, Text,
    TextWrap, basic::ComponentContext, scroll_area, theme::Theme, ui, view,
};

type TextEditCallback<T> = Box<dyn FnMut(T) + Send + 'static>;

#[derive(Clone)]
pub struct TextEditHandler<T> {
    callback: Arc<Mutex<TextEditCallback<T>>>,
}

impl<T> TextEditHandler<T> {
    pub fn new(callback: impl FnMut(T) + Send + 'static) -> Self {
        Self {
            callback: Arc::new(Mutex::new(Box::new(callback))),
        }
    }

    pub fn call(&self, event: T) {
        (self.callback.lock().expect("text edit handler poisoned"))(event);
    }
}

impl<T> fmt::Debug for TextEditHandler<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextEditHandler").finish_non_exhaustive()
    }
}

impl<T> PartialEq for TextEditHandler<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.callback, &other.callback)
    }
}
impl<T> Eq for TextEditHandler<T> {}

impl<T, F> ops::DivAssign<F> for Attr<TextEditHandler<T>>
where
    F: FnMut(T) + Send + 'static,
{
    fn div_assign(&mut self, rhs: F) {
        *self = Attr::Set(TextEditHandler::new(rhs));
    }
}

// The value and clipboard event types are owned by the input module; the
// legacy editor re-exports them so existing callers keep compiling until the
// public cutover removes this component.
pub use super::input::{TextClipboardAction, TextClipboardEvent, TextValueEvent};

#[derive(Clone, Default)]
pub struct InputProps {
    pub value: Attr<String>,
    pub default_value: Attr<String>,
    pub placeholder: Attr<String>,
    pub width: Attr<u16>,
    pub max_length: Attr<usize>,
    pub disabled: Attr<bool>,
    pub read_only: Attr<bool>,
    pub on_change: Attr<TextEditHandler<TextValueEvent>>,
    pub on_submit: Attr<TextEditHandler<TextValueEvent>>,
    pub on_clipboard: Attr<TextEditHandler<TextClipboardEvent>>,
    pub on_focus_change: Attr<TextEditHandler<FocusEvent>>,
}

impl fmt::Debug for InputProps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputProps")
            .field("value", &self.value)
            .field("default_value", &self.default_value)
            .field("placeholder", &self.placeholder)
            .field("width", &self.width)
            .field("max_length", &self.max_length)
            .field("disabled", &self.disabled)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Default)]
pub struct TextAreaProps {
    pub value: Attr<String>,
    pub default_value: Attr<String>,
    pub placeholder: Attr<String>,
    pub width: Attr<u16>,
    pub height: Attr<u16>,
    pub wrap: Attr<TextWrap>,
    pub max_length: Attr<usize>,
    pub disabled: Attr<bool>,
    pub read_only: Attr<bool>,
    pub on_change: Attr<TextEditHandler<TextValueEvent>>,
    pub on_clipboard: Attr<TextEditHandler<TextClipboardEvent>>,
    pub on_focus_change: Attr<TextEditHandler<FocusEvent>>,
}

impl fmt::Debug for TextAreaProps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextAreaProps")
            .field("value", &self.value)
            .field("default_value", &self.default_value)
            .field("placeholder", &self.placeholder)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("wrap", &self.wrap)
            .field("max_length", &self.max_length)
            .field("disabled", &self.disabled)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct EditorState {
    value: String,
    cursor: usize,
    anchor: usize,
    initialized: bool,
    controlled: Option<String>,
    focused: bool,
    dragging: bool,
    preferred_column: Option<usize>,
    scroll_y: usize,
    scroll_x: usize,
    ensure_cursor_visible: bool,
    layout: Option<EditorLayout>,
    /// Set when the paint pass reports a content width different from the one
    /// the rows were wrapped for. A request for the next render is a state
    /// change, and the editor's render pass is the only place that can make it.
    remeasure: bool,
    /// The content width the current rows were wrapped for. It follows what the
    /// layout grants rather than what the widget asked for, so a parent that
    /// clamps the box cannot put the rows and the viewport out of step.
    wrapped_width: u16,
    pending_controlled: Option<PendingControlledEdit>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct EditorLayout {
    width: u16,
    height: u16,
    wrap: TextWrap,
}

#[derive(Clone)]
struct PendingControlledEdit {
    /// The authoritative value the owner had published when the edit was made.
    /// Renders that echo it are not external replacements.
    authoritative: String,
    /// The speculative post-edit buffer, already emitted through `on_change`.
    value: String,
    cursor: usize,
    anchor: usize,
}

#[derive(Clone, Copy)]
struct EditorConfig {
    multiline: bool,
    /// The content box width, already reduced to what the layout granted.
    width: u16,
    height: u16,
    wrap: TextWrap,
    max_length: Option<usize>,
    disabled: bool,
    read_only: bool,
}

struct EditorBindings {
    placeholder: String,
    value: Option<String>,
    default_value: Option<String>,
    on_change: Option<TextEditHandler<TextValueEvent>>,
    on_submit: Option<TextEditHandler<TextValueEvent>>,
    on_clipboard: Option<TextEditHandler<TextClipboardEvent>>,
    on_focus_change: Option<TextEditHandler<FocusEvent>>,
}

fn visually_wrapped(config: EditorConfig) -> bool {
    config.multiline && matches!(config.wrap, TextWrap::Soft | TextWrap::Hard)
}

fn normalize(mut value: String, multiline: bool) -> String {
    value = value.replace("\r\n", "\n").replace('\r', "\n");
    if multiline {
        value
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
            .collect()
    } else {
        value
            .chars()
            .map(|ch| {
                if matches!(ch, '\n' | '\r' | '\t') {
                    ' '
                } else {
                    ch
                }
            })
            .filter(|ch| !ch.is_control())
            .collect()
    }
}

fn boundaries(value: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(value.grapheme_indices(true).map(|(index, _)| index))
        .chain(std::iter::once(value.len()))
        .collect()
}

fn clamp_boundary(value: &str, position: usize) -> usize {
    boundaries(value)
        .into_iter()
        .min_by_key(|boundary| boundary.abs_diff(position))
        .unwrap_or(0)
}

fn selection(state: &EditorState) -> (usize, usize) {
    if state.anchor <= state.cursor {
        (state.anchor, state.cursor)
    } else {
        (state.cursor, state.anchor)
    }
}

fn next_boundary(value: &str, position: usize) -> usize {
    boundaries(value)
        .into_iter()
        .find(|boundary| *boundary > position)
        .unwrap_or(value.len())
}

fn previous_boundary(value: &str, position: usize) -> usize {
    boundaries(value)
        .into_iter()
        .rev()
        .find(|boundary| *boundary < position)
        .unwrap_or(0)
}

fn line_bounds(value: &str, position: usize) -> (usize, usize) {
    let start = value[..position].rfind('\n').map_or(0, |index| index + 1);
    let end = value[position..]
        .find('\n')
        .map_or(value.len(), |index| position + index);
    (start, end)
}

fn word_left(value: &str, position: usize) -> usize {
    let mut segments = value[..position]
        .split_word_bound_indices()
        .collect::<Vec<_>>();
    while segments
        .last()
        .is_some_and(|(_, segment)| segment.chars().all(char::is_whitespace))
    {
        segments.pop();
    }
    segments.last().map_or(0, |(index, _)| *index)
}

fn word_right(value: &str, position: usize) -> usize {
    let tail = &value[position..];
    let mut seen_word = false;
    for (index, segment) in tail.split_word_bound_indices() {
        if segment.chars().all(char::is_whitespace) {
            if seen_word {
                return position + index;
            }
        } else {
            seen_word = true;
        }
    }
    value.len()
}

fn cell_column(value: &str) -> usize {
    let mut column = 0usize;
    for grapheme in value.graphemes(true) {
        column += if grapheme == "\t" {
            4 - column % 4
        } else {
            UnicodeWidthStr::width(grapheme).max(1)
        };
    }
    column
}

/// Byte boundary a hit on `cell` selects.
///
/// `cell` is measured in cells, which is what a pointer gives: the caret
/// belongs to the cell that was hit. `boundary_at_cell` returns the boundary at
/// the *start* of that cell, which is what vertical navigation wants (a
/// preferred column). A pointer lands *inside* a cell, so it wants the boundary
/// just after the cell's grapheme - see [`boundary_after_cell`].
fn boundary_at_cell(value: &str, cell: usize, initial_column: usize) -> usize {
    let mut used: usize = 0;
    for (index, grapheme) in value.grapheme_indices(true) {
        let width = if grapheme == "\t" {
            4 - initial_column.saturating_add(used) % 4
        } else {
            UnicodeWidthStr::width(grapheme).max(1)
        };
        if cell < used.saturating_add(width) {
            return if width > 1 && cell.saturating_sub(used) * 2 >= width {
                index + grapheme.len()
            } else {
                index
            };
        }
        used += width;
    }
    value.len()
}

/// Byte boundary a pointer hit on `cell` selects: the far side of that cell, so
/// the caret sits after the character the pointer landed on rather than one
/// cell behind it. A double-width grapheme splits at its half way point.
fn boundary_after_cell(value: &str, cell: usize, initial_column: usize) -> usize {
    let mut used: usize = 0;
    for (index, grapheme) in value.grapheme_indices(true) {
        let width = if grapheme == "\t" {
            4 - initial_column.saturating_add(used) % 4
        } else {
            UnicodeWidthStr::width(grapheme).max(1)
        };
        let end = used.saturating_add(width);
        if end > cell {
            return if width > 1 && cell.saturating_sub(used) * 2 < width {
                index
            } else {
                index + grapheme.len()
            };
        }
        used = end;
    }
    value.len()
}

fn logical_column_at(value: &str, position: usize) -> usize {
    let line_start = value[..position]
        .rfind('\n')
        .map_or(0, |index| index.saturating_add(1));
    cell_column(&value[line_start..position])
}

fn row_column(value: &str, row: VisualRow, position: usize) -> usize {
    let initial = logical_column_at(value, row.start);
    let position = position.max(row.start).min(row.end);
    cell_column_with_initial(&value[row.start..position], initial)
}

fn cursor_cell_width(value: &str, row: VisualRow, position: usize) -> usize {
    let position = position.max(row.start).min(row.end);
    if position >= row.end {
        return 1;
    }
    let grapheme = value[position..row.end]
        .graphemes(true)
        .next()
        .unwrap_or(" ");
    if grapheme == "\t" {
        let column = logical_column_at(value, row.start) + row_column(value, row, position);
        4 - column % 4
    } else {
        UnicodeWidthStr::width(grapheme).max(1)
    }
}

fn scroll_x_for_cursor(
    value: &str,
    row: VisualRow,
    position: usize,
    current: usize,
    visible_width: usize,
) -> usize {
    let visible_width = visible_width.max(1);
    let column = row_column(value, row, position);
    if column < current {
        return column;
    }

    let caret_width = cursor_cell_width(value, row, position);
    if caret_width > visible_width {
        // The terminal cannot display a wide grapheme in a narrower viewport.
        // Keep its leading boundary visible rather than scrolling into its
        // continuation cell.
        return if column >= current.saturating_add(visible_width) {
            column
        } else {
            current
        };
    }
    let caret_end = column.saturating_add(caret_width);
    if caret_end > current.saturating_add(visible_width) {
        caret_end - visible_width
    } else {
        current
    }
}

fn cell_column_with_initial(value: &str, initial_column: usize) -> usize {
    let mut used = 0usize;
    for grapheme in value.graphemes(true) {
        used += if grapheme == "\t" {
            4 - initial_column.saturating_add(used) % 4
        } else {
            UnicodeWidthStr::width(grapheme).max(1)
        };
    }
    used
}

fn vertical_target(
    value: &str,
    position: usize,
    direction: i32,
    config: EditorConfig,
    preferred: &mut Option<usize>,
) -> usize {
    let rows = visual_rows(value, config);
    let current = visual_row_index(&rows, position);
    let row = rows.get(current).copied().unwrap_or_default();
    let column = preferred.get_or_insert_with(|| row_column(value, row, position));
    let target =
        (current as i32 + direction).clamp(0, rows.len().saturating_sub(1) as i32) as usize;
    let row = rows.get(target).copied().unwrap_or_default();
    row.start
        + boundary_at_cell(
            &value[row.start..row.end],
            *column,
            logical_column_at(value, row.start),
        )
}

fn visual_row_index(rows: &[VisualRow], position: usize) -> usize {
    rows.iter()
        .enumerate()
        .find(|(index, row)| {
            let next_starts_later = rows.get(index + 1).is_none_or(|next| next.start > position);
            let follows_hidden_separator = index.checked_sub(1).is_some_and(|previous| {
                let previous = rows[previous];
                position >= previous.end && position < previous.source_end
            });
            position < row.end
                || position == row.start
                || follows_hidden_separator
                || (position == row.source_end && next_starts_later)
        })
        .map_or_else(|| rows.len().saturating_sub(1), |(index, _)| index)
}

fn value_with_insertion(before: &str, inserted: &str, after: &str) -> String {
    let mut value = String::with_capacity(before.len() + inserted.len() + after.len());
    value.push_str(before);
    value.push_str(inserted);
    value.push_str(after);
    value
}

fn constrain_insertion(before: &str, inserted: &str, after: &str, limit: usize) -> String {
    let full = value_with_insertion(before, inserted, after);
    if full.graphemes(true).count() <= limit {
        return inserted.to_owned();
    }

    // Grapheme clusters can merge across either insertion boundary (for
    // example, inserting a combining mark after its base). Test complete
    // inserted graphemes in context instead of subtracting independent
    // counts, which incorrectly rejects those edits.
    let base_count = value_with_insertion(before, "", after)
        .graphemes(true)
        .count();
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
        if value_with_insertion(before, &inserted[..end], after)
            .graphemes(true)
            .count()
            <= limit
        {
            accepted_end = end;
        }
    }
    inserted[..accepted_end].to_owned()
}

fn replace_selection(
    state: &mut EditorState,
    inserted: &str,
    max_length: Option<usize>,
) -> Option<String> {
    let (start, end) = selection(state);
    let before = &state.value[..start];
    let after = &state.value[end..];
    let inserted = if let Some(limit) = max_length {
        constrain_insertion(before, inserted, after, limit)
    } else {
        inserted.to_owned()
    };
    let next = value_with_insertion(before, &inserted, after);
    if next == state.value {
        return None;
    }
    let cursor = start.saturating_add(inserted.len()).min(next.len());
    state.value = next.clone();
    state.cursor = clamp_boundary(&state.value, cursor);
    state.anchor = state.cursor;
    state.preferred_column = None;
    Some(next)
}

fn delete_range(state: &mut EditorState, start: usize, end: usize) -> Option<String> {
    if start >= end {
        return None;
    }
    state.value.replace_range(start..end, "");
    state.cursor = start.min(state.value.len());
    state.anchor = state.cursor;
    state.preferred_column = None;
    Some(state.value.clone())
}

fn move_cursor(state: &mut EditorState, next: usize, extend: bool) {
    let next = clamp_boundary(&state.value, next);
    if extend {
        state.cursor = next;
    } else {
        state.cursor = next;
        state.anchor = next;
    }
}

fn selected_span(span: Span, focused: bool, theme: &Theme) -> Span {
    if focused {
        span.foreground(theme.colors.primary_foreground)
            .background(theme.colors.primary)
    } else {
        span.foreground(theme.colors.muted_foreground)
            .background(theme.colors.muted)
    }
}

fn styled_text(
    state: &EditorState,
    config: EditorConfig,
    theme: &Theme,
    placeholder: &str,
) -> Text {
    let empty = state.value.is_empty();
    let focused = state.focused && !config.disabled;
    let (selection_start, selection_end) = selection(state);
    let mut spans = Vec::new();
    if empty {
        let mut placeholder_graphemes = placeholder.graphemes(true).enumerate().peekable();
        if focused && placeholder_graphemes.peek().is_none() {
            spans.push(Span::new(" ").reverse());
        }
        for (index, grapheme) in placeholder_graphemes {
            let mut span = Span::new(grapheme).foreground(theme.colors.muted_foreground);
            if focused && index == 0 {
                span = span.reverse();
            }
            spans.push(span);
        }
    } else if matches!(config.wrap, TextWrap::Soft | TextWrap::Hard) {
        let rows = visual_rows(&state.value, config);
        for (row_index, row) in rows.iter().enumerate() {
            let mut byte = row.start;
            let mut logical_column = logical_column_at(&state.value, row.start);
            let caret_from_hidden_separator = row_index.checked_sub(1).is_some_and(|previous| {
                let previous = rows[previous];
                state.cursor >= previous.end && state.cursor < previous.source_end
            });
            for grapheme in state.value[row.start..row.end].graphemes(true) {
                let end = byte + grapheme.len();
                let displayed = if grapheme == "\t" {
                    " ".repeat(4 - logical_column % 4)
                } else {
                    grapheme.to_owned()
                };
                logical_column =
                    logical_column.saturating_add(UnicodeWidthStr::width(displayed.as_str()));
                let mut span = Span::new(displayed);
                if selection_start < end && selection_end > byte {
                    span = selected_span(span, focused, theme);
                }
                if focused
                    && ((state.cursor >= byte && state.cursor < end)
                        || (caret_from_hidden_separator && byte == row.start))
                {
                    span = span.reverse();
                }
                spans.push(span);
                byte = end;
            }
            let source_newline_follows = rows
                .get(row_index + 1)
                .is_some_and(|next| next.start > row.source_end);
            let separator_selected = source_newline_follows
                && rows.get(row_index + 1).is_some_and(|next| {
                    selection_start < next.start && selection_end > row.source_end
                });
            let caret_marker = focused
                && ((caret_from_hidden_separator && row.start == row.end)
                    || (state.cursor == row.end
                        && (row_index + 1 == rows.len() || source_newline_follows)));
            if separator_selected || caret_marker {
                let mut marker = Span::new(" ");
                if separator_selected {
                    marker = selected_span(marker, focused, theme);
                }
                if caret_marker {
                    marker = marker.reverse();
                }
                spans.push(marker);
            }
            if row_index + 1 < rows.len() {
                spans.push(Span::new("\n"));
            }
        }
    } else {
        let mut byte = 0usize;
        for grapheme in state.value.graphemes(true) {
            let end = byte + grapheme.len();
            if grapheme == "\n" {
                let selected = selection_start < end && selection_end > byte;
                let caret = focused && state.cursor == byte;
                if selected || caret {
                    let mut marker = Span::new(" ");
                    if selected {
                        marker = selected_span(marker, focused, theme);
                    }
                    if caret {
                        marker = marker.reverse();
                    }
                    spans.push(marker);
                }
                spans.push(Span::new("\n"));
                byte = end;
                continue;
            }
            let mut span = Span::new(grapheme);
            if selection_start < end && selection_end > byte {
                span = selected_span(span, focused, theme);
            }
            if focused && state.cursor >= byte && state.cursor < end {
                span = span.reverse();
            }
            spans.push(span);
            byte = end;
        }
        if focused && state.cursor == state.value.len() {
            spans.push(Span::new(" ").reverse());
        }
    }
    let mut style = Style::default();
    // This surface is the editor's content box, so it declares exactly the box
    // the wrapping above was computed for. `base_style` lays every inset out
    // around those cells, which keeps the painted viewport and the wrapped rows
    // in agreement: a viewport one cell narrower than the wrapping width drops
    // the final glyph of every wrapped row, and one row shorter drops the final
    // wrapped row.
    //
    // Unwrapped content keeps its intrinsic width instead, because the
    // surrounding scroll host needs the whole unbroken surface in order to pan
    // across a logical line. Its height stays viewport driven, so tall
    // documents stay scrollable.
    style.width /= if visually_wrapped(config) {
        Dimension::Cells(config.width.max(1))
    } else {
        Dimension::Auto
    };
    style.height /= Dimension::Max;
    style.overflow /= Overflow::Clip;
    let wrap = if !empty && matches!(config.wrap, TextWrap::Soft | TextWrap::Hard) {
        TextWrap::NoWrap
    } else {
        config.wrap
    };
    Text::from_spans(spans).with_style(style).wrap(wrap)
}

#[derive(Debug, Clone, Copy, Default)]
struct VisualRow {
    start: usize,
    /// End of the bytes painted on this row.
    end: usize,
    /// End of the source bytes owned by this row. Separator whitespace
    /// dropped at a wrap boundary lives between `end` and `source_end`.
    source_end: usize,
}

/// The editor's legacy row view, now read from the canonical layout.
///
/// The editor stores its already-normalized value, so the layout's raw-source
/// byte coordinates are the editor's byte coordinates.
fn visual_rows(value: &str, config: EditorConfig) -> Vec<VisualRow> {
    let layout = editor_layout(value, config);
    (0..layout.row_count())
        .map(|index| VisualRow {
            start: layout.row_start(index),
            end: layout.row_end(index),
            source_end: layout.row_source_end(index),
        })
        .collect()
}

/// Build the canonical layout of the editor's current value.
fn editor_layout(value: &str, config: EditorConfig) -> TextLayout {
    crate::basic::text_layout::layout_text(
        &Text::new(value).wrap(config.wrap),
        config.width.max(1) as usize,
        crate::basic::text_layout::ComputedText::default(),
        |parent, _| parent,
        |style| *style,
    )
}

fn sync_state(
    state: &mut EditorState,
    value: Option<String>,
    default_value: Option<String>,
    multiline: bool,
) -> bool {
    let controlled = value.map(|value| normalize(value, multiline));
    if !state.initialized {
        state.value = controlled
            .clone()
            .unwrap_or_else(|| normalize(default_value.unwrap_or_default(), multiline));
        state.cursor = state.value.len();
        state.anchor = state.cursor;
        state.controlled = controlled;
        state.initialized = true;
        return true;
    }

    let mut changed = false;
    match controlled.as_ref() {
        Some(value) => {
            let accepted_pending = state
                .pending_controlled
                .as_ref()
                .is_some_and(|pending| pending.value == *value);
            if accepted_pending {
                let pending = state
                    .pending_controlled
                    .take()
                    .expect("accepted pending edit must exist");
                state.value = value.clone();
                state.cursor = clamp_boundary(&state.value, pending.cursor);
                state.anchor = clamp_boundary(&state.value, pending.anchor);
                changed = true;
            } else {
                // A controlled owner reconciles asynchronously, so renders keep
                // arriving with a value it has already replaced. Both the value
                // the edit was based on and the value it emitted are echoes of
                // that edit; only any other value is a genuine external
                // replacement. Treating an echo as a replacement drops the
                // speculative edit and snaps the caret back, which is what makes
                // a held key insert behind the caret the user sees.
                let pending_replaced = state.pending_controlled.as_ref().is_some_and(|pending| {
                    !state.value.eq(&pending.authoritative) && !value.eq(&pending.authoritative)
                });
                if pending_replaced {
                    state.pending_controlled = None;
                }
                if state.value != *value {
                    state.value = value.clone();
                    changed = true;
                }
                if state.pending_controlled.is_none() {
                    state.cursor = clamp_boundary(&state.value, state.cursor);
                    state.anchor = clamp_boundary(&state.value, state.anchor);
                }
            }
        }
        None => {
            // Switching back to uncontrolled mode retains the most recently rendered
            // authoritative value, not a speculative controlled edit.
            state.pending_controlled = None;
        }
    }
    state.controlled = controlled;
    changed
}

/// Restore the authoritative controlled value after an edit that has not been
/// reconciled by its owner yet, keeping the edit itself in `pending_controlled`.
///
/// `authoritative` is the baseline the edit started from - the value the owner
/// had published - and `state` currently holds the speculative post-edit
/// buffer. The caret must stay where the edit left it: later keystrokes of a
/// held key resume from this snapshot, and restoring a pre-edit caret instead
/// makes them insert *behind* the caret the user sees.
fn hold_controlled_edit(state: &mut EditorState, authoritative: &str) {
    state.pending_controlled = Some(PendingControlledEdit {
        authoritative: authoritative.to_owned(),
        value: state.value.clone(),
        cursor: state.cursor,
        anchor: state.anchor,
    });
    state.value = authoritative.to_owned();
}

/// Snapshot the buffer as a parked speculative edit. `state.value` must hold
/// the speculative buffer here, which is the case in `resume_`/`suspend_`.
fn edit_snapshot(state: &EditorState, authoritative: String) -> PendingControlledEdit {
    PendingControlledEdit {
        authoritative,
        value: state.value.clone(),
        cursor: state.cursor,
        anchor: state.anchor,
    }
}

/// Continue editing a controlled value that has been emitted but has not yet
/// been reconciled by its owner. The authoritative baseline stays available for
/// rendering and is restored by `suspend_pending_controlled_edit`.
fn resume_pending_controlled_edit(state: &mut EditorState) -> Option<String> {
    let pending = state.pending_controlled.take()?;
    let authoritative = pending.authoritative.clone();
    state.value = pending.value;
    state.cursor = pending.cursor;
    state.anchor = pending.anchor;
    Some(authoritative)
}

fn suspend_pending_controlled_edit(state: &mut EditorState, authoritative: String) {
    // The baseline belongs to the owner's last published value, so it survives
    // a speculative edit being parked and resumed.
    let post_edit = edit_snapshot(state, authoritative.clone());
    state.value = authoritative;
    state.cursor = clamp_boundary(&state.value, state.cursor);
    state.anchor = clamp_boundary(&state.value, state.anchor);
    state.pending_controlled = Some(post_edit);
}

fn set_focus(state: &mut EditorState, focused: bool) -> Option<FocusEvent> {
    if state.focused == focused {
        return None;
    }
    state.focused = focused;
    if focused {
        state.ensure_cursor_visible = true;
    } else {
        state.dragging = false;
    }
    Some(if focused {
        FocusEvent::Gained
    } else {
        FocusEvent::Lost
    })
}

fn editor_node(
    cx: &mut ComponentContext,
    mut dom: DomProps,
    config: EditorConfig,
    bindings: EditorBindings,
) -> Node {
    let EditorBindings {
        placeholder,
        value,
        default_value,
        on_change,
        on_submit,
        on_clipboard,
        on_focus_change,
    } = bindings;
    let theme = cx.use_theme();
    let state_ref = cx.use_ref(EditorState::default);
    let (_, redraw) = cx.use_state(|| 0_u64);
    // A parent may clamp the box, so wrap to the content width the last layout
    // actually granted instead of the width that was requested. Both the
    // wrapping and the text surface then use that width, which is what keeps
    // the caret and the pointer in the same coordinate space. The wrapped width
    // sticks even when the configured width is larger: asking for the box back
    // must not put the rows out of step with the viewport again.
    //
    // A resize reaches this component as a fresh measurement, not as a new
    // value or a prop change, so a width that moved since the previous render
    // requests one more render. That render wraps to the new width, and the
    // request stops as soon as the granted width settles.
    let config = {
        let mut state = state_ref.lock().expect("editor state poisoned");
        if state.wrapped_width == 0 {
            state.wrapped_width = config.width.max(1);
        }
        let width = state.wrapped_width;
        let remeasure = state.remeasure;
        state.remeasure = false;
        drop(state);
        if remeasure {
            redraw.update(|value| *value += 1);
        }
        EditorConfig { width, ..config }
    };
    {
        let mut state = state_ref.lock().expect("editor state poisoned");
        let value_changed = sync_state(&mut state, value, default_value, config.multiline);
        let layout = EditorLayout {
            width: config.width,
            height: config.height,
            wrap: config.wrap,
        };
        let layout_changed = state.layout.replace(layout) != Some(layout);
        if config.disabled {
            state.dragging = false;
            state.ensure_cursor_visible = false;
        }
        state.cursor = clamp_boundary(&state.value, state.cursor);
        state.anchor = clamp_boundary(&state.value, state.anchor);
        let rows = visual_rows(&state.value, config);
        let visible_height = config.height.max(1) as usize;
        let visible_width = config.width.max(1) as usize;
        state.scroll_y = state
            .scroll_y
            .min(rows.len().saturating_sub(visible_height));
        let max_width = rows
            .iter()
            .map(|row| row_column(&state.value, *row, row.end))
            .max()
            .unwrap_or(0);
        // Wrapped rows normally fit the viewport, but a box that the parent
        // clamps narrower can still leave their tail off screen. Keep the
        // horizontal offset in range either way so a caret that reaches those
        // cells can pan the viewport to them, exactly like the single line
        // `input` does.
        state.scroll_x = state.scroll_x.min(max_width.saturating_sub(visible_width));
        if state.focused
            && !config.disabled
            && (state.ensure_cursor_visible || layout_changed || value_changed)
        {
            let row_index = visual_row_index(&rows, state.cursor);
            if row_index < state.scroll_y {
                state.scroll_y = row_index;
            } else if row_index >= state.scroll_y.saturating_add(visible_height) {
                state.scroll_y = row_index + 1 - visible_height;
            }
            let row = rows.get(row_index).copied().unwrap_or_default();
            state.scroll_x = scroll_x_for_cursor(
                &state.value,
                row,
                state.cursor,
                state.scroll_x,
                visible_width,
            );
        }
        state.ensure_cursor_visible = false;
    }
    let snapshot = state_ref.lock().expect("editor state poisoned");
    let content = styled_text(&snapshot, config, &theme, &placeholder).on_measure_width(
        std::sync::Arc::new({
            let state_ref = state_ref.clone();
            let redraw = redraw.clone();
            move |width: u16| {
                let mut state = state_ref.lock().expect("editor state poisoned");
                if width > 0 && width != state.wrapped_width {
                    state.wrapped_width = width;
                    state.remeasure = true;
                    drop(state);
                    redraw.update(|value| *value += 1);
                }
            }
        }),
    );
    let scroll_offset = ScrollOffset::new(snapshot.scroll_x as u32, snapshot.scroll_y as u32);
    let focused = snapshot.focused && !config.disabled;
    drop(snapshot);
    if focused {
        dom.style.border.foreground /= theme.colors.ring;
    }

    // Wrapped editors scroll vertically, except when a clamped box leaves part
    // of a row off screen: then the horizontal axis has to stay available so
    // the caret can be brought to those cells. Unwrapped editors pan across the
    // whole logical line, so both axes apply there as well.
    let scroll_axes = if config.multiline {
        ScrollAxes::Both
    } else {
        ScrollAxes::Horizontal
    };
    let scroll_state = state_ref.clone();
    let scroll_redraw = redraw.clone();
    let scroll_event = move |event: ScrollEvent| {
        let mut state = scroll_state.lock().expect("editor state poisoned");
        state.scroll_x = event.offset.x as usize;
        state.scroll_y = event.offset.y as usize;
        state.ensure_cursor_visible = false;
        drop(state);
        scroll_redraw.update(|value| *value += 1);
    };
    let scroll_focus_state = state_ref.clone();
    let scroll_focus_handler = on_focus_change.clone();
    let scroll_focus_redraw = redraw.clone();
    let scroll_focus = move |event: FocusEvent| {
        if event == FocusEvent::Lost {
            let focus_event = set_focus(
                &mut scroll_focus_state.lock().expect("editor state poisoned"),
                false,
            );
            if let Some(event) = focus_event {
                scroll_focus_redraw.update(|value| *value += 1);
                if let Some(handler) = scroll_focus_handler.clone() {
                    handler.call(event);
                }
            }
        }
    };
    let scroll_content = if config.disabled {
        ui! {
            <view style={|style| {
                style.width /= Dimension::Max;
                style.height /= Dimension::Max;
                style.overflow /= Overflow::Clip;
            }}>
                {content}
            </view>
        }
    } else {
        ui! {
            <scroll_area
                axes={scroll_axes}
                scrollbar_visibility={ScrollbarVisibility::Hidden}
                offset={scroll_offset}
                enable_keyboard={false}
                on_scroll={scroll_event}
                on_focus_event={scroll_focus}
                style={|style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Max;
                }}
            >
                {content}
            </scroll_area>
        }
    };

    let redraw_key = redraw.clone();
    let key_state = state_ref.clone();
    let key_change = on_change.clone();
    let key_submit = on_submit.clone();
    let key_clipboard = on_clipboard.clone();
    let key_focus = on_focus_change.clone();
    let key_down = move |event: crate::KeyboardEvent| {
        if config.disabled {
            return;
        }
        event.stop_propagation();
        let mut changed = false;
        let mut value_event = None;
        let mut submit_event = None;
        let mut clipboard_event = None;
        let focus_event;
        let focus_handler = key_focus.clone();
        {
            let mut state = key_state.lock().expect("editor state poisoned");
            focus_event = set_focus(&mut state, true);
            let authoritative = resume_pending_controlled_edit(&mut state);
            let modifiers = event.key.modifiers;
            let shift = modifiers.contains(KeyModifiers::SHIFT);
            let ctrl = modifiers.contains(KeyModifiers::CONTROL);
            let editable =
                !config.read_only && (state.controlled.is_none() || key_change.is_some());
            match event.key.code {
                KeyCode::Left => {
                    let (start, end) = selection(&state);
                    let target = if !shift && start != end {
                        start
                    } else if ctrl {
                        word_left(&state.value, state.cursor)
                    } else {
                        previous_boundary(&state.value, state.cursor)
                    };
                    move_cursor(&mut state, target, shift);
                    state.preferred_column = None;
                    changed = true;
                }
                KeyCode::Right => {
                    let (start, end) = selection(&state);
                    let target = if !shift && start != end {
                        end
                    } else if ctrl {
                        word_right(&state.value, state.cursor)
                    } else {
                        next_boundary(&state.value, state.cursor)
                    };
                    move_cursor(&mut state, target, shift);
                    state.preferred_column = None;
                    changed = true;
                }
                KeyCode::Home => {
                    let target = if ctrl {
                        0
                    } else {
                        let rows = visual_rows(&state.value, config);
                        rows.get(visual_row_index(&rows, state.cursor)).map_or_else(
                            || line_bounds(&state.value, state.cursor).0,
                            |row| row.start,
                        )
                    };
                    move_cursor(&mut state, target, shift);
                    state.preferred_column = None;
                    changed = true;
                }
                KeyCode::End => {
                    let target = if ctrl {
                        state.value.len()
                    } else {
                        let rows = visual_rows(&state.value, config);
                        rows.get(visual_row_index(&rows, state.cursor)).map_or_else(
                            || line_bounds(&state.value, state.cursor).1,
                            |row| row.end,
                        )
                    };
                    move_cursor(&mut state, target, shift);
                    state.preferred_column = None;
                    changed = true;
                }
                KeyCode::Up if config.multiline => {
                    let value = state.value.clone();
                    let cursor = state.cursor;
                    let target =
                        vertical_target(&value, cursor, -1, config, &mut state.preferred_column);
                    move_cursor(&mut state, target, shift);
                    changed = true;
                }
                KeyCode::Down if config.multiline => {
                    let value = state.value.clone();
                    let cursor = state.cursor;
                    let target =
                        vertical_target(&value, cursor, 1, config, &mut state.preferred_column);
                    move_cursor(&mut state, target, shift);
                    changed = true;
                }
                KeyCode::PageUp if config.multiline => {
                    let value = state.value.clone();
                    let cursor = state.cursor;
                    let mut target = cursor;
                    for _ in 0..config.height.max(1) {
                        target = vertical_target(
                            &value,
                            target,
                            -1,
                            config,
                            &mut state.preferred_column,
                        );
                    }
                    move_cursor(&mut state, target, shift);
                    changed = true;
                }
                KeyCode::PageDown if config.multiline => {
                    let value = state.value.clone();
                    let cursor = state.cursor;
                    let mut target = cursor;
                    for _ in 0..config.height.max(1) {
                        target =
                            vertical_target(&value, target, 1, config, &mut state.preferred_column);
                    }
                    move_cursor(&mut state, target, shift);
                    changed = true;
                }
                KeyCode::Char('a' | 'A') if ctrl => {
                    state.anchor = 0;
                    state.cursor = state.value.len();
                    state.preferred_column = None;
                    changed = true;
                }
                KeyCode::Char('c' | 'C') if ctrl => {
                    let (start, end) = selection(&state);
                    if start < end {
                        clipboard_event = Some(TextClipboardEvent {
                            action: TextClipboardAction::Copy,
                            text: state.value[start..end].to_string(),
                        });
                    }
                }
                KeyCode::Char('x' | 'X') if ctrl => {
                    let (start, end) = selection(&state);
                    if start < end && key_clipboard.is_some() && editable {
                        clipboard_event = Some(TextClipboardEvent {
                            action: TextClipboardAction::Cut,
                            text: state.value[start..end].to_string(),
                        });
                        value_event = delete_range(&mut state, start, end)
                            .map(|value| TextValueEvent { value });
                        changed = true;
                    }
                }
                KeyCode::Backspace => {
                    if editable {
                        let (start, end) = selection(&state);
                        let (start, end) = if start < end {
                            (start, end)
                        } else {
                            let previous = if ctrl {
                                word_left(&state.value, state.cursor)
                            } else {
                                previous_boundary(&state.value, state.cursor)
                            };
                            (previous, state.cursor)
                        };
                        value_event = delete_range(&mut state, start, end)
                            .map(|value| TextValueEvent { value });
                        changed = value_event.is_some();
                        state.preferred_column = None;
                    }
                }
                KeyCode::Delete => {
                    if editable {
                        let (start, end) = selection(&state);
                        let (start, end) = if start < end {
                            (start, end)
                        } else {
                            let next = if ctrl {
                                word_right(&state.value, state.cursor)
                            } else {
                                next_boundary(&state.value, state.cursor)
                            };
                            (state.cursor, next)
                        };
                        value_event = delete_range(&mut state, start, end)
                            .map(|value| TextValueEvent { value });
                        changed = value_event.is_some();
                        state.preferred_column = None;
                    }
                }
                KeyCode::Enter if config.multiline && editable => {
                    value_event = replace_selection(&mut state, "\n", config.max_length)
                        .map(|value| TextValueEvent { value });
                    changed = value_event.is_some();
                    state.preferred_column = None;
                }
                KeyCode::Enter if !config.multiline => {
                    submit_event = Some(TextValueEvent {
                        value: state.value.clone(),
                    });
                }
                KeyCode::Char(ch)
                    if !ctrl
                        && !ch.is_control()
                        && !modifiers.intersects(
                            KeyModifiers::ALT
                                | KeyModifiers::SUPER
                                | KeyModifiers::HYPER
                                | KeyModifiers::META,
                        )
                        && editable =>
                {
                    value_event = replace_selection(&mut state, &ch.to_string(), config.max_length)
                        .map(|value| TextValueEvent { value });
                    changed = value_event.is_some();
                    state.preferred_column = None;
                }
                _ => {}
            }
            if state.controlled.is_some() {
                if let Some(authoritative) = authoritative {
                    suspend_pending_controlled_edit(&mut state, authoritative);
                } else if value_event.is_some() {
                    let baseline = state.controlled.clone().unwrap_or_default();
                    hold_controlled_edit(&mut state, &baseline);
                }
            } else if changed {
                state.pending_controlled = None;
            }
            if changed {
                state.ensure_cursor_visible = true;
            }
        }
        if let Some(event) = focus_event
            && let Some(handler) = focus_handler
        {
            handler.call(event);
        }
        if let Some(event) = clipboard_event
            && let Some(handler) = key_clipboard.clone()
        {
            handler.call(event);
        }
        if let Some(event) = value_event
            && let Some(handler) = key_change.clone()
        {
            handler.call(event);
        }
        if let Some(event) = submit_event
            && let Some(handler) = key_submit.clone()
        {
            handler.call(event);
        }
        if changed || focus_event.is_some() {
            redraw_key.update(|value| *value += 1);
        }
    };

    let paste_state = state_ref.clone();
    let paste_change = on_change.clone();
    let paste_focus = on_focus_change.clone();
    let paste_redraw = redraw.clone();
    let paste = move |event: crate::PasteEvent| {
        if config.disabled {
            return;
        }
        let (value_event, focus_event) = {
            let mut state = paste_state.lock().expect("editor state poisoned");
            let focus_event = set_focus(&mut state, true);
            let authoritative = resume_pending_controlled_edit(&mut state);
            if config.read_only || (state.controlled.is_some() && paste_change.is_none()) {
                if let Some(authoritative) = authoritative {
                    suspend_pending_controlled_edit(&mut state, authoritative);
                }
                (None, focus_event)
            } else {
                let value = normalize(event.text, config.multiline);
                let value_event = replace_selection(&mut state, &value, config.max_length)
                    .map(|value| TextValueEvent { value });
                if state.controlled.is_some() {
                    if let Some(authoritative) = authoritative {
                        suspend_pending_controlled_edit(&mut state, authoritative);
                    } else if value_event.is_some() {
                        let baseline = state.controlled.clone().unwrap_or_default();
                        hold_controlled_edit(&mut state, &baseline);
                    }
                }
                if value_event.is_some() {
                    state.ensure_cursor_visible = true;
                }
                (value_event, focus_event)
            }
        };
        if let Some(event) = focus_event
            && let Some(handler) = paste_focus.clone()
        {
            handler.call(event);
        }
        let value_changed = value_event.is_some();
        if let Some(event) = value_event
            && let Some(handler) = paste_change.clone()
        {
            handler.call(event);
        }
        if value_changed || focus_event.is_some() {
            paste_redraw.update(|value| *value += 1);
        }
    };

    let pointer_state = state_ref.clone();
    let pointer_focus = on_focus_change.clone();
    let pointer_redraw = redraw.clone();
    let pointer_down = move |event: crate::PointerEvent| {
        if config.disabled || !event.is_primary_button() {
            return;
        }
        let mut state = pointer_state.lock().expect("editor state poisoned");
        let focus_event = set_focus(&mut state, true);
        let target = pointer_index(
            &state.value,
            event.local_position.column.max(0) as usize,
            event.local_position.line.max(0) as usize + state.scroll_y,
            state.scroll_x,
            config,
        );
        if event.modifiers.contains(KeyModifiers::SHIFT) {
            state.cursor = target;
        } else {
            state.anchor = target;
            state.cursor = target;
        }
        state.preferred_column = None;
        state.pending_controlled = None;
        state.dragging = true;
        state.ensure_cursor_visible = true;
        drop(state);
        if let Some(event) = focus_event
            && let Some(handler) = pointer_focus.clone()
        {
            handler.call(event);
        }
        pointer_redraw.update(|value| *value += 1);
    };
    let move_state = state_ref.clone();
    let move_redraw = redraw.clone();
    let pointer_move = move |event: crate::PointerEvent| {
        if event.is_primary_button() || event.buttons & 1 != 0 {
            let mut state = move_state.lock().expect("editor state poisoned");
            if state.dragging {
                let local_line = event.local_position.line;
                let local_column = event.local_position.column;
                let visible_height = config.height.max(1);
                let visible_width = config.width.max(1);
                if config.multiline && local_line < 0 {
                    state.scroll_y = state.scroll_y.saturating_sub(1);
                } else if config.multiline && local_line >= i32::from(visible_height) {
                    state.scroll_y = state.scroll_y.saturating_add(1);
                }
                if !visually_wrapped(config) && local_column < 0 {
                    state.scroll_x = state.scroll_x.saturating_sub(1);
                } else if !visually_wrapped(config) && local_column >= i32::from(visible_width) {
                    state.scroll_x = state.scroll_x.saturating_add(1);
                }
                let hit_column = local_column.clamp(0, i32::from(visible_width)) as usize;
                let hit_line = local_line.clamp(0, i32::from(visible_height) - 1) as usize;
                state.cursor = pointer_index(
                    &state.value,
                    hit_column,
                    hit_line + state.scroll_y,
                    state.scroll_x,
                    config,
                );
                state.pending_controlled = None;
                // Edge dragging explicitly moves the viewport one step above. Do not
                // let normal caret tracking turn that into a jump to the document edge.
                state.ensure_cursor_visible = false;
                move_redraw.update(|value| *value += 1);
            }
        }
    };
    let up_state = state_ref.clone();
    let pointer_up = move |_event: crate::PointerEvent| {
        up_state.lock().expect("editor state poisoned").dragging = false;
    };
    let cancel_state = state_ref.clone();
    let pointer_cancel = move |_event: crate::PointerEvent| {
        cancel_state.lock().expect("editor state poisoned").dragging = false;
    };
    let key_up = move |event: crate::KeyboardEvent| {
        if !config.disabled {
            event.stop_propagation();
        }
    };
    let focus_state = state_ref.clone();
    let focus_redraw = redraw.clone();
    let focus = move |event: FocusEvent| {
        if event == FocusEvent::Lost {
            let focus_event = set_focus(
                &mut focus_state.lock().expect("editor state poisoned"),
                false,
            );
            if let Some(event) = focus_event {
                focus_redraw.update(|value| *value += 1);
                if let Some(handler) = on_focus_change.clone() {
                    handler.call(event);
                }
            }
        }
    };

    let caller_events = dom.events.clone();
    let mut inner_dom = dom;
    inner_dom.events = EventHandlers::default();
    // The editor's inner host is the keyboard focus target; it is focusable
    // exactly when the control is enabled.
    inner_dom.focusable = !config.disabled;
    let outer_dom = DomProps {
        events: caller_events,
        ..DomProps::default()
    };
    if config.disabled {
        ui! {
            <view dom={outer_dom}>
                <view key="disabled-editor" dom={inner_dom}>{scroll_content}</view>
            </view>
        }
    } else {
        ui! {
            <view dom={outer_dom}>
                <view key="enabled-editor" dom={inner_dom}
                    on_key_down={key_down}
                    on_key_up={key_up}
                    on_paste_event={paste}
                    on_pointer_down={pointer_down}
                    on_pointer_move={pointer_move}
                    on_pointer_up={pointer_up}
                    on_pointer_cancel={pointer_cancel}
                    on_focus_event={focus}
                >
                    {scroll_content}
                </view>
            </view>
        }
    }
}

fn pointer_index(
    value: &str,
    column: usize,
    line: usize,
    scroll_x: usize,
    config: EditorConfig,
) -> usize {
    let rows = visual_rows(value, config);
    let line_index = line.min(rows.len().saturating_sub(1));
    let selected_row = rows.get(line_index).copied().unwrap_or_default();
    let line_start = selected_row.start;
    let selected = &value[selected_row.start..selected_row.end];
    // `column` is relative to the scrolled viewport, so shift it by the offset
    // the viewport is displaying before mapping it into the row's cells.
    let cell = column.saturating_add(scroll_x);
    line_start + boundary_after_cell(selected, cell, logical_column_at(value, line_start))
}

fn base_style(theme: &Theme, multiline: bool, width: u16, height: u16) -> Style {
    let mut style = Style::default();
    let border_edges = if multiline {
        Edges::all(true)
    } else {
        Edges {
            top: false,
            right: false,
            bottom: true,
            left: false,
        }
    };
    // `width` and `height` describe the editable content box. Every inset the
    // style adds - horizontal padding, vertical padding and the border - is
    // therefore laid out around those cells, never taken out of them. The
    // editor wraps its content to exactly this box and renders it for exactly
    // `height` rows, so any inset that the border box does not pay for shortens
    // the internal viewport and silently clips the final wrapped row.
    //
    // Vertical padding only exists to keep the text off the bottom edge, which
    // the editor already does for itself. The single line `input` - whose only
    // edge is its underline - therefore stays exactly one content row tall
    // above the rule, and a disabled editor keeps its compact box.
    let horizontal_padding = 2u16;
    let vertical_padding = if border_edges.top { 2u16 } else { 0u16 };
    let outer_width = width
        .saturating_add(horizontal_padding)
        .saturating_add(u16::from(border_edges.left))
        .saturating_add(u16::from(border_edges.right));
    let outer_height = height
        .saturating_add(vertical_padding)
        .saturating_add(u16::from(border_edges.top))
        .saturating_add(u16::from(border_edges.bottom));
    style.layout /= Layout::Vertical;
    style.gap /= 0;
    style.justify /= Justify::Start;
    style.align /= Align::Start;
    style.width /= Dimension::Cells(outer_width.max(1));
    style.height /= Dimension::Cells(outer_height.max(1));
    style.background /= theme.colors.input;
    style.text.foreground /= theme.colors.foreground;
    style.padding /= Edges::symmetric(0, 1);
    style.border.kind /= theme.borders.kind;
    style.border.edges /= border_edges;
    style.border.foreground /= theme.colors.border;
    style.border.background /= theme.colors.input;
    style.overflow /= Overflow::Clip;
    style.overflow_x /= Overflow::Clip;
    style.overflow_y /= Overflow::Clip;
    style
}

fn editor_dom(props: &DomProps, owned: &Style) -> DomProps {
    let mut dom = DomProps {
        style: owned.clone(),
        ..DomProps::default()
    };
    dom = dom.with_overrides(props);
    dom.style.layout = owned.layout;
    dom.style.width = owned.width;
    dom.style.height = owned.height;
    dom.style.padding = owned.padding;
    dom.style.gap = owned.gap;
    dom.style.justify = owned.justify;
    dom.style.align = owned.align;
    dom.style.overflow = owned.overflow;
    dom.style.overflow_x = owned.overflow_x;
    dom.style.overflow_y = owned.overflow_y;
    dom.style.border.edges = owned.border.edges;
    dom
}

/// Render a focused, single-line editable text control.
///
/// This component owns its text presentation; children supplied through the
/// generic component API are intentionally ignored.
pub fn input(cx: &mut ComponentContext, props: &Props<InputProps>) -> Node {
    let theme = cx.use_theme();
    let width = props.width | 24;
    let style = base_style(&theme, false, width, 1);
    let dom = editor_dom(&props.dom, &style);
    editor_node(
        cx,
        dom,
        EditorConfig {
            multiline: false,
            width,
            height: 1,
            wrap: TextWrap::NoWrap,
            max_length: props.max_length.as_ref().copied(),
            disabled: props.disabled | false,
            read_only: props.read_only | false,
        },
        EditorBindings {
            placeholder: props.placeholder.clone() | String::new(),
            value: props.value.as_ref().cloned(),
            default_value: props.default_value.as_ref().cloned(),
            on_change: props.on_change.as_ref().cloned(),
            on_submit: props.on_submit.as_ref().cloned(),
            on_clipboard: props.on_clipboard.as_ref().cloned(),
            on_focus_change: props.on_focus_change.as_ref().cloned(),
        },
    )
}

/// Render a focused, multiline editable text control.
///
/// This component owns its text presentation; children supplied through the
/// generic component API are intentionally ignored. `width` and `height` are
/// the editable content box: the surrounding padding and border are laid out
/// around them, and every wrapped row inside that box stays visible. Text wraps
/// hard by default, keeping unbreakable tokens inside the editable width; set
/// [`TextWrap::NoWrap`] explicitly for horizontal scrolling.
pub fn text_area(cx: &mut ComponentContext, props: &Props<TextAreaProps>) -> Node {
    let theme = cx.use_theme();
    let width = props.width | 40;
    let height = props.height | 5;
    let style = base_style(&theme, true, width, height);
    let dom = editor_dom(&props.dom, &style);
    editor_node(
        cx,
        dom,
        EditorConfig {
            multiline: true,
            width,
            height,
            wrap: props.wrap | TextWrap::Hard,
            max_length: props.max_length.as_ref().copied(),
            disabled: props.disabled | false,
            read_only: props.read_only | false,
        },
        EditorBindings {
            placeholder: props.placeholder.clone() | String::new(),
            value: props.value.as_ref().cloned(),
            default_value: props.default_value.as_ref().cloned(),
            on_change: props.on_change.as_ref().cloned(),
            on_submit: None,
            on_clipboard: props.on_clipboard.as_ref().cloned(),
            on_focus_change: props.on_focus_change.as_ref().cloned(),
        },
    )
}

/// Temporary parity support for the canonical layout migration.
#[cfg(test)]
pub(crate) fn legacy_visual_rows_for_test(
    value: &str,
    wrap: TextWrap,
    width: u16,
) -> Vec<(usize, usize, usize)> {
    let config = EditorConfig {
        multiline: true,
        width,
        height: 1,
        wrap,
        max_length: None,
        disabled: false,
        read_only: false,
    };
    // The editor lays out its normalized value, so the parity baseline must
    // normalize first as well.
    let normalized = normalize(value.to_owned(), true);
    visual_rows(&normalized, config)
        .into_iter()
        .map(|row| (row.start, row.end, row.source_end))
        .collect()
}
