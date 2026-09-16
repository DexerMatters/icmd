//! Shaping and layout of a text value: wraps it into painted rows and builds the
//! caret-boundary and hit-testing tables every consumer (painting, navigation,
//! and selection) reads.

use std::ops::Range;

use textwrap::core::Fragment;
use textwrap::wrap_algorithms::wrap_first_fit;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{EmojiMerging, MAX_GLYPH_BYTES};

use super::props::TextStyle;
use super::text::{Text, TextWrap};

/// Cells a tab advances to the next multiple of, measured from the logical line
/// column so the width is independent of where a soft wrap put it.
pub const TAB_WIDTH: usize = 4;

/// Which side of a cell a hit test resolves a caret to when the cell holds a
/// wide grapheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitBias {
    /// Resolve to the boundary at the cell's leading edge.
    #[default]
    Leading,
    /// Resolve to the boundary at the cell's trailing edge.
    Trailing,
}

/// What one shaped glyph contributes to a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// A painted grapheme.
    Glyph,
    /// A soft-wrap space that owns source bytes but paints no cells.
    Separator,
    /// An explicit line break that owns no item.
    Newline,
}

/// Resolved foreground, background, and attributes for a glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputedText {
    /// The glyph colour.
    pub foreground: crossterm::style::Color,
    /// The glyph background, or `None` for the terminal default.
    pub background: Option<crossterm::style::Color>,
    /// The glyph's text attributes.
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

/// One materialized item of a laid-out row.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Item {
    /// Byte range in the source text that this item covers.
    pub source: Range<usize>,
    /// Byte range into the layout's normalized text, from which the glyph's
    /// rendered symbol is resolved rather than stored per item.
    pub text: Range<usize>,
    /// Row-relative starting cell.
    pub cell: usize,
    /// Width in cells.
    pub width: usize,
    /// What the item contributes to the row.
    pub kind: ItemKind,
    /// Index of the owning row.
    pub row: usize,
    /// Resolved style for the item.
    pub style: ComputedText,
}

impl Item {
    /// The symbol this item paints within `text`. Empty for a separator, which
    /// occupies cells but is deliberately not drawn.
    pub fn symbol<'a>(&self, text: &'a str) -> &'a str {
        text.get(self.text.clone()).unwrap_or("")
    }

    /// Whether the item resolves to an empty symbol.
    pub fn is_empty_symbol(&self, text: &str) -> bool {
        self.symbol(text).is_empty()
    }

    /// Whether the item's symbol is a tab character.
    #[allow(dead_code)]
    pub fn is_tab(&self, text: &str) -> bool {
        self.symbol(text) == "\t"
    }
}

/// One visual row of the layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Row {
    /// Index of the row's first item.
    pub first_item: usize,
    /// Number of items in the row.
    pub len: usize,
    /// First source byte the row paints.
    pub source_start: usize,
    /// First source byte after the row.
    pub source_end: usize,
}

/// One shaped glyph before it is packed into a row.
#[derive(Debug, Clone)]
pub struct ShapedGlyph<S> {
    /// Byte range in the source text.
    pub source: Range<usize>,
    /// Byte range into the shared normalized text built during shaping; glyphs
    /// refer to ranges instead of owning a `String`, so shaping performs one
    /// backing allocation rather than one per glyph.
    pub text: Range<usize>,
    /// Width in cells.
    pub width: usize,
    /// What the glyph contributes.
    pub kind: ItemKind,
    /// Style produced by the merge callback.
    pub style: S,
}

/// A shaped value laid out into painted rows, with the caret and hit-testing
/// tables built once at construction.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct TextLayout {
    text: String,
    items: Vec<Item>,
    rows: Vec<Row>,
    boundaries: Vec<usize>,
    /// Row source ranges are non-decreasing, so a row lookup is a binary search
    /// rather than a scan; cached with each row's painted and maximum width.
    row_source_ends: Vec<usize>,
    row_widths: Vec<usize>,
    max_row_width: usize,
    width: usize,
    source_len: usize,
}

