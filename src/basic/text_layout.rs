use std::ops::Range;

use textwrap::core::Fragment;
use textwrap::wrap_algorithms::wrap_first_fit;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{EmojiMerging, MAX_GLYPH_BYTES};

use super::props::TextStyle;
use super::text::{Text, TextWrap};

pub(crate) const TAB_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HitBias {
    #[default]
    Leading,
    Trailing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ItemKind {
    Glyph,
    Separator,
    Newline,
}

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

#[derive(Debug, Clone)]
#[allow(dead_code)] // `text`/`cell`/`row` are the layout's public geometry surface.
pub(crate) struct Item {
    pub(crate) source: Range<usize>,
    pub(crate) text: Range<usize>,
    pub(crate) cell: usize,
    pub(crate) width: usize,
    pub(crate) kind: ItemKind,
    pub(crate) row: usize,
    pub(crate) symbol: String,
    pub(crate) style: ComputedText,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) first_item: usize,
    pub(crate) len: usize,
    pub(crate) source_start: usize,
    pub(crate) source_end: usize,
}

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

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // Operations are the API; storage stays private.
pub(crate) struct TextLayout {
    text: String,
    items: Vec<Item>,
    rows: Vec<Row>,
    boundaries: Vec<usize>,
    // Row source ranges are non-decreasing, so a row lookup is a binary search
    // rather than a scan. Cached with each row's painted and maximum width.
    row_source_ends: Vec<usize>,
    row_widths: Vec<usize>,
    max_row_width: usize,
    width: usize,
    source_len: usize,
}

pub(crate) fn layout_text<S>(
    text: &Text,
    width: usize,
    inherited: ComputedText,
    merging: EmojiMerging,
    merge: impl Fn(ComputedText, &TextStyle) -> S,
    into_computed: impl Fn(&S) -> ComputedText,
) -> TextLayout
where
    S: Clone,
{
    let shaped = shape_text(text, inherited, merging, merge);
    TextLayout::layout(shaped, text.wrap, width, into_computed)
}

