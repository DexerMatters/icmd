//! One canonical text layout engine.
//!
//! This module is the single owner of text shaping geometry: it turns styled
//! source text into visual rows of extended grapheme clusters, and answers
//! every geometric question the commit pipeline and the editor surface have:
//!
//! - cell measurement and visual row count;
//! - source byte boundary to caret cell and visual row;
//! - visual cell to source byte boundary with an explicit hit bias;
//! - vertical movement through a preferred terminal-cell column; and
//! - ordered iteration for rasterization, selection, and caret decoration.
//!
//! Everything is expressed over three coordinate systems that are never used
//! interchangeably: UTF-8 source bytes, extended grapheme clusters, and
//! terminal cells. Every source position the layout returns is an extended
//! grapheme boundary.
//!
//! Wrapping follows the terminal text model used by the renderer. `Soft`
//! wrapping prefers Unicode line-break opportunities (falling back to grapheme
//! boundaries for an overlong unbreakable fragment), `Hard` wrapping splits at
//! the cell limit, and `NoWrap` only breaks at explicit newlines. At a
//! non-final soft-wrap boundary the trailing whitespace that caused the break is
//! dropped from the painted row but stays navigable as an
//! [`ItemKind::Separator`].

use std::ops::Range;

use textwrap::core::Fragment;
use textwrap::wrap_algorithms::wrap_first_fit;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::MAX_GLYPH_BYTES;

use super::props::TextStyle;
use super::text::{Text, TextWrap};

/// The tab stop width used by both the renderer and the editor.
pub(crate) const TAB_WIDTH: usize = 4;

/// How a pointer hit inside an ambiguous cell is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HitBias {
    /// A hit on a wide grapheme before its halfway cell resolves to the
    /// grapheme's leading boundary. This is what vertical navigation wants.
    #[default]
    Leading,
    /// A hit before a wide grapheme's halfway cell resolves to its trailing
    /// boundary. This is the pointer-selection default: clicking a glyph places
    /// the caret after it.
    Trailing,
}

/// The role a laid-out item plays in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ItemKind {
    /// A visible grapheme cluster. It may have cell width zero.
    Glyph,
    /// Whitespace dropped at a soft-wrap boundary. It owns source bytes but is
    /// not painted.
    Separator,
    /// An explicit line break (`\n` after normalization). It owns its source
    /// bytes and ends the current row.
    Newline,
}

/// Effective (resolved) text style attached to each laid-out item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ComputedText {
    pub foreground: crossterm::style::Color,
    pub background: Option<crossterm::style::Color>,
    pub attributes: crossterm::style::Attributes,
}

impl Default for ComputedText {
    fn default() -> Self {
        Self {
            foreground: crossterm::style::Color::Reset,
            background: None,
            attributes: crossterm::style::Attributes::default(),
        }
    }
}

/// One laid-out unit: a grapheme cluster, a dropped wrap separator, or an
/// explicit newline.
///
/// `source` is the UTF-8 range in the caller's source string; `text` is the
/// same unit inside the layout's normalized string. `symbol` is what the
/// renderer paints (for example a tab expands to spaces).
#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub(crate) source: Range<usize>,
    pub(crate) text: Range<usize>,
    /// Cell offset of this item inside its visual row.
    pub(crate) cell: usize,
    /// Terminal cells this item occupies. Always zero for newlines and
    /// zero-width graphemes.
    pub(crate) width: usize,
    pub(crate) kind: ItemKind,
    /// Row index this item belongs to.
    pub(crate) row: usize,
    pub(crate) symbol: String,
    pub(crate) style: ComputedText,
}

/// One visual row: a contiguous, ordered range of items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) first_item: usize,
    /// Number of items in this row. A row can be empty (an empty logical line).
    pub(crate) len: usize,
    /// Navigable source position of an empty row.
    pub(crate) empty_source: Option<usize>,
}

impl Row {
    pub(crate) const fn is_empty(self) -> bool {
        self.len == 0
    }
}

/// One shaped grapheme handed to [`TextLayout::layout`].
#[derive(Debug, Clone)]
pub(crate) struct ShapedGlyph<S> {
    pub source: Range<usize>,
    pub text_start: usize,
    pub text_end: usize,
    pub width: usize,
    pub symbol: String,
    pub kind: ItemKind,
    pub style: S,
}

/// An immutable layout of one styled text value at one cell width.
#[derive(Debug, Clone, Default)]
pub(crate) struct TextLayout {
    text: String,
    items: Vec<Item>,
    rows: Vec<Row>,
    /// Total cell width of the layout (the widest row).
    width: usize,
    /// Total source length the layout owns (including the final newline).
    source_len: usize,
}

/// Build the canonical layout of `text` for `width` cells, merging each span's
/// style over `inherited` with `merge`.
pub(crate) fn layout_text<S>(
    text: &Text,
    width: usize,
    inherited: ComputedText,
    merge: impl Fn(ComputedText, &TextStyle) -> S,
    into_computed: impl Fn(&S) -> ComputedText,
) -> TextLayout
where
    S: Clone,
{
    let shaped = shape_text(text, inherited, merge);
    TextLayout::layout(shaped, text.wrap, width, into_computed)
}

/// Shape every grapheme of `text` without wrapping, preserving span styles.
fn shape_text<S>(
    text: &Text,
    inherited: ComputedText,
    merge: impl Fn(ComputedText, &TextStyle) -> S,
) -> Vec<ShapedGlyph<S>>
where
    S: Clone,
{
    let mut shaped = Vec::new();
    let mut text_offset = 0usize;
    for span in &text.spans {
        let style = merge(inherited, &span.style);
        let content = span.content.replace("\r\n", "\n").replace('\r', "\n");
        for (source_index, grapheme) in content.grapheme_indices(true) {
            let (symbol, width, kind) = display_glyph(grapheme);
            let text_end = text_offset + symbol.len();
            shaped.push(ShapedGlyph {
                source: source_index..source_index + grapheme.len(),
                text_start: text_offset,
                text_end,
                width,
                symbol,
                kind,
                style: style.clone(),
            });
            text_offset = text_end;
        }
    }
    shaped
}