/// Shape `text` and lay it out at `width` cells, resolving each glyph's style
/// through `merge` and then `into_computed`.
pub fn layout_text<S>(
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

impl TextLayout {
    /// Pack shaped glyphs into wrapped rows and build the caret and hit-testing
    /// tables. `width` is clamped to at least 1; `wrap` chooses the wrapping
    /// rule. Tabs advance from the logical line column, empty logical lines stay
    /// as empty rows, explicit newlines own no item, and a source position past
    /// the last shaped glyph falls back to the end of the value.
    pub fn layout<S>(
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

        let mut logical_lines: Vec<Range<usize>> = Vec::new();
        let mut line_start = 0usize;
        for (index, glyph) in shaped.iter().enumerate() {
            if glyph.kind == ItemKind::Newline {
                logical_lines.push(line_start..index + 1);
                line_start = index + 1;
            }
        }
        logical_lines.push(line_start..shaped.len());

        struct Placed {
            range: Range<usize>,
            source_start: usize,
            pieces: Vec<Piece>,
            is_last_row: bool,
        }
        let source_at = |index: usize| {
            shaped
                .get(index)
                .map_or(source_len, |glyph| glyph.source.start)
        };
        let mut placed: Vec<Placed> = Vec::new();
        for logical in &logical_lines {
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

        let mut boundaries = Vec::with_capacity(items.len() * 2 + 4);
        boundaries.push(0usize);
        for glyph in &shaped {
            boundaries.push(glyph.source.start);
            boundaries.push(glyph.source.end);
        }
        boundaries.push(source_len);
        boundaries.sort_unstable();
        boundaries.dedup();

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

    /// Number of visual rows.
    #[allow(dead_code)]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Painted width of the widest row in cells.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Length of the source text in bytes.
    #[allow(dead_code)]
    pub fn source_len(&self) -> usize {
        self.source_len
    }

    /// The normalized text every item's `text` range indexes.
    #[allow(dead_code)]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// All items in visual order.
    #[allow(dead_code)]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The items of one row, or an empty slice when the index is out of range.
    pub fn row_items(&self, index: usize) -> &[Item] {
        self.rows.get(index).map_or(&[], |row| {
            &self.items[row.first_item..row.first_item + row.len]
        })
    }

    /// First source byte the row paints.
    pub fn row_start(&self, index: usize) -> usize {
        self.rows
            .get(index)
            .map_or(self.source_len, |row| row.source_start)
    }

    /// First source byte after the row's last non-empty glyph.
    #[allow(dead_code)]
    pub fn row_end(&self, index: usize) -> usize {
        self.row_items(index)
            .iter()
            .rfind(|item| item.kind == ItemKind::Glyph && !item.is_empty_symbol(&self.text))
            .map_or_else(|| self.row_start(index), |item| item.source.end)
    }

    /// First source byte after the row.
    #[allow(dead_code)]
    pub fn row_source_end(&self, index: usize) -> usize {
        self.rows
            .get(index)
            .map_or(self.source_len, |row| row.source_end)
    }

    /// Every valid caret boundary, in ascending order. The selection engine
    /// builds its document-wide table from these, so the editor and a selectable
    /// region share exactly one definition of where a caret may sit.
    pub(crate) fn boundaries(&self) -> &[usize] {
        &self.boundaries
    }

    /// Painted cell width of one row, separators excluded. A separator is
    /// deliberately not painted, so it contributes no cells even though it owns
    /// source bytes; the width is cached at construction.
    pub fn row_width(&self, index: usize) -> usize {
        self.row_widths.get(index).copied().unwrap_or(0)
    }

    /// Widest painted row in cells.
    pub fn max_row_width(&self) -> usize {
        self.max_row_width
    }

    /// Index of the row owning a source byte; a position past the end is the
    /// last row.
    pub fn row_of_source(&self, source: usize) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        let source = source.min(self.source_len);
        match self.row_source_ends.partition_point(|end| *end <= source) {
            index if index < self.rows.len() => index,
            _ => self.rows.len() - 1,
        }
    }

    /// Snap an arbitrary source offset to the nearest caret boundary, preferring
    /// the nearer boundary; ties resolve forward so repeated stepping always
    /// advances.
    pub fn clamp(&self, source: usize) -> usize {
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
                if source - before < after - source {
                    before
                } else {
                    after
                }
            }
        }
    }

    /// The caret boundary before `source`, or 0 at the start.
    #[allow(dead_code)]
    pub fn previous_boundary(&self, source: usize) -> usize {
        let source = self.clamp(source);
        match self.boundaries.binary_search(&source) {
            Ok(0) => 0,
            Ok(at) => self.boundaries[at - 1],
            Err(at) if at > 0 => self.boundaries[at - 1],
            Err(_) => 0,
        }
    }

    /// The caret boundary after `source`, or the clamped position at the end.
    #[allow(dead_code)]
    pub fn next_boundary(&self, source: usize) -> usize {
        let clamped = self.clamp(source);
        if clamped > source {
            return clamped;
        }
        match self.boundaries.binary_search(&clamped) {
            Ok(at) => self.boundaries.get(at + 1).copied().unwrap_or(clamped),
            Err(at) => self.boundaries.get(at).copied().unwrap_or(clamped),
        }
    }

    /// The `(row, cell, width)` caret for a source offset, where `width` is the
    /// painted width of the glyph at the caret. A position in a dropped
    /// separator or on an explicit newline belongs to the row that owns those
    /// bytes, not the next row.
    pub fn caret(&self, source: usize) -> (usize, usize, usize) {
        if self.rows.is_empty() {
            return (0, 0, 1);
        }
        let source = self.clamp(source);
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
                return (row, cell, item.width.max(1));
            }
            if source < item.source.end {
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

    /// Resolve a cell in a row to a source offset. `bias` picks the side of a
    /// wide grapheme, and the result never leaves the hit row, so a boundary
    /// shared with the previous row cannot place a caret on the wrong line.
    pub fn hit(&self, row: usize, cell: usize, bias: HitBias) -> usize {
        let row = row.min(self.rows.len().saturating_sub(1));
        let mut offset = 0usize;
        for item in self.row_items(row) {
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
        self.row_last_boundary(row)
    }

    /// Source offset at the end of the row's last item.
    pub(crate) fn row_last_boundary(&self, row: usize) -> usize {
        self.row_items(row)
            .last()
            .map_or_else(|| self.row_start(row), |item| item.source.end)
    }

    /// Painted cell of a source offset.
    #[allow(dead_code)]
    pub fn column(&self, source: usize) -> usize {
        let (_, cell, _) = self.caret(source);
        cell
    }

    /// Step one row in `direction` (`-1` up, `1` down), holding `preferred` cell
    /// when given, and return the new source offset plus the preferred cell.
    #[allow(dead_code)]
    pub fn vertical(
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

/// Pack the glyphs of one logical line into pieces, using the explicit newline
/// as a hard break and applying the wrap rule to the content before it.
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
    if content_last < last {
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

/// One piece per glyph, the hard-wrap rule.
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

/// Whether a glyph is trailing whitespace dropped by a soft wrap: the last piece
/// packed onto its row, on a row that is not the logical line's last.
fn item_is_dropped_separator(pieces: &[Piece], index: usize, is_last_row: bool) -> bool {
    if is_last_row {
        return false;
    }
    let position = pieces.partition_point(|piece| piece.start <= index);
    let Some(piece) = position.checked_sub(1).and_then(|at| pieces.get(at)) else {
        return false;
    };
    let is_last_piece = position >= pieces.len();
    is_last_piece && index >= piece.content_end
}

/// Painted cells of a row, counting glyph items only.
fn row_cells(items: &[Item], row: &Row) -> usize {
    items[row.first_item..row.first_item + row.len]
        .iter()
        .filter(|item| item.kind == ItemKind::Glyph)
        .fold(0usize, |sum, item| sum.saturating_add(item.width))
}

/// One packing piece of a logical line, with its trailing whitespace separated
/// from its content so the wrapper can drop it at a soft break.
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

/// Split the content before an explicit newline into word pieces, preserving
/// trailing whitespace on the logical line and breaking an oversized word
/// between glyphs.
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

/// Sum of the glyph widths in `start..end`, ignoring out-of-range indexes.
fn sum_widths(widths: &[usize], start: usize, end: usize) -> usize {
    widths[start.min(widths.len())..end.min(widths.len())]
        .iter()
        .fold(0usize, |sum, width| sum.saturating_add(*width))
}

/// Test-only handle exposing private layout accessors.
#[doc(hidden)]
pub struct TextLayoutForTest(pub TextLayout);

impl TextLayoutForTest {
    /// Number of visual rows.
    pub fn row_count(&self) -> usize {
        self.0.rows.len()
    }

    /// Index of the row owning a source byte.
    pub fn row_of_source(&self, source: usize) -> usize {
        self.0.row_of_source(source)
    }

    /// First source byte after the row.
    pub fn row_source_end(&self, index: usize) -> usize {
        self.0.row_source_end(index)
    }

    /// Painted cell width of one row, separators excluded.
    pub fn row_width(&self, index: usize) -> usize {
        self.0.row_width(index)
    }

    /// Widest painted row in cells.
    pub fn max_row_width(&self) -> usize {
        self.0.max_row_width()
    }
}

/// Build a layout for tests at `width` cells, splitting emoji rather than
/// merging them.
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

mod shape;
use shape::shape_text;
