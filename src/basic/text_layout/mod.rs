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
    // Byte range into the layout's normalized text. The glyph's rendered symbol
    // is resolved from here rather than stored per item, which removed a heap
    // allocation and a copy per glyph.
    pub(crate) text: Range<usize>,
    pub(crate) cell: usize,
    pub(crate) width: usize,
    pub(crate) kind: ItemKind,
    pub(crate) row: usize,
    pub(crate) style: ComputedText,
}

impl Item {
    // The symbol this item paints within `text`. Empty for a separator, which
    // occupies cells but is deliberately not drawn.
    pub(crate) fn symbol<'a>(&self, text: &'a str) -> &'a str {
        text.get(self.text.clone()).unwrap_or("")
    }

    pub(crate) fn is_empty_symbol(&self, text: &str) -> bool {
        self.symbol(text).is_empty()
    }

    #[allow(dead_code)] // Used by the rasterization paths and their tests.
    pub(crate) fn is_tab(&self, text: &str) -> bool {
        self.symbol(text) == "\t"
    }
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
    // Byte range into the shared normalized text built during shaping. Glyphs
    // refer to ranges instead of owning a `String`, so shaping performs one
    // backing allocation rather than one per glyph.
    pub text: Range<usize>,
    pub width: usize,
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
    TextLayout::layout(
        shaped.normalized,
        shaped.glyphs,
        text.wrap,
        width,
        into_computed,
    )
}

// Shaping output: one shared normalized buffer plus glyph records that index
// into it.
struct ShapedText<S> {
    normalized: String,
    glyphs: Vec<ShapedGlyph<S>>,
}

fn shape_text<S>(
    text: &Text,
    inherited: ComputedText,
    merging: EmojiMerging,
    merge: impl Fn(ComputedText, &TextStyle) -> S,
) -> ShapedText<S>
where
    S: Clone,
{
    crate::runtime::metrics::note_text_shaping();
    let mut normalized = String::new();
    let mut shaped = Vec::new();
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
                    let start = normalized.len();
                    normalized.push_str(&grapheme[range.clone()]);
                    shaped.push(ShapedGlyph {
                        source: source_origin + source_index + range.start
                            ..source_origin + source_index + range.end,
                        text: start..normalized.len(),
                        width,
                        kind: ItemKind::Glyph,
                        style: style.clone(),
                    });
                }
                continue;
            }
            let (symbol, width, kind) = display_glyph(grapheme);
            let start = normalized.len();
            normalized.push_str(&symbol);
            shaped.push(ShapedGlyph {
                source: source_origin + source_index..source_origin + source_index + grapheme.len(),
                text: start..normalized.len(),
                width,
                kind,
                style: style.clone(),
            });
        }
        source_origin += content.len();
    }
    ShapedText {
        normalized,
        glyphs: shaped,
    }
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
        text: String,
        shaped: Vec<ShapedGlyph<S>>,
        wrap: TextWrap,
        width: usize,
        into_computed: impl Fn(&S) -> ComputedText,
    ) -> Self
    where
        S: Clone,
    {
        let width = width.max(1);
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
            let width = if &text[glyph.text.clone()] == "\t" {
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
                &text,
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
                    text: glyph.text.clone(),
                    cell,
                    width: item_width,
                    kind,
                    row: row_index,
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
            .rfind(|item| item.kind == ItemKind::Glyph && !item.is_empty_symbol(&self.text))
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
    normalized: &str,
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
    let mut pieces = soft_pieces(normalized, shaped, widths, first, content_last, width);
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
    normalized: &str,
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
        source.push_str(&normalized[glyph.text.clone()]);
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
    let last_symbol = &normalized[shaped[content_last - 1].text.clone()];
    let preserve_trailing = last_symbol.chars().all(char::is_whitespace) && !last_symbol.is_empty();
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
mod tests;