fn shape_text<S>(
    text: &Text,
    inherited: ComputedText,
    merging: EmojiMerging,
    merge: impl Fn(ComputedText, &TextStyle) -> S,
) -> Vec<ShapedGlyph<S>>
where
    S: Clone,
{
    let mut shaped = Vec::new();
    let mut text_offset = 0usize;
    // Source offsets are global to the whole `Text`, so a multi-span value has
    // non-overlapping ranges just like a single-span one. Each span contributes
    // its normalized length to the running origin.
    let mut source_origin = 0usize;
    for span in &text.spans {
        let style = merge(inherited, &span.style);
        let content = span.content.replace("\r\n", "\n").replace('\r', "\n");
        for (source_index, grapheme) in content.grapheme_indices(true) {
            if !merging.merges()
                && let Some(parts) = crate::data::separate_units(grapheme)
            {
                // Each unit owns its own codepoints' source range, so a caret
                // boundary exists between the parts - exactly the cells a
                // non-merging terminal paints - and pointer hits are monotonic
                // across them.
                for (range, width) in parts {
                    let symbol = grapheme[range.clone()].to_string();
                    let text_end = text_offset + symbol.len();
                    shaped.push(ShapedGlyph {
                        source: source_origin + source_index + range.start
                            ..source_origin + source_index + range.end,
                        text_start: text_offset,
                        text_end,
                        width,
                        symbol,
                        kind: ItemKind::Glyph,
                        style: style.clone(),
                    });
                    text_offset = text_end;
                }
                continue;
            }
            let (symbol, width, kind) = display_glyph(grapheme);
            let text_end = text_offset + symbol.len();
            shaped.push(ShapedGlyph {
                source: source_origin + source_index..source_origin + source_index + grapheme.len(),
                text_start: text_offset,
                text_end,
                width,
                symbol,
                kind,
                style: style.clone(),
            });
            text_offset = text_end;
        }
        source_origin += content.len();
    }
    shaped
}

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
            source_start: usize,
            pieces: Vec<Piece>,
            is_last_row: bool,
        }
        // The source start of a shaped index; the end of the value past the
        // last shaped glyph.
        let source_at = |index: usize| {
            shaped
                .get(index)
                .map_or(source_len, |glyph| glyph.source.start)
        };
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
                    source_start: source_at(logical.start),
                    pieces: Vec::new(),
                    is_last_row: true,
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
                    source_start: source_at(start),
                    pieces: line.to_vec(),
                    is_last_row: line_index == last_index,
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
            let source_end = placed
                .get(row_index + 1)
                .map_or(source_len, |next| next.source_start);
            rows.push(Row {
                first_item,
                len: items.len() - first_item,
                source_start: entry.source_start,
                // The final row owns everything through the end of the value,
                // including a trailing newline the value ends with.
                source_end: source_end.max(entry.source_start),
            });
        }
        if let Some(last) = rows.last_mut() {
            last.source_end = source_len.max(last.source_start);
        }

        let layer_width = rows
            .iter()
            .map(|row| row_cells(&items, row))
            .max()
            .unwrap_or(0);

        // Every grapheme edge, every explicit-newline edge, and the value ends
        // are valid boundaries. Newlines own no items, so they are collected
        // separately from the shaped table.
        let mut boundaries = Vec::with_capacity(items.len() * 2 + 4);
        boundaries.push(0usize);
        for glyph in &shaped {
            boundaries.push(glyph.source.start);
            boundaries.push(glyph.source.end);
        }
        boundaries.push(source_len);
        boundaries.sort_unstable();
        boundaries.dedup();

        // One traversal builds both row indexes, and the row widths are
        // computed once here instead of being rescanned per query.
        let mut row_source_ends = Vec::with_capacity(rows.len());
        let mut row_widths = Vec::with_capacity(rows.len());
        let mut max_row_width = 0usize;
        for row in &rows {
            row_source_ends.push(row.source_end);
            let painted = items[row.first_item..row.first_item + row.len]
                .iter()
                .filter(|item| item.kind != ItemKind::Separator)
                .fold(0usize, |sum, item| sum.saturating_add(item.width));
            max_row_width = max_row_width.max(painted);
            row_widths.push(painted);
        }

        Self {
            text,
            items,
            rows,
            boundaries,
            row_source_ends,
            row_widths,
            max_row_width,
            width: layer_width,
            source_len,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn width(&self) -> usize {
        self.width
    }

    #[allow(dead_code)]
    pub(crate) fn source_len(&self) -> usize {
        self.source_len
    }

    #[allow(dead_code)]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    #[allow(dead_code)]
    pub(crate) fn items(&self) -> &[Item] {
        &self.items
    }

    pub(crate) fn row_items(&self, index: usize) -> &[Item] {
        self.rows.get(index).map_or(&[], |row| {
            &self.items[row.first_item..row.first_item + row.len]
        })
    }

    pub(crate) fn row_start(&self, index: usize) -> usize {
        self.rows
            .get(index)
            .map_or(self.source_len, |row| row.source_start)
    }

    #[allow(dead_code)]
    pub(crate) fn row_end(&self, index: usize) -> usize {
        self.row_items(index)
            .iter()
            .rfind(|item| item.kind == ItemKind::Glyph && !item.symbol.is_empty())
            .map_or_else(|| self.row_start(index), |item| item.source.end)
    }

    #[allow(dead_code)] // Part of the layout's operation surface; exercised by tests.
    pub(crate) fn row_source_end(&self, index: usize) -> usize {
        self.rows
            .get(index)
            .map_or(self.source_len, |row| row.source_end)
    }

    // A separator is deliberately not painted, so it contributes no cells even
    // though it owns source bytes. The width is cached at construction.
    pub(crate) fn row_width(&self, index: usize) -> usize {
        self.row_widths.get(index).copied().unwrap_or(0)
    }

    pub(crate) fn max_row_width(&self) -> usize {
        self.max_row_width
    }

    pub(crate) fn row_of_source(&self, source: usize) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        let source = source.min(self.source_len);
        // `partition_point` finds the first row whose source range ends after
        // the offset: O(log rows) instead of a linear scan per query.
        match self.row_source_ends.partition_point(|end| *end <= source) {
            index if index < self.rows.len() => index,
            _ => self.rows.len() - 1,
        }
    }

    pub(crate) fn clamp(&self, source: usize) -> usize {
        if self.boundaries.is_empty() {
            return 0;
        }
        let source = source.min(self.source_len);
        match self.boundaries.binary_search(&source) {
            Ok(_) => source,
            Err(at) => {
                if at == 0 {
                    return self.boundaries[0];
                }
                if at >= self.boundaries.len() {
                    return *self.boundaries.last().expect("non-empty boundaries");
                }
                let before = self.boundaries[at - 1];
                let after = self.boundaries[at];
                // Prefer the nearer boundary; ties resolve forward so repeated
                // stepping always advances.
                if source - before < after - source {
                    before
                } else {
                    after
                }
            }
        }
    }

    #[allow(dead_code)] // Part of the layout's operation surface; exercised by tests.
    pub(crate) fn previous_boundary(&self, source: usize) -> usize {
        let source = self.clamp(source);
        match self.boundaries.binary_search(&source) {
            Ok(0) => 0,
            Ok(at) => self.boundaries[at - 1],
            Err(at) if at > 0 => self.boundaries[at - 1],
            Err(_) => 0,
        }
    }

    #[allow(dead_code)] // Part of the layout's operation surface; exercised by tests.
    pub(crate) fn next_boundary(&self, source: usize) -> usize {
        let clamped = self.clamp(source);
        // A position that clamped forward is already the next boundary.
        if clamped > source {
            return clamped;
        }
        match self.boundaries.binary_search(&clamped) {
            Ok(at) => self.boundaries.get(at + 1).copied().unwrap_or(clamped),
            Err(at) => self.boundaries.get(at).copied().unwrap_or(clamped),
        }
    }

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

    pub(crate) fn hit(&self, row: usize, cell: usize, bias: HitBias) -> usize {
        let row = row.min(self.rows.len().saturating_sub(1));
        let mut offset = 0usize;
        for item in self.row_items(row) {
            // Never resolve outside the row that was hit: a boundary shared with
            // the previous row (the leading edge of this row's first grapheme)
            // would otherwise place a caret on the wrong visual line.
            let clamp_to_row =
                |offset: usize| offset.clamp(self.row_start(row), self.row_last_boundary(row));
            if item.kind == ItemKind::Newline {
                return item.source.start;
            }
            let end = offset.saturating_add(item.width);
            if item.width > 0 && cell < end {
                let right_half = (cell - offset) * 2 >= item.width;
                return clamp_to_row(match bias {
                    HitBias::Leading => {
                        if item.width > 1 && right_half {
                            item.source.end
                        } else {
                            item.source.start
                        }
                    }
                    HitBias::Trailing => {
                        if item.width > 1 && !right_half {
                            item.source.start
                        } else {
                            item.source.end
                        }
                    }
                });
            }
            offset = end;
        }
        // Past the painted content: the caret belongs after this row's last
        // glyph, not at the next row's first boundary.
        self.row_last_boundary(row)
    }

    fn row_last_boundary(&self, row: usize) -> usize {
        self.row_items(row)
            .last()
            .map_or_else(|| self.row_start(row), |item| item.source.end)
    }

    #[allow(dead_code)]
    pub(crate) fn column(&self, source: usize) -> usize {
        let (_, cell, _) = self.caret(source);
        cell
    }

    #[allow(dead_code)]
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
}

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