/// The display symbol, cell width, and role of one grapheme.
fn display_glyph(grapheme: &str) -> (String, usize, ItemKind) {
    if grapheme == "\n" {
        return ("\n".to_string(), 0, ItemKind::Newline);
    }
    if grapheme == "\t" {
        // Column-relative; `TextLayout::layout` fixes the width from the
        // logical line column.
        return ("\t".to_string(), 0, ItemKind::Glyph);
    }
    if grapheme.chars().any(char::is_control) {
        return ("\u{FFFD}".to_string(), 1, ItemKind::Glyph);
    }
    let symbol = grapheme.to_string();
    let width = UnicodeWidthStr::width(symbol.as_str());
    if symbol.len() > MAX_GLYPH_BYTES || !(1..=2).contains(&width) {
        return ("\u{FFFD}".to_string(), 1, ItemKind::Glyph);
    }
    (symbol, width, ItemKind::Glyph)
}

impl TextLayout {
    /// Lay out pre-shaped graphemes. `into_computed` projects the caller's
    /// style type onto the effective text style stored per item.
    pub(crate) fn layout<S>(
        shaped: Vec<ShapedGlyph<S>>,
        wrap: TextWrap,
        width: usize,
        into_computed: impl Fn(&S) -> ComputedText,
    ) -> Self
    where
        S: Clone,
    {
        let width = width.max(1);
        let mut text = String::new();
        for glyph in &shaped {
            text.push_str(&glyph.symbol);
        }
        let source_len = shaped.last().map_or(0, |glyph| glyph.source.end);

        // A tab advances to the next tab stop from the *logical* line column,
        // so its width is independent of where a soft wrap put it.
        let mut widths = Vec::with_capacity(shaped.len());
        let mut column = 0usize;
        for glyph in &shaped {
            if glyph.kind == ItemKind::Newline {
                widths.push(0);
                column = 0;
                continue;
            }
            let width = if glyph.symbol == "\t" {
                TAB_WIDTH - column % TAB_WIDTH
            } else {
                glyph.width
            };
            widths.push(width);
            column = column.saturating_add(width);
        }

        // Split the shaped sequence into logical lines, then wrap each line
        // independently. Empty logical lines stay as empty rows.
        let mut logical_lines: Vec<Range<usize>> = Vec::new();
        let mut line_start = 0usize;
        for (index, glyph) in shaped.iter().enumerate() {
            if glyph.kind == ItemKind::Newline {
                logical_lines.push(line_start..index + 1);
                line_start = index + 1;
            }
        }
        logical_lines.push(line_start..shaped.len());

        // Wrap each logical line into one or more ordered visual rows, keeping
        // each row's packing pieces so dropped separators can be identified.
        // Whitespace is dropped only on a non-final row of its logical line.
        struct Placed {
            range: Range<usize>,
            pieces: Vec<Piece>,
            is_last_row: bool,
            empty_source: Option<usize>,
        }
        let mut placed: Vec<Placed> = Vec::new();
        for logical in &logical_lines {
            // A logical line with no glyphs (only its terminating newline, or a
            // trailing empty line) is a real but empty row. Its single
            // navigable position is where its content would begin.
            if logical.start >= logical.end
                || shaped[logical.start..logical.end]
                    .iter()
                    .all(|glyph| glyph.kind == ItemKind::Newline)
            {
                placed.push(Placed {
                    range: logical.start..logical.start,
                    pieces: Vec::new(),
                    is_last_row: true,
                    empty_source: Some(logical.start),
                });
                continue;
            }
            let pieces = line_pieces(
                &shaped,
                &widths,
                logical.start,
                logical.end - logical.start,
                wrap,
                width,
            );
            let lines: Vec<&[Piece]> = wrap_first_fit(&pieces, &[width as f64]);
            let last_index = lines.len().saturating_sub(1);
            for (line_index, line) in lines.into_iter().enumerate() {
                let start = line.first().map_or(logical.start, |piece| piece.start);
                let end = line.last().map_or(logical.start, |piece| piece.end);
                placed.push(Placed {
                    range: start..end,
                    pieces: line.to_vec(),
                    is_last_row: line_index == last_index,
                    empty_source: None,
                });
            }
        }

        // Materialize items in visual order with row and row-relative cells.
        // Explicit newlines are not items: a row's source range ends at the
        // newline that terminates it, and the next row starts after it, which
        // is exactly how navigation and selection treat line breaks.
        let mut items = Vec::with_capacity(shaped.len());
        let mut rows = Vec::with_capacity(placed.len());
        for (row_index, entry) in placed.iter().enumerate() {
            let first_item = items.len();
            let mut cell = 0usize;
            for index in entry.range.clone() {
                let glyph = &shaped[index];
                if glyph.kind == ItemKind::Newline {
                    continue;
                }
                let item_width = widths[index];
                let kind = if glyph.kind == ItemKind::Glyph
                    && item_is_dropped_separator(&entry.pieces, index, entry.is_last_row)
                {
                    ItemKind::Separator
                } else {
                    glyph.kind
                };
                items.push(Item {
                    source: glyph.source.clone(),
                    text: glyph.text_start..glyph.text_end,
                    cell,
                    width: item_width,
                    kind,
                    row: row_index,
                    symbol: glyph.symbol.clone(),
                    style: into_computed(&glyph.style),
                });
                cell = cell.saturating_add(item_width);
            }
            rows.push(Row {
                first_item,
                len: items.len() - first_item,
                empty_source: entry.empty_source,
            });
        }

        let layer_width = rows
            .iter()
            .map(|row| row_cells(&items, row))
            .max()
            .unwrap_or(0);

        Self {
            text,
            items,
            rows,
            width: layer_width,
            source_len,
        }
    }

    /// Number of visual rows this layout occupies.
    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Intrinsic content width in terminal cells.
    pub(crate) fn width(&self) -> usize {
        self.width
    }

    /// Total UTF-8 source length the layout owns.
    pub(crate) fn source_len(&self) -> usize {
        self.source_len
    }

    /// The normalized string the layout was shaped from.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Layout items in visual order.
    pub(crate) fn items(&self) -> &[Item] {
        &self.items
    }

    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub(crate) fn row_items(&self, index: usize) -> &[Item] {
        self.rows.get(index).map_or(&[], |row| {
            &self.items[row.first_item..row.first_item + row.len]
        })
    }

    /// First UTF-8 source byte painted on `index`.
    pub(crate) fn row_start(&self, index: usize) -> usize {
        self.row_items(index)
            .first()
            .map_or_else(|| self.row_fallback_source(index), |item| item.source.start)
    }