fn item_is_dropped_separator(pieces: &[Piece], index: usize, is_last_row: bool) -> bool {
    if is_last_row {
        // The last row of a logical line paints its trailing whitespace.
        return false;
    }
    let position = pieces.partition_point(|piece| piece.start <= index);
    let Some(piece) = position.checked_sub(1).and_then(|at| pieces.get(at)) else {
        return false;
    };
    // `pieces` holds only the pieces packed onto THIS row. Whitespace is
    // dropped only when its piece is the row's LAST piece: that whitespace is
    // what pushed the following piece onto a later row. Whitespace inside the
    // row - a piece that is followed by another piece on the same row - is
    // ordinary painted content, and dropping it would shift every later
    // grapheme out of agreement with the caret and hit-testing tables.
    let is_last_piece = position >= pieces.len();
    is_last_piece && index >= piece.content_end
}

fn row_cells(items: &[Item], row: &Row) -> usize {
    items[row.first_item..row.first_item + row.len]
        .iter()
        .filter(|item| item.kind == ItemKind::Glyph)
        .fold(0usize, |sum, item| sum.saturating_add(item.width))
}

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
        EmojiMerging::Merge,
        |parent, _| parent,
        |style| *style,
    )
}

#[cfg(test)]
pub(crate) fn layout_for_test_with(
    value: &str,
    wrap: TextWrap,
    width: usize,
    merging: EmojiMerging,
) -> TextLayout {
    layout_text(
        &Text::new(value).wrap(wrap),
        width,
        ComputedText::default(),
        merging,
        |parent, _| parent,
        |style| *style,
    )
}

// Test-only surface for the layout's index queries. The layout itself is
// crate-private, so integration tests need a narrow, documented view.
#[doc(hidden)]
pub struct TextLayoutForTest(pub(crate) TextLayout);

impl TextLayoutForTest {
    pub fn row_count(&self) -> usize {
        self.0.rows.len()
    }

    pub fn row_of_source(&self, source: usize) -> usize {
        self.0.row_of_source(source)
    }

    pub fn row_source_end(&self, index: usize) -> usize {
        self.0.row_source_end(index)
    }

    pub fn row_width(&self, index: usize) -> usize {
        self.0.row_width(index)
    }

    pub fn max_row_width(&self) -> usize {
        self.0.max_row_width()
    }
}