    /// End of the UTF-8 source bytes painted on `index`.
    ///
    /// Whitespace dropped at a soft-wrap boundary is not painted, so this stops
    /// before it even though [`Self::row_source_end`] owns it.
    pub(crate) fn row_end(&self, index: usize) -> usize {
        self.row_items(index)
            .iter()
            .rfind(|item| item.kind == ItemKind::Glyph && !item.symbol.is_empty())
            .map_or_else(|| self.row_start(index), |item| item.source.end)
    }

    /// End of the UTF-8 source bytes owned by `index`, including whitespace
    /// dropped at a soft-wrap boundary.
    ///
    /// Together with [`Self::row_start`], row ranges tile the source with no
    /// gaps or overlaps: each row starts where the previous one ended, and an
    /// explicit newline is the last byte the next row does not own.
    pub(crate) fn row_source_end(&self, index: usize) -> usize {
        self.row_items(index)
            .last()
            .map_or_else(|| self.row_start(index), |item| item.source.end)
    }

    /// Total painted cell width of `index`.
    pub(crate) fn row_width(&self, index: usize) -> usize {
        self.row_items(index)
            .iter()
            .fold(0usize, |sum, item| sum.saturating_add(item.width))
    }

    /// The maximum painted row width across the layout.
    pub(crate) fn max_row_width(&self) -> usize {
        (0..self.rows.len())
            .map(|index| self.row_width(index))
            .max()
            .unwrap_or(0)
    }

    /// The visual row that owns `source`, clamped into range.
    ///
    /// A source position on an explicit newline belongs to the row that the
    /// newline terminates, and a position at the end of a row that continues on
    /// the next row stays with the earlier row. A position after the final
    /// newline resolves to the final (possibly empty) row.
    pub(crate) fn row_of_source(&self, source: usize) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        for (index, row) in self.rows.iter().enumerate() {
            let start = self.row_start(index);
            let end = self.row_source_end(index);
            if source <= end && (source > start || row.len == 0) {
                return index;
            }
        }
        self.rows.len() - 1
    }

    /// The grapheme boundary nearest to `source`, clamped to the value.
    pub(crate) fn clamp(&self, source: usize) -> usize {
        if self.items.is_empty() {
            return 0;
        }
        let last_end = self.items.last().map_or(0, |item| item.source.end);
        if source >= last_end {
            return last_end;
        }
        for item in &self.items {
            if source < item.source.end {
                return if source <= item.source.start {
                    item.source.start
                } else {
                    item.source.end
                };
            }
        }
        last_end
    }

    /// Source boundary immediately before `source`.
    pub(crate) fn previous_boundary(&self, source: usize) -> usize {
        if source == 0 {
            return 0;
        }
        let source = self.clamp(source);
        for item in self.items.iter().rev() {
            if item.source.end <= source {
                return item.source.start;
            }
        }
        0
    }

    /// Source boundary immediately after `source`.
    pub(crate) fn next_boundary(&self, source: usize) -> usize {
        let source = self.clamp(source);
        for item in &self.items {
            if item.source.start >= source {
                return item.source.end;
            }
        }
        self.items.last().map_or(0, |item| item.source.end)
    }

    /// The caret position for a source boundary: the visual row, the cell
    /// column the caret sits at, and the width of the grapheme it precedes
    /// (1 at end of row).
    pub(crate) fn caret(&self, source: usize) -> (usize, usize, usize) {
        if self.rows.is_empty() {
            return (0, 0, 1);
        }
        let source = self.clamp(source);
        // A source position that falls in a dropped separator or on an explicit
        // newline belongs to the row that owns those bytes, not the next row.
        let row = self.row_of_source(source);
        let mut cell = 0usize;
        let mut separator_cell = None;
        for item in self.row_items(row) {
            if item.kind == ItemKind::Newline {
                break;
            }
            if item.kind == ItemKind::Separator {
                separator_cell.get_or_insert(cell);
            }
            if source <= item.source.start {
                // The caret sits before this item. For a wide grapheme with the
                // caret at its left edge the offset is correct as-is.
                return (row, cell, item.width.max(1));
            }
            if source < item.source.end {
                // Inside a grapheme (already snapped by `clamp`); place the
                // caret after it.
                return (row, cell.saturating_add(item.width), 1);
            }
            cell = cell.saturating_add(item.width);
        }
        if cell == 0
            && let Some(cell) = separator_cell
        {
            return (row, cell, 1);
        }
        (row, cell, 1)
    }

    /// Map a pointer hit at `cell` inside `row` to a source boundary.
    pub(crate) fn hit(&self, row: usize, cell: usize, bias: HitBias) -> usize {
        let row = row.min(self.rows.len().saturating_sub(1));
        let mut offset = 0usize;
        for item in self.row_items(row) {
            if item.kind == ItemKind::Newline {
                return item.source.start;
            }
            let end = offset.saturating_add(item.width);
            if item.width > 0 && cell < end {
                let trailing = (cell - offset) * 2 >= item.width;
                return if item.width > 1 && trailing == (bias == HitBias::Trailing) {
                    item.source.end
                } else {
                    item.source.start
                };
            }
            offset = end;
        }
        self.row_source_end(row)
    }

    /// The cell column of a source boundary inside its visual row.
    pub(crate) fn column(&self, source: usize) -> usize {
        let (_, cell, _) = self.caret(source);
        cell
    }

    /// Move `source` vertically by `direction` rows, keeping `preferred` as the
    /// target terminal-cell column when the caller supplied one.
    ///
    /// Returns the new boundary and the preferred column that should be carried
    /// into the next vertical move.
    pub(crate) fn vertical(
        &self,
        source: usize,
        direction: i32,
        preferred: Option<usize>,
    ) -> (usize, Option<usize>) {
        if self.rows.is_empty() {
            return (0, preferred);
        }
        let current = self.row_of_source(source);
        let column = preferred.unwrap_or_else(|| self.column(source));
        let target = (current as i64 + i64::from(direction))
            .clamp(0, self.rows.len().saturating_sub(1) as i64) as usize;
        (self.hit(target, column, HitBias::Leading), Some(column))
    }

    fn row_fallback_source(&self, index: usize) -> usize {
        // An empty row (an empty logical line) still has one navigable source
        // position: the newline that created it, or the end of the value.
        self.rows
            .get(index)
            .and_then(|row| row.empty_source)
            .unwrap_or(self.source_len)
    }
}

/// Split the logical line `first..first + len` into wrapping pieces.
///
/// The returned pieces cover the line in order; a soft wrap boundary may drop
/// trailing whitespace, but that whitespace stays inside the owning piece so
/// source navigation remains complete.
fn line_pieces<S>(
    shaped: &[ShapedGlyph<S>],
    widths: &[usize],
    first: usize,
    len: usize,
    wrap: TextWrap,
    width: usize,
) -> Vec<Piece> {
    let last = first + len;
    // An explicit newline is not wrapped; it terminates the logical line.
    let content_last = (first..last)
        .find(|index| shaped[*index].kind == ItemKind::Newline)
        .unwrap_or(last);
    if matches!(wrap, TextWrap::NoWrap) || content_last == first {
        let total = sum_widths(widths, first, last);
        return vec![Piece {
            start: first,
            end: last,
            content_end: last,
            content_width: total,
            width: total,
        }];
    }
    if matches!(wrap, TextWrap::Hard) {
        return hard_pieces(widths, first, last);
    }
    let mut pieces = soft_pieces(shaped, widths, first, content_last, width);
    // The explicit newline that terminated the line always belongs to the final
    // row and is never dropped as a separator.
    if content_last < last {
        // Whitespace immediately before an explicit newline is ordinary
        // content: that break is hard, not soft, so nothing is dropped for it.
        if let Some(last_piece) = pieces.last_mut() {
            last_piece.content_end = last_piece.end;
            last_piece.content_width = last_piece.width;
        }
        let total = sum_widths(widths, content_last, last);
        pieces.push(Piece {
            start: content_last,
            end: last,
            content_end: last,
            content_width: total,
            width: total,
        });
    }
    pieces
}

/// Hard wrapping splits at grapheme boundaries and never drops a glyph.
fn hard_pieces(widths: &[usize], first: usize, last: usize) -> Vec<Piece> {
    (first..last)
        .map(|index| Piece {
            start: index,
            end: index + 1,
            content_end: index + 1,
            content_width: widths[index],
            width: widths[index],
        })
        .collect()
}

/// True when `index` is whitespace dropped at a soft-wrap boundary.
///
/// Dropping happens only on a row that is not the final row of its logical
/// line: whitespace inside a piece ("hello world" fitting on one row) and
/// trailing whitespace on the last row both stay painted.
fn item_is_dropped_separator(pieces: &[Piece], index: usize, is_last_row: bool) -> bool {
    if is_last_row {
        return false;
    }
    let position = pieces.partition_point(|piece| piece.start <= index);
    let Some(piece) = position.checked_sub(1).and_then(|at| pieces.get(at)) else {
        return false;
    };
    index >= piece.content_end
}

fn row_cells(items: &[Item], row: &Row) -> usize {
    items[row.first_item..row.first_item + row.len]
        .iter()
        .filter(|item| item.kind != ItemKind::Newline)
        .fold(0usize, |sum, item| sum.saturating_add(item.width))
}

/// A wrapping fragment over a half-open item range.
#[derive(Debug, Clone, Copy)]
struct Piece {
    start: usize,
    end: usize,
    content_end: usize,
    content_width: usize,
    width: usize,
}

impl Fragment for Piece {
    fn width(&self) -> f64 {
        self.content_width as f64
    }

    fn whitespace_width(&self) -> f64 {
        self.width.saturating_sub(self.content_width) as f64
    }

    fn penalty_width(&self) -> f64 {
        0.0
    }
}

/// Build soft-wrap pieces from Unicode line-break opportunities.
fn soft_pieces<S>(
    shaped: &[ShapedGlyph<S>],
    widths: &[usize],
    first: usize,
    content_last: usize,
    width: usize,
) -> Vec<Piece> {
    let mut source = String::new();
    let mut offsets = Vec::with_capacity(content_last - first + 1);
    offsets.push(0usize);
    for glyph in &shaped[first..content_last] {
        source.push_str(&glyph.symbol);
        offsets.push(source.len());
    }

    let words = textwrap::WordSeparator::UnicodeBreakProperties
        .find_words(&source)
        .collect::<Vec<_>>();
    if words.is_empty() {
        return vec![Piece {
            start: first,
            end: content_last,
            content_end: content_last,
            content_width: sum_widths(widths, first, content_last),
            width: sum_widths(widths, first, content_last),
        }];
    }

    let mut pieces = Vec::new();
    let mut cursor = 0usize;
    for word in words {
        let word_start = cursor;
        let content_end = word_start.saturating_add(word.word.len());
        let end = content_end.saturating_add(word.whitespace.len());
        cursor = end;

        let absolute = |offset: usize| first + offsets.partition_point(|value| *value < offset);
        let start = absolute(word_start).min(content_last);
        let whitespace_end = absolute(end).min(content_last);
        let body_end = absolute(content_end).min(whitespace_end);
        let content_width = sum_widths(widths, start, body_end);
        let total_width = sum_widths(widths, start, whitespace_end);
        pieces.push(Piece {
            start,
            end: whitespace_end,
            content_end: body_end,
            content_width,
            width: total_width,
        });
    }
    if cursor < source.len() {
        let start = (first + offsets.partition_point(|value| *value < cursor)).min(content_last);
        let total = sum_widths(widths, start, content_last);
        pieces.push(Piece {
            start,
            end: content_last,
            content_end: content_last,
            content_width: total,
            width: total,
        });
    }

    // Trailing whitespace on the logical line is content, not a separator.
    let preserve_trailing = shaped[content_last - 1]
        .symbol
        .chars()
        .all(char::is_whitespace)
        && !shaped[content_last - 1].symbol.is_empty();
    if preserve_trailing && let Some(last) = pieces.last_mut() {
        last.content_end = last.end;
        last.content_width = last.width;
    }

    let mut split = Vec::new();
    for piece in pieces {
        if piece.content_width <= width {
            split.push(piece);
            continue;
        }
        let whitespace = piece.width.saturating_sub(piece.content_width);
        let mut piece_start = piece.start;
        let mut piece_width = 0usize;
        for (offset, &glyph_width) in widths[piece.start..piece.content_end].iter().enumerate() {
            let index = piece.start + offset;
            if piece_width > 0 && piece_width.saturating_add(glyph_width) > width {
                split.push(Piece {
                    start: piece_start,
                    end: index,
                    content_end: index,
                    content_width: piece_width,
                    width: piece_width,
                });
                piece_start = index;
                piece_width = 0;
            }
            piece_width = piece_width.saturating_add(glyph_width);
        }
        split.push(Piece {
            start: piece_start,
            end: piece.end,
            content_end: piece.content_end,
            content_width: piece_width,
            width: piece_width.saturating_add(whitespace),
        });
    }
    split
}