#[doc(hidden)]
pub fn indexed_layout_for_test(text: &str, width: usize) -> TextLayoutForTest {
    let node = crate::Text::new(text);
    TextLayoutForTest(layout_text(
        &node,
        width,
        ComputedText::default(),
        crate::EmojiMerging::Merge,
        |parent, _| parent,
        |style| *style,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(value: &str, wrap: TextWrap, width: usize) -> TextLayout {
        layout_text(
            &Text::new(value).wrap(wrap),
            width,
            ComputedText::default(),
            EmojiMerging::Merge,
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
                let mut painted = 0usize;
                for item in layout.row_items(index) {
                    assert_eq!(item.cell, cell, "item cell order in row {index}");
                    cell += item.width;
                    if item.kind != ItemKind::Separator {
                        painted += item.width;
                    }
                }
                assert_eq!(layout.row_width(index), painted);
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
    fn separators_inside_a_row_are_painted() {
        let layout = build("ab cd efgh", TextWrap::Soft, 6);
        let painted: Vec<String> = (0..layout.row_count())
            .map(|index| {
                layout
                    .row_items(index)
                    .iter()
                    .filter(|item| item.kind != ItemKind::Separator)
                    .map(|item| item.symbol.clone())
                    .collect()
            })
            .collect();
        assert_eq!(
            painted,
            ["ab cd", "efgh"],
            "the space inside the first row is painted; only the trailing one \
             that pushed 'efgh' onto the next row is dropped"
        );
        // The caret table and the painted cells agree: 'c' is at cell 3 because
        // the space before it occupies cell 2.
        assert_eq!(layout.caret(3).1, 3, "'c' follows the painted space");
        assert_eq!(layout.hit(0, 2, HitBias::Leading), 2);
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

#[cfg(test)]
mod parity {
    pub(crate) fn normalized(value: &str) -> String {
        value
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
            .collect()
    }
}

#[cfg(test)]
mod property_tests {
    use super::parity::normalized;
    use super::*;
    use crate::Span;
    #[allow(unused_imports)]
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
            // Emoji sequences whose clusters merge at runtime.
            "👩\u{200D}💻",
            "👨\u{200D}👩\u{200D}👧\u{200D}👦",
            "👩\u{200D}",
            "\u{200D}💻",
            "a\u{200D}b",
            "\u{200D}",
            "\u{FE0F}",
            "\u{301}",
            "1\u{FE0F}\u{20E3}",
            "❤\u{FE0F}",
            "❤",
            "👍👍",
            "🇺🇸🇨🇳🇯🇵",
            "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
        ]
    }

    const WRAPS: [W; 3] = [W::NoWrap, W::Soft, W::Hard];
    const WIDTHS: [usize; 10] = [1, 2, 3, 4, 5, 6, 8, 12, 24, 80];

    #[test]
    fn every_returned_position_is_a_grapheme_boundary() {
        use unicode_segmentation::UnicodeSegmentation;
        for value in corpus() {
            let value = normalized(value);
            let boundaries: std::collections::HashSet<usize> = std::iter::once(0)
                .chain(
                    value
                        .grapheme_indices(true)
                        .map(|(index, grapheme)| index + grapheme.len()),
                )
                .collect();
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test(&value, wrap, width);
                    let mut position = 0usize;
                    let mut guard = 0usize;
                    loop {
                        assert!(
                            boundaries.contains(&position),
                            "{position} is not a boundary of {value:?}"
                        );
                        if position >= value.len() {
                            break;
                        }
                        let next = layout.next_boundary(position);
                        assert!(
                            next > position,
                            "next_boundary({position}) must advance for {value:?} \
                             {wrap:?} {width}, returned {next}"
                        );
                        position = next;
                        guard += 1;
                        assert!(guard <= value.len() + 2, "walk must terminate");
                    }
                    for index in 0..layout.row_count() {
                        assert!(boundaries.contains(&layout.row_start(index)));
                        assert!(boundaries.contains(&layout.row_end(index)));
                        assert!(boundaries.contains(&layout.row_source_end(index)));
                    }
                }
            }
        }
    }

    #[test]
    fn hit_test_round_trips_caret_positions() {
        for value in corpus() {
            let value = normalized(value);
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test(&value, wrap, width);
                    let mut position = 0usize;
                    loop {
                        let (row, cell, caret_width) = layout.caret(position);
                        let hit = layout.hit(row, cell, HitBias::Leading);
                        // A caret past the last painted cell of its row shares
                        // that cell with the following row's first boundary, and
                        // a wide grapheme's trailing edge shares its second cell.
                        // Those positions are visually ambiguous by definition;
                        // the plan only requires the unambiguous ones to round
                        // trip.
                        let past_last_cell = cell >= layout.row_width(row);
                        let ambiguous = past_last_cell || (caret_width > 1 && cell > 0);
                        assert!(
                            hit == position || ambiguous,
                            "round trip failed for {value:?} {wrap:?} {width}: \
                             position={position} row={row} cell={cell} hit={hit}"
                        );
                        if position >= value.len() {
                            break;
                        }
                        position = layout.next_boundary(position);
                    }
                }
            }
        }
    }

    #[test]
    fn every_displayed_cell_is_reachable() {
        for value in corpus() {
            let value = normalized(value);
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test(&value, wrap, width);
                    for index in 0..layout.row_count() {
                        let cells = layout.row_width(index);
                        for cell in 0..cells {
                            let hit = layout.hit(index, cell, HitBias::Trailing);
                            // A hit resolves to a boundary; that boundary's
                            // caret belongs to this row, except at the very end
                            // of a row, where the caret continues on the next
                            // one. Both are correct: what must never happen is a
                            // hit landing on an unrelated earlier row.
                            let (row, _, _) = layout.caret(hit);
                            assert!(
                                row == index || row == index + 1,
                                "cell {cell} of row {index} resolved to row {row} \
                                 for {value:?} {wrap:?} {width}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn measure_and_paint_agree_on_extents() {
        for value in corpus() {
            let value = normalized(value);
            for wrap in WRAPS {
                for width in [0usize, 1, 2, 3, 4, 5, 8, 24] {
                    let layout = layout_for_test(&value, wrap, width);
                    assert!(layout.row_count() >= 1, "a layout always has one row");
                    let widest = (0..layout.row_count())
                        .map(|index| layout.row_width(index))
                        .max()
                        .unwrap_or(0);
                    assert_eq!(layout.max_row_width(), widest);
                    assert_eq!(layout.width(), widest);
                }
            }
        }
    }

    #[test]
    fn wrap_modes_preserve_source_and_respect_the_viewport() {
        for value in corpus() {
            let value = normalized(value);
            for wrap in WRAPS {
                for width in [1usize, 3, 5, 12, 80] {
                    let layout = layout_for_test(&value, wrap, width);
                    let mut cursor = 0usize;
                    for index in 0..layout.row_count() {
                        let start = layout.row_start(index);
                        let end = layout.row_source_end(index);
                        assert!(start >= cursor, "rows must not overlap");
                        cursor = end;
                        // A wrapped row fits its viewport. Two documented
                        // exceptions: a grapheme wider than the viewport cannot
                        // be split, and `NoWrap` deliberately overflows so a
                        // scroll host can pan across the logical line.
                        if wrap != W::NoWrap {
                            let unbreakable = layout
                                .row_items(index)
                                .iter()
                                .any(|item| item.kind == ItemKind::Glyph && item.width > width);
                            assert!(
                                unbreakable || layout.row_width(index) <= width,
                                "row {index} is {} cells wide in a {width}-cell box \
                                 for {value:?} {wrap:?}",
                                layout.row_width(index)
                            );
                        }
                    }
                    assert_eq!(cursor, value.len(), "rows must cover {value:?}");
                }
            }
        }
    }

    #[test]
    fn wide_graphemes_in_a_one_cell_viewport_are_safe() {
        // The wide grapheme overflows its one-cell viewport, but it still owns
        // exactly one row and never produces a half continuation cell.
        let layout = layout_for_test("界a", W::Hard, 1);
        let row = layout.row_of_source(0);
        assert_eq!(
            layout.row_items(row)[0].source,
            0..3,
            "the wide grapheme starts its row"
        );
        assert_eq!(layout.row_items(row)[0].width, 2);
        assert!(
            layout.row_count() >= 2,
            "the following glyph needs its own row"
        );
        // The leading boundary sits at the start of the wide grapheme's row;
        // its trailing boundary begins the following row, which is what lets a
        // caret step past a glyph the viewport cannot show in full.
        assert_eq!(layout.row_of_source(0), row);
        // The caret at the grapheme's leading boundary starts its row at cell
        // zero, and its trailing boundary starts the next row.
        assert_eq!(
            layout.caret(0),
            (row, 0, 2),
            "the caret precedes both cells"
        );
        assert_eq!(
            layout.caret(3).0,
            row + 1,
            "the caret moves to the next row"
        );
    }

    #[test]
    fn wide_glyphs_are_indivisible() {
        for value in ["界a", "你好", "👍🏽ok"] {
            for wrap in WRAPS {
                let layout = layout_for_test(value, wrap, 4);
                for item in layout.items() {
                    if item.kind != ItemKind::Glyph || item.width < 2 {
                        continue;
                    }
                    let graphemes: Vec<Range<usize>> = {
                        use unicode_segmentation::UnicodeSegmentation;
                        value
                            .grapheme_indices(true)
                            .map(|(index, grapheme)| index..index + grapheme.len())
                            .collect()
                    };
                    assert!(
                        graphemes.contains(&item.source),
                        "a wide item must span exactly one grapheme: {:?}",
                        item.source
                    );
                }
            }
        }
    }

    #[test]
    fn merging_sequences_are_single_graphemes() {
        use unicode_segmentation::UnicodeSegmentation;
        use unicode_width::UnicodeWidthStr;
        for value in [
            "👩\u{200D}💻",
            "👨\u{200D}👩\u{200D}👧\u{200D}👦",
            "👩\u{200D}",
            "👍🏽",
            "🇺🇸",
            "1\u{FE0F}\u{20E3}",
            "❤\u{FE0F}",
            "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
        ] {
            assert_eq!(value.graphemes(true).count(), 1, "{value:?} is one cluster");
            let measured = UnicodeWidthStr::width(value);
            assert!(
                (1..=2).contains(&measured),
                "{value:?} measures {measured} cells"
            );
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test(value, wrap, width);
                    assert_eq!(
                        layout.items().len(),
                        1,
                        "{value:?} {wrap:?} {width} must shape one item"
                    );
                    let item = &layout.items()[0];
                    assert_eq!(
                        item.source,
                        0..value.len(),
                        "{value:?} {wrap:?} {width} owns its whole source range"
                    );
                    assert_eq!(
                        item.width, measured,
                        "{value:?} {wrap:?} {width} keeps its measured width"
                    );
                }
            }
        }
    }

    fn separate_corpus() -> Vec<(&'static str, usize)> {
        vec![
            ("👩\u{200D}💻", 4),
            ("👨\u{200D}👩\u{200D}👧\u{200D}👦", 8),
            ("👍🏽", 4),
            ("🇺🇸", 2),
            ("1\u{FE0F}\u{20E3}", 1),
            ("❤\u{FE0F}", 1),
            ("a👩\u{200D}💻b", 6),
            ("x👍🏽y", 6),
        ]
    }

    #[test]
    fn separate_merging_measures_each_codepoint() {
        for (value, expected) in separate_corpus() {
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test_with(value, wrap, width, EmojiMerging::Separate);
                    // Wrapping only moves whole groups between rows, so the
                    // cells painted across the value are the same at every
                    // width.
                    let painted: usize = layout
                        .items()
                        .iter()
                        .filter(|item| item.kind == ItemKind::Glyph)
                        .map(|item| item.width)
                        .sum();
                    assert_eq!(
                        painted, expected,
                        "{value:?} {wrap:?} {width} must paint {expected} cells"
                    );
                    // No grapheme is ever painted in two rows: every item that
                    // shares a source range shares a row.
                    let mut ranges: Vec<Range<usize>> = layout
                        .items()
                        .iter()
                        .map(|item| item.source.clone())
                        .collect();
                    ranges.sort_by_key(|range| (range.start, range.end));
                    ranges.dedup();
                    for range in ranges {
                        let rows: Vec<usize> = layout
                            .items()
                            .iter()
                            .filter(|item| item.source == range)
                            .map(|item| item.row)
                            .collect();
                        assert!(
                            rows.windows(2).all(|pair| pair[0] == pair[1]),
                            "a grapheme split across rows for {value:?} {wrap:?} {width}"
                        );
                    }
                    // Every part still resolves to the grapheme's own
                    // boundaries, so navigation steps over the sequence whole.
                    assert_eq!(layout.clamp(value.len()), value.len());
                }
            }
        }
    }

    #[test]
    fn separate_merging_tiles_the_source_without_splitting_a_unit() {
        let sequence = "👩\u{200D}💻";
        let value = format!("ab{sequence}cd");
        for wrap in [W::Soft, W::Hard] {
            for width in [1usize, 2, 3, 4, 6] {
                let layout = layout_for_test_with(&value, wrap, width, EmojiMerging::Separate);
                let mut cursor = 0usize;
                for index in 0..layout.row_count() {
                    assert_eq!(
                        layout.row_start(index),
                        cursor,
                        "{wrap:?} {width} row start"
                    );
                    cursor = layout.row_source_end(index);
                }
                assert_eq!(cursor, value.len(), "{wrap:?} {width} rows cover the value");
                // The sequence shapes two units, each two cells, and the value
                // keeps its six cells in total.
                let parts: Vec<_> = layout
                    .items()
                    .iter()
                    .filter(|item| item.source.start >= 2 && item.source.end <= 2 + sequence.len())
                    .collect();
                assert_eq!(parts.len(), 2, "{wrap:?} {width}: two parts");
                assert!(
                    parts.iter().all(|item| item.width == 2),
                    "{wrap:?} {width}: each part is two cells"
                );
            }
        }
    }

    #[test]
    fn separate_merging_refines_the_merged_boundaries() {
        for (value, _) in separate_corpus() {
            for wrap in WRAPS {
                let merged = layout_for_test_with(value, wrap, 4, EmojiMerging::Merge);
                let separate = layout_for_test_with(value, wrap, 4, EmojiMerging::Separate);
                assert_eq!(merged.source_len(), separate.source_len(), "{value:?}");
                for layout in [&merged, &separate] {
                    let mut cursor = 0usize;
                    for index in 0..layout.row_count() {
                        assert_eq!(layout.row_start(index), cursor, "{value:?} row start");
                        cursor = layout.row_source_end(index);
                    }
                    assert_eq!(cursor, value.len(), "{value:?} rows must cover the value");
                }
                // Every merged boundary is a separate boundary.
                let mut merged_walk = 0usize;
                loop {
                    let mut candidate = 0usize;
                    let mut found = false;
                    loop {
                        if candidate == merged_walk {
                            found = true;
                            break;
                        }
                        if candidate >= value.len() {
                            break;
                        }
                        candidate = separate.next_boundary(candidate);
                    }
                    assert!(found, "{value:?}: {merged_walk} is not a separate boundary");
                    if merged_walk >= value.len() {
                        break;
                    }
                    merged_walk = merged.next_boundary(merged_walk);
                }
            }
        }
    }

    #[test]
    fn vertical_movement_keeps_a_preferred_column() {
        let layout = layout_for_test("abcdef\nxy\nabcdef", W::Soft, 3);
        assert_eq!(layout.row_count(), 5);
        // Source 5 is the last cell of row 1 ("def"); its caret column is 2.
        assert_eq!(layout.caret(5).1, 2);
        // Moving down lands at column 2 of row 2 ("xy"), whose content is only
        // two cells, so that is the row's own end.
        let (first, preferred) = layout.vertical(5, 1, None);
        assert_eq!(preferred, Some(2));
        assert_eq!(first, 9, "column 2 of the two-cell row is its end");
        assert_eq!(layout.row_of_source(first), 2);

        // Repeating the preferred column from there reaches row 3 unchanged.
        let (second, _) = layout.vertical(first, 1, preferred);
        assert_eq!(layout.row_of_source(second), 3);
        assert_eq!(layout.column(second), 2, "the preferred column is kept");

        // Moving back up returns to row 2's column 2 and then row 1's.
        let (back, _) = layout.vertical(second, -1, preferred);
        assert_eq!(layout.row_of_source(back), 2);
    }

    #[test]
    fn multi_span_source_ranges_are_global() {
        let text = Text::from_spans([Span::new("ab"), Span::new("cd"), Span::new("ef")]);
        let layout = layout_text(
            &text,
            80,
            ComputedText::default(),
            EmojiMerging::Merge,
            |parent, _| parent,
            |style| *style,
        );
        let ranges: Vec<Range<usize>> = layout.items().iter().map(|i| i.source.clone()).collect();
        assert_eq!(
            ranges,
            vec![0..1, 1..2, 2..3, 3..4, 4..5, 5..6],
            "spans must not restart source offsets"
        );
        assert_eq!(layout.source_len(), 6);
        // Every returned boundary is still valid and the row owns the range.
        assert_eq!(layout.row_source_end(0), 6);
        assert_eq!(layout.caret(4).1, 4);
    }

    #[test]
    fn painted_columns_match_item_cells_across_separators() {
        for (value, wrap, width) in [
            ("ab cd efgh", TextWrap::Soft, 6usize),
            ("hello world again", TextWrap::Soft, 8),
            ("a b c d e f g h", TextWrap::Soft, 5),
        ] {
            let layout = super::layout_for_test(value, wrap, width);
            for index in 0..layout.row_count() {
                let mut column = 0usize;
                for item in layout.row_items(index) {
                    assert_eq!(
                        item.cell, column,
                        "{value:?} row {index}: item {:?} reports cell {} but the \
                         painter reaches it at column {column}",
                        item.symbol, item.cell
                    );
                    // Every item advances, separators included: they own cells
                    // even though they are not painted.
                    column = column.saturating_add(item.width);
                }
                // The painted width is what remains after separators, but the
                // final column still counts them.
                let painted: usize = layout
                    .row_items(index)
                    .iter()
                    .filter(|item| item.kind != ItemKind::Separator)
                    .map(|item| item.width)
                    .sum();
                assert!(
                    column >= painted,
                    "the full column extent includes separators"
                );
            }
        }
    }

    #[test]
    fn debug_wide_rows() {
        for width in [4usize, 5] {
            let layout = super::layout_for_test("你好世界", TextWrap::Soft, width);
            eprintln!("width {width}: {} rows", layout.row_count());
            for index in 0..layout.row_count() {
                eprintln!(
                    "  row {index} w={} items={:?}",
                    layout.row_width(index),
                    layout
                        .row_items(index)
                        .iter()
                        .map(|i| (i.symbol.clone(), i.cell, i.width))
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn hits_are_monotonic_within_a_row() {
        for (value, wrap, width) in [
            ("ab cd efgh", TextWrap::Soft, 6usize),
            ("hello world again", TextWrap::Soft, 8),
            ("a b c d e f", TextWrap::Soft, 4),
        ] {
            let layout = super::layout_for_test(value, wrap, width);
            for index in 0..layout.row_count() {
                // Probe one cell past the painted content to cover a trailing
                // dropped separator.
                let cells = layout.row_width(index) + 2;
                let mut previous = None;
                for cell in 0..cells {
                    let hit = layout.hit(index, cell, HitBias::Leading);
                    if let Some(previous) = previous {
                        assert!(
                            hit >= previous,
                            "{value:?} row {index}: cell {cell} resolved backwards \
                             ({hit} after {previous})"
                        );
                    }
                    previous = Some(hit);
                }
            }
        }
    }

    #[test]
    fn zero_width_geometry_is_clamped() {
        for value in corpus() {
            for wrap in WRAPS {
                let layout = layout_for_test(value, wrap, 0);
                assert!(layout.row_count() >= 1);
                assert!(layout.row_count() <= value.chars().count().max(1) * 2 + 2);
            }
        }
    }
}

#[cfg(test)]
mod mode_agreement_tests {
    use super::*;

    fn layout_for(value: &str, wrap: TextWrap, width: usize) -> TextLayout {
        super::layout_for_test(value, wrap, width)
    }

    #[test]
    fn wrap_policies_have_consistent_row_counts() {
        for value in ["hello world", "abcdefghij", "abc def", "a b c d e f"] {
            let no_wrap = layout_for(value, TextWrap::NoWrap, 4);
            let soft = layout_for(value, TextWrap::Soft, 4);
            let hard = layout_for(value, TextWrap::Hard, 4);
            assert_eq!(no_wrap.row_count(), 1, "NoWrap keeps one logical line");
            // Hard wrapping fills every cell, so it never needs more rows than
            // soft wrapping, which leaves a row early to keep words whole.
            assert!(
                hard.row_count() <= soft.row_count(),
                "{value:?}: hard wrapping packs more per row than soft ({} vs {})",
                hard.row_count(),
                soft.row_count()
            );
            for layout in [&soft, &hard] {
                for index in 0..layout.row_count() {
                    assert!(
                        layout.row_width(index) <= 4,
                        "{value:?}: a wrapped row must fit the viewport"
                    );
                }
            }
            // Soft keeps whole words together where they fit.
            if value.contains(' ') {
                let rows: Vec<String> = (0..soft.row_count())
                    .map(|index| {
                        soft.row_items(index)
                            .iter()
                            .map(|item| item.symbol.clone())
                            .collect()
                    })
                    .collect();
                assert!(
                    rows.iter().all(|row| !row.starts_with(' ')),
                    "{value:?}: a soft-wrapped row does not start with a space: {rows:?}"
                );
            }
        }
    }

    #[test]
    fn navigable_ranges_survive_every_policy() {
        for value in ["hello world", "a\n\nb", "a\n", "  leading", "trailing  "] {
            for wrap in [TextWrap::NoWrap, TextWrap::Soft, TextWrap::Hard] {
                let layout = layout_for(value, wrap, 3);
                let mut position = 0usize;
                let mut guard = 0;
                let mut visited = vec![position];
                while position < value.len() {
                    position = layout.next_boundary(position);
                    visited.push(position);
                    guard += 1;
                    assert!(guard <= value.len() + 2, "the walk must terminate");
                }
                assert_eq!(position, value.len());
                // Every byte of the value is either owned by a row or is a
                // newline that a row terminates.
                let mut cursor = 0usize;
                for index in 0..layout.row_count() {
                    let end = layout.row_source_end(index);
                    assert!(end >= cursor, "rows must not overlap");
                    cursor = end;
                }
                assert_eq!(cursor, value.len(), "rows must cover {value:?}");
                assert!(
                    visited.len() >= value.len(),
                    "each byte advances the boundary walk"
                );
            }
        }
    }
}