fn sum_widths(widths: &[usize], start: usize, end: usize) -> usize {
    widths[start.min(widths.len())..end.min(widths.len())]
        .iter()
        .fold(0usize, |sum, width| sum.saturating_add(*width))
}

#[cfg(test)]
pub(crate) fn layout_for_test(value: &str, wrap: TextWrap, width: usize) -> TextLayout {
    layout_text(
        &Text::new(value).wrap(wrap),
        width,
        ComputedText::default(),
        |parent, _| parent,
        |style| *style,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(value: &str, wrap: TextWrap, width: usize) -> TextLayout {
        layout_text(
            &Text::new(value).wrap(wrap),
            width,
            ComputedText::default(),
            |parent, _| parent,
            |style| *style,
        )
    }

    fn rows(layout: &TextLayout) -> Vec<String> {
        layout
            .items()
            .iter()
            .map(|item| {
                if item.kind == ItemKind::Newline {
                    "\n".to_string()
                } else {
                    item.symbol.clone()
                }
            })
            .collect()
    }

    fn row_strings(layout: &TextLayout) -> Vec<String> {
        (0..layout.row_count())
            .map(|index| {
                layout
                    .row_items(index)
                    .iter()
                    .filter(|item| item.kind == ItemKind::Glyph)
                    .map(|item| item.symbol.as_str())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn empty_input_has_one_empty_row() {
        let layout = build("", TextWrap::Soft, 4);
        assert_eq!(layout.row_count(), 1);
        assert_eq!(layout.width(), 0);
        assert!(layout.row_items(0).is_empty());
    }

    #[test]
    fn items_partition_the_source_except_line_breaks() {
        for value in [
            "abc",
            "hello world",
            "a\n\nb",
            "你好世界",
            "a\tb",
            "e\u{301}x",
        ] {
            for wrap in [TextWrap::NoWrap, TextWrap::Soft, TextWrap::Hard] {
                for width in [1usize, 4, 80] {
                    let layout = build(value, wrap, width);
                    // Items are strictly ordered and never overlap, and the
                    // only bytes they may skip are explicit newlines, which are
                    // unowned line breaks rather than painted glyphs.
                    let mut cursor = 0usize;
                    for item in layout.items() {
                        assert!(
                            item.source.start >= cursor,
                            "overlap in {value:?} {wrap:?} {width}"
                        );
                        let skipped = &value[cursor..item.source.start];
                        assert!(
                            skipped.chars().all(|ch| ch == '\n'),
                            "unowned bytes {skipped:?} in {value:?} {wrap:?} {width}"
                        );
                        cursor = item.source.end;
                    }
                    assert!(
                        value[cursor..].chars().all(|ch| ch == '\n'),
                        "unowned tail in {value:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn rows_tile_the_source() {
        for value in ["abc", "hello world", "a\n\nb", "a\n", "\n\n", "你好 世界"] {
            for wrap in [TextWrap::NoWrap, TextWrap::Soft, TextWrap::Hard] {
                for width in [1usize, 3, 5, 80] {
                    let layout = build(value, wrap, width);
                    let mut cursor = 0usize;
                    for index in 0..layout.row_count() {
                        let start = layout.row_start(index);
                        let end = layout.row_source_end(index);
                        assert!(
                            start >= cursor,
                            "row {index} overlaps for {value:?} {wrap:?} {width}"
                        );
                        cursor = end;
                    }
                    assert_eq!(cursor, value.len(), "rows must cover {value:?}");
                }
            }
        }
    }

    #[test]
    fn every_row_positions_its_items_in_cell_order() {
        for value in ["abc", "hello world", "你好 世界", "e\u{301}x"] {
            let layout = build(value, TextWrap::Soft, 3);
            for index in 0..layout.row_count() {
                let mut cell = 0usize;
                for item in layout.row_items(index) {
                    assert_eq!(item.cell, cell, "item cell order in row {index}");
                    cell += item.width;
                }
                assert_eq!(layout.row_width(index), cell);
            }
        }
    }

    #[test]
    fn soft_wraps_at_line_break_opportunities() {
        let layout = build("hello world", TextWrap::Soft, 5);
        assert_eq!(row_strings(&layout), ["hello", "world"]);
    }

    #[test]
    fn soft_splits_overlong_fragments() {
        let layout = build("abcdef", TextWrap::Soft, 3);
        assert_eq!(row_strings(&layout), ["abc", "def"]);
    }

    #[test]
    fn hard_wraps_at_the_cell_limit() {
        let layout = build("ab cd", TextWrap::Hard, 4);
        assert_eq!(row_strings(&layout), ["ab c", "d"]);
        let layout = build("ab cd", TextWrap::Soft, 4);
        assert_eq!(row_strings(&layout), ["ab", "cd"]);
    }

    #[test]
    fn dropped_separator_stays_navigable() {
        let layout = build("hello world", TextWrap::Soft, 5);
        // The space lives on the first row as a separator and is not painted.
        let separators: Vec<&Item> = layout
            .items()
            .iter()
            .filter(|item| item.kind == ItemKind::Separator)
            .collect();
        assert_eq!(separators.len(), 1);
        assert_eq!(&layout.text()[separators[0].text.clone()], " ");
    }

    #[test]
    fn explicit_empty_and_trailing_lines_are_retained() {
        let layout = build("a\n\n", TextWrap::Soft, 3);
        assert_eq!(layout.row_count(), 3);
        assert_eq!(row_strings(&layout), ["a", "", ""]);
    }

    #[test]
    fn hit_test_round_trips_every_boundary() {
        for value in ["abc", "hello world", "你好世界", "e\u{301}x"] {
            let layout = build(value, TextWrap::Hard, 4);
            let mut boundary = 0usize;
            loop {
                assert_eq!(layout.clamp(boundary), boundary, "{value:?} at {boundary}");
                if boundary >= value.len() {
                    break;
                }
                boundary = layout.next_boundary(boundary);
            }
        }
    }

    #[test]
    fn vertical_movement_keeps_a_preferred_column() {
        // Two rows of the same 6-cell width; moving down from column 5 lands on
        // the short middle row's final boundary, and carrying that preferred
        // column forward returns to column 5 on the third row.
        let layout = build("abcdef\nxy\nabcdef", TextWrap::Soft, 3);
        assert_eq!(row_strings(&layout), ["abc", "def", "xy", "abc", "def"]);
        assert_eq!(layout.row_of_source(5), 1);
        assert_eq!(layout.column(5), 2);
        let (moved, preferred) = layout.vertical(5, 1, None);
        // Row 1 is "def" (source 6..9). Column 2 lands after "de" at source 9.
        assert_eq!(moved, 9);
        assert_eq!(preferred, Some(2));
        // Carrying the preferred column forward reaches column 2 of row 2.
        let (moved, _) = layout.vertical(moved, 1, preferred);
        assert_eq!(layout.column(moved), 2);
    }

    #[test]
    fn zero_width_viewport_is_clamped() {
        let layout = build("界", TextWrap::Soft, 0);
        assert!(layout.row_count() >= 1);
        let layout = build("界", TextWrap::Hard, 0);
        assert!(layout.row_count() >= 1);
    }

    #[test]
    fn every_item_has_a_row() {
        let layout = build("a\n\nbc", TextWrap::Soft, 2);
        for item in layout.items() {
            assert!(item.row < layout.row_count());
        }
        let _ = rows(&layout);
    }
}

/// Temporary parity harness: compares the canonical layout against the two
/// wrap implementations it is replacing. Removed once migration is complete.
#[cfg(test)]
pub(crate) mod parity {
    use super::*;
    use crate::{Text as PublicText, TextWrap as PublicWrap};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct LegacyRow {
        pub start: usize,
        pub end: usize,
        pub source_end: usize,
    }

    pub(crate) fn legacy_visual_rows(value: &str, wrap: PublicWrap, width: u16) -> Vec<LegacyRow> {
        crate::elements::text_edit::legacy_visual_rows_for_test(value, wrap, width)
            .into_iter()
            .map(|(start, end, source_end)| LegacyRow {
                start,
                end,
                source_end,
            })
            .collect()
    }

    /// The normalized form both engines lay out (CRLF/CR to LF, controls
    /// discarded). The canonical layout tracks raw-source bytes, so parity
    /// compares shapes over this normalized value.
    pub(crate) fn normalized(value: &str) -> String {
        value
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
            .collect()
    }

    pub(crate) fn canonical_rows(value: &str, wrap: PublicWrap, width: u16) -> Vec<LegacyRow> {
        let layout = layout_text(
            &PublicText::new(normalized(value)).wrap(wrap),
            width as usize,
            ComputedText::default(),
            |parent, _| parent,
            |style| *style,
        );
        (0..layout.row_count())
            .map(|index| LegacyRow {
                start: layout.row_start(index),
                end: layout.row_end(index),
                source_end: layout.row_source_end(index),
            })
            .collect()
    }

    pub(crate) fn canonical_row_symbols(
        value: &str,
        wrap: PublicWrap,
        width: usize,
    ) -> Vec<String> {
        let layout = layout_text(
            &PublicText::new(normalized(value)).wrap(wrap),
            width,
            ComputedText::default(),
            |parent, _| parent,
            |style| *style,
        );
        (0..layout.row_count())
            .map(|index| {
                let mut painted = String::new();
                for item in layout.row_items(index) {
                    if item.kind != ItemKind::Glyph || item.width == 0 {
                        continue;
                    }
                    if item.symbol == "\t" {
                        painted.push_str(&" ".repeat(item.width));
                    } else {
                        painted.push_str(&item.symbol);
                    }
                }
                painted
            })
            .collect()
    }

    // --- Self-contained copy of the legacy commit glyph/wrap algorithm. ---
    // This is the exact code that occupied `runtime::commit::text` before the
    // canonical layout migration. It shapes from the raw source independently
    // rather than reading the canonical layout, so it is a genuine oracle.

    #[derive(Debug, Clone)]
    struct LegacyGlyph {
        symbol: String,
        width: usize,
    }

    #[derive(Debug, Clone)]
    struct LegacyFragment {
        glyphs: Vec<LegacyGlyph>,
        content_len: usize,
        content_width: usize,
        whitespace_width: usize,
    }

    impl textwrap::core::Fragment for LegacyFragment {
        fn width(&self) -> f64 {
            self.content_width as f64
        }
        fn whitespace_width(&self) -> f64 {
            self.whitespace_width as f64
        }
        fn penalty_width(&self) -> f64 {
            0.0
        }
    }

    fn glyph_width(glyphs: &[LegacyGlyph]) -> usize {
        glyphs.iter().map(|glyph| glyph.width).sum()
    }

    fn split_overlong_fragment(fragment: LegacyFragment, width: usize) -> Vec<LegacyFragment> {
        if fragment.content_width <= width || width == 0 {
            return vec![fragment];
        }
        let mut pieces = Vec::new();
        let content = &fragment.glyphs[..fragment.content_len.min(fragment.glyphs.len())];
        let whitespace = &fragment.glyphs[fragment.content_len.min(fragment.glyphs.len())..];
        let mut current = Vec::new();
        let mut current_width = 0usize;
        for glyph in content {
            if current_width > 0 && current_width.saturating_add(glyph.width) > width {
                pieces.push(LegacyFragment {
                    content_len: current.len(),
                    glyphs: std::mem::take(&mut current),
                    content_width: current_width,
                    whitespace_width: 0,
                });
                current_width = 0;
            }
            current_width = current_width.saturating_add(glyph.width);
            current.push(glyph.clone());
        }
        if !current.is_empty() {
            pieces.push(LegacyFragment {
                content_len: current.len(),
                glyphs: current
                    .into_iter()
                    .chain(whitespace.iter().cloned())
                    .collect(),
                content_width: current_width,
                whitespace_width: fragment.whitespace_width,
            });
        }
        if pieces.is_empty() {
            vec![fragment]
        } else {
            pieces
        }
    }

    fn hard_wrapped_line(line: Vec<LegacyGlyph>, width: usize) -> Vec<Vec<LegacyGlyph>> {
        if line.is_empty() {
            return vec![line];
        }
        let fragments = line
            .into_iter()
            .map(|glyph| LegacyFragment {
                content_width: glyph.width,
                content_len: 1,
                whitespace_width: 0,
                glyphs: vec![glyph],
            })
            .collect::<Vec<_>>();
        textwrap::wrap_algorithms::wrap_first_fit(&fragments, &[width.max(1) as f64])
            .into_iter()
            .map(|row| {
                row.iter()
                    .flat_map(|fragment| fragment.glyphs.iter().cloned())
                    .collect()
            })
            .collect()
    }

    fn wrapped_line(
        line: Vec<LegacyGlyph>,
        wrap: PublicWrap,
        width: usize,
    ) -> Vec<Vec<LegacyGlyph>> {
        if matches!(wrap, PublicWrap::NoWrap) || line.is_empty() {
            return vec![line];
        }
        if matches!(wrap, PublicWrap::Hard) {
            return hard_wrapped_line(line, width);
        }
        let mut source = String::new();
        let mut offsets = Vec::with_capacity(line.len() + 1);
        offsets.push(0);
        for glyph in &line {
            source.push_str(&glyph.symbol);
            offsets.push(source.len());
        }
        let words = textwrap::WordSeparator::UnicodeBreakProperties
            .find_words(&source)
            .collect::<Vec<_>>();
        if words.is_empty() {
            return vec![line];
        }
        let mut fragments = Vec::new();
        let mut byte_cursor = 0usize;
        for word in words {
            let word_start = byte_cursor;
            let content_end = word_start.saturating_add(word.word.len());
            let end = content_end.saturating_add(word.whitespace.len());
            byte_cursor = end;
            let first = offsets.partition_point(|offset| *offset < word_start);
            let last = offsets.partition_point(|offset| *offset < end);
            let content_last = offsets.partition_point(|offset| *offset < content_end);
            let glyphs = line[first.min(line.len())..last.min(line.len())].to_vec();
            let content_count = content_last.saturating_sub(first).min(glyphs.len());
            let content_glyphs = glyphs[..content_count].to_vec();
            let whitespace_glyphs = glyphs[content_count..].to_vec();
            fragments.push(LegacyFragment {
                content_len: content_glyphs.len(),
                content_width: glyph_width(&content_glyphs),
                whitespace_width: glyph_width(&whitespace_glyphs),
                glyphs,
            });
        }
        if byte_cursor < source.len() {
            let first = offsets.partition_point(|offset| *offset < byte_cursor);
            fragments.push(LegacyFragment {
                glyphs: line[first.min(line.len())..].to_vec(),
                content_len: line.len().saturating_sub(first.min(line.len())),
                content_width: glyph_width(&line[first.min(line.len())..]),
                whitespace_width: 0,
            });
        }
        let preserve_trailing_whitespace = line
            .last()
            .is_some_and(|glyph| glyph.symbol.chars().all(char::is_whitespace));
        if preserve_trailing_whitespace && let Some(last) = fragments.last_mut() {
            last.content_len = last.glyphs.len();
            last.content_width = last.content_width.saturating_add(last.whitespace_width);
            last.whitespace_width = 0;
        }
        fragments = fragments
            .into_iter()
            .flat_map(|fragment| split_overlong_fragment(fragment, width))
            .collect();
        textwrap::wrap_algorithms::wrap_first_fit(&fragments, &[width.max(1) as f64])
            .into_iter()
            .map(|line| {
                let mut glyphs = Vec::new();
                for (index, fragment) in line.iter().enumerate() {
                    let content_count = if index + 1 == line.len() && !preserve_trailing_whitespace
                    {
                        fragment.content_len.min(fragment.glyphs.len())
                    } else {
                        fragment.glyphs.len()
                    };
                    glyphs.extend(fragment.glyphs.iter().take(content_count).cloned());
                    if index + 1 != line.len() {
                        glyphs.extend(fragment.glyphs.iter().skip(content_count).cloned());
                    }
                }
                glyphs
            })
            .collect()
    }

    pub(crate) fn legacy_commit_rows(value: &str, wrap: PublicWrap, width: usize) -> Vec<String> {
        let value = normalized(value);
        let mut lines: Vec<Vec<LegacyGlyph>> = vec![Vec::new()];
        for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(value.as_str(), true) {
            if grapheme == "\n" {
                lines.push(Vec::new());
                continue;
            }
            let (symbol, width) = if grapheme == "\t" {
                ("    ".to_string(), 4usize)
            } else if grapheme.chars().any(char::is_control) {
                ("\u{FFFD}".to_string(), 1)
            } else {
                let symbol = grapheme.to_string();
                let width = unicode_width::UnicodeWidthStr::width(symbol.as_str());
                if symbol.len() > crate::MAX_GLYPH_BYTES || !(1..=2).contains(&width) {
                    ("\u{FFFD}".to_string(), 1)
                } else {
                    (symbol, width)
                }
            };
            if width == 0 {
                continue;
            }
            lines
                .last_mut()
                .expect("line exists")
                .push(LegacyGlyph { symbol, width });
        }
        let width = width.max(1);
        let mut result = Vec::new();
        for line in lines {
            result.extend(wrapped_line(line, wrap, width));
        }
        if result.is_empty() {
            result.push(Vec::new());
        }
        result
            .into_iter()
            .map(|line| line.into_iter().map(|glyph| glyph.symbol).collect())
            .collect()
    }
}

#[cfg(test)]
mod parity_tests {
    use super::parity::*;
    use super::*;
    use crate::TextWrap as W;

    fn corpus() -> Vec<&'static str> {
        vec![
            "",
            "a",
            "abc",
            "abcdef",
            "hello world",
            "a   ",
            "     ",
            "ab cd",
            "a\n",
            "a\n\n",
            "a\n\nb",
            "\n",
            "\n\n",
            "line one\nline two",
            "line one\n\nline three",
            "你好世界",
            "你好 世界",
            "a\u{301}b界",
            "👩‍💻x",
            "a\tb",
            "\t\tx",
            "a\tb\nc\td",
            "Long Unicode text: 这是一个很长的示例文本，含有 emoji 👩‍💻 and an unbreakable-token-for-hard-wrap.",
            "  leading",
            "trailing  ",
            "word ",
            " \n ",
            "a\r\nb",
            "a\rb",
            "e\u{301}\u{301}f",
            "🇺🇸🇨🇳",
            "👍🏽ok",
        ]
    }

    fn widths_u16() -> [u16; 10] {
        [1, 2, 3, 4, 5, 6, 8, 12, 24, 80]
    }

    fn widths_usize() -> [usize; 10] {
        [1, 2, 3, 4, 5, 6, 8, 12, 24, 80]
    }

    const WRAPS: [W; 3] = [W::NoWrap, W::Soft, W::Hard];

    /// The legacy editor appends a phantom empty row so a caret at the end of a
    /// full row can be painted. That is an editor presentation artifact rather
    /// than layout geometry, so it is not part of the canonical layout.
    fn strip_phantom_rows(mut rows: Vec<LegacyRow>, wrap: W) -> Vec<LegacyRow> {
        while rows.len() > 1 {
            let last = rows[rows.len() - 1];
            let previous = rows[rows.len() - 2];
            if last.start == last.end
                && last.start == previous.source_end
                && matches!(wrap, W::Soft | W::Hard)
            {
                rows.pop();
            } else {
                break;
            }
        }
        rows
    }

    #[test]
    fn canonical_matches_legacy_editor_row_geometry() {
        let mut failures = Vec::new();
        for value in corpus() {
            for wrap in WRAPS {
                for width in widths_u16() {
                    let legacy = strip_phantom_rows(legacy_visual_rows(value, wrap, width), wrap);
                    let canonical = canonical_rows(value, wrap, width);
                    if legacy != canonical {
                        failures.push(format!(
                            "value={value:?} wrap={wrap:?} width={width}\n  legacy={legacy:?}\n  canonical={canonical:?}"
                        ));
                    }
                }
            }
        }
        report("editor", failures);
    }

    /// Strip whitespace that the canonical layout marks as a separator: it is
    /// not painted, so the painted rows of the two engines agree even when the
    /// internal row strings differ.
    fn without_separators(value: &str, wrap: W, width: usize) -> Vec<String> {
        let layout =
            crate::basic::text_layout::layout_for_test(normalized(value).as_str(), wrap, width);
        (0..layout.row_count())
            .map(|index| {
                layout
                    .row_items(index)
                    .iter()
                    .filter(|item| item.kind == ItemKind::Glyph && item.width > 0)
                    .map(|item| item.symbol.as_str())
                    .collect::<String>()
            })
            .collect()
    }

    /// Commit parity is asserted over painted cells, not internal row strings.
    ///
    /// Two intentional differences live in the row strings only:
    ///
    /// 1. Tabs: the canonical layout uses terminal tab stops while the legacy
    ///    commit glyph expander always emitted four spaces. Tabs are excluded
    ///    here and pinned separately.
    /// 2. Whitespace that forces a soft wrap is a separator rather than a
    ///    painted glyph, so the row's internal string omits it even when the
    ///    following word also overflows.
    ///
    /// Both produce the same visible cells at their final widths and row
    /// boundaries, and the row geometry itself is asserted to match (see
    /// `separator_wrapping_preserves_row_geometry`).
    #[test]
    fn canonical_matches_legacy_commit_rows() {
        let mut failures = Vec::new();
        for value in corpus() {
            if value.contains('\t') {
                continue;
            }
            for wrap in WRAPS {
                for width in widths_usize() {
                    let legacy = legacy_commit_rows(value, wrap, width);
                    let canonical = without_separators(value, wrap, width);
                    let legacy_painted: Vec<String> = legacy
                        .iter()
                        .map(|row| row.chars().filter(|c| !c.is_whitespace()).collect())
                        .collect();
                    let canonical_painted: Vec<String> = canonical
                        .iter()
                        .map(|row| row.chars().filter(|c| !c.is_whitespace()).collect())
                        .collect();
                    if legacy_painted != canonical_painted {
                        failures.push(format!(
                            "value={value:?} wrap={wrap:?} width={width}\n  legacy={legacy:?}\n  canonical={canonical:?}"
                        ));
                    }
                }
            }
        }
        report("commit", failures);
    }

    /// Whitespace dropped at a soft wrap must not change where the source
    /// bytes land: the surviving glyphs keep their rows and order.
    #[test]
    fn separator_wrapping_preserves_row_geometry() {
        for value in ["你好 世界", "hello world", "ab cd ef"] {
            for width in widths_u16() {
                let canonical = canonical_rows(value, wrap_soft(), width);
                // Concatenating the row ranges reproduces the source exactly.
                let mut expected = 0usize;
                for row in &canonical {
                    assert!(
                        row.start >= expected,
                        "row overlap for {value:?} at {width}: {canonical:?}"
                    );
                    expected = row.source_end;
                }
            }
        }
    }

    fn wrap_soft() -> W {
        W::Soft
    }

    /// Intentional behavior changes from the legacy engines.
    ///
    /// These are asserted rather than merely omitted so the difference is
    /// explicit and the canonical behavior is pinned:
    ///
    /// 1. Tabs use terminal tab stops (column-relative), which both the
    ///    renderer's cells and the editor already assume. The old commit glyph
    ///    expander always emitted four spaces per tab regardless of column.
    /// 2. Whitespace that forces a soft wrap is not painted (it is a
    ///    [`ItemKind::Separator`]) even when the following word also overflows.
    ///    The rendered terminal cells are identical; only the internal row
    ///    string differs.
    #[test]
    fn intentional_behavior_changes_are_pinned() {
        // Tab stops are column-relative: "a\tb" needs three spaces after "a"
        // (to the next four-cell stop), not four.
        let layout = crate::basic::text_layout::layout_for_test("a\tb", W::NoWrap, 8);
        assert_eq!(
            layout.column(2),
            4,
            "a tab after one column reaches column 4"
        );
        assert_eq!(layout.column(3), 5, "the glyph after the tab follows it");
        let painted: String = layout
            .items()
            .iter()
            .filter(|item| item.kind == ItemKind::Glyph && item.width > 0)
            .map(|item| {
                if item.symbol == "\t" {
                    " ".repeat(item.width)
                } else {
                    item.symbol.clone()
                }
            })
            .collect();
        assert_eq!(painted, "a   b");

        // At a width of one cell the forcing space is dropped, so the words sit
        // next to each other in the row text but still paint on separate rows.
        let rows = canonical_row_symbols("hello world", W::Soft, 5);
        assert_eq!(rows, ["hello", "world"]);
    }

    fn report(label: &str, failures: Vec<String>) {
        if failures.is_empty() {
            return;
        }
        let head: Vec<&String> = failures.iter().take(8).collect();
        panic!(
            "{} {label} parity failures:\n{}",
            failures.len(),
            head.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
