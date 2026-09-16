//! The document a selection is expressed against.
//!
//! A document is an ordered list of text segments tiled into one byte space by
//! an optional block separator, plus the geometry each segment was painted with.
//! Every selection operation - boundaries, clamping, word and row motion, hit
//! testing, copy slicing, and the per-leaf painting projection - is implemented
//! here once, against that description.

use std::ops::Range;
use std::sync::Arc;

use crate::basic::text::{TextAlign, TextWrap};
use crate::basic::text_layout::{ComputedText, HitBias, TextLayout, layout_text};
use crate::{EmojiMerging, ScreenPosition};

use super::DocPoint;

/// Shaping width for a value-derived layout. `NoWrap` at a width no terminal can
/// reach keeps every logical line in one row, so the boundary table is exactly
/// the value's grapheme edges - which is all horizontal selection motion reads,
/// and is why soft wrapping never changes a boundary.
const MEASURE_WIDTH: usize = usize::MAX / 4;

/// One text leaf's participation in a selection document.
#[derive(Debug, Clone)]
pub struct Segment {
    /// First document byte owned by this segment's text.
    pub doc_start: usize,
    /// Block separator bytes that precede this segment's text.
    pub separator_before: usize,
    /// The leaf's source text.
    pub value: Arc<str>,
    /// The layout the frame painted, or the value-derived layout an editor uses
    /// before its first frame is committed.
    pub layout: Arc<TextLayout>,
    /// The painted content-box origin. Document coordinates are defined by
    /// these, so a consumer may pass local or absolute points as long as the
    /// origins agree with them.
    pub origin: ScreenPosition,
    /// The painted content-box width in cells.
    pub width: usize,
    /// The painted content-box height in rows.
    pub height: usize,
    /// The leaf's horizontal alignment. Centered and end-aligned text is painted
    /// inset within its box, so the engine has to apply the same inset before it
    /// maps a point to a cell or a cell to a point.
    pub align: TextAlign,
    /// Whether the frame painted any of this segment inside its clip.
    pub visible: bool,
}

impl Segment {
    /// Build a segment whose geometry is derived from the value alone.
    pub fn from_value(value: String, merging: EmojiMerging) -> Self {
        let layout = value_layout(&value, merging);
        Self {
            doc_start: 0,
            separator_before: 0,
            value: Arc::from(value),
            layout: Arc::new(layout),
            origin: ScreenPosition::new(0, 0),
            width: 0,
            height: 1,
            align: TextAlign::Start,
            visible: true,
        }
    }

    /// First document byte after this segment's text.
    pub fn content_end(&self) -> usize {
        self.doc_start + self.value.len()
    }

    /// The painted inset of one row, matching the painter's own alignment math
    /// exactly: the row is centered or pushed to the end of the segment's box.
    fn row_inset(&self, row: usize) -> usize {
        let line_width = self.layout.row_width(row);
        match self.align {
            TextAlign::Start => 0,
            TextAlign::Center => self.width.saturating_sub(self.width.min(line_width)) / 2,
            TextAlign::End => self.width.saturating_sub(line_width),
        }
    }
}

/// The separator rule, in one place: two segments that begin on a later painted
/// line are separate blocks and copy with a newline between them; segments
/// sharing a line are inline and concatenate with nothing.
pub fn block_separator(previous_last_line: Option<i32>, first_line: i32) -> usize {
    match previous_last_line {
        Some(last) if first_line > last => 1,
        _ => 0,
    }
}

/// One painted text leaf, as the painter saw it.
#[derive(Debug, Clone)]
pub struct Run {
    /// The leaf's source text.
    pub value: Arc<str>,
    /// The layout the frame painted.
    pub layout: Arc<TextLayout>,
    /// The painted content-box origin.
    pub origin: ScreenPosition,
    /// The painted content-box width in cells.
    pub width: usize,
    /// The painted content-box height in rows.
    pub height: usize,
    /// The leaf's horizontal alignment.
    pub align: TextAlign,
    /// Whether the frame painted any of this run inside its clip.
    pub visible: bool,
}

impl Run {
    /// A run with start alignment and visible set, taking its painted geometry.
    pub fn new(
        value: Arc<str>,
        layout: Arc<TextLayout>,
        origin: ScreenPosition,
        width: usize,
        height: usize,
    ) -> Self {
        Self {
            value,
            layout,
            origin,
            width,
            height,
            align: TextAlign::Start,
            visible: true,
        }
    }

    /// Set the horizontal alignment.
    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Set whether the run was painted.
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

/// Incrementally assembles a document in paint order, owning the tiling so no
/// caller can produce overlapping or gapped byte ranges.
pub struct DocumentBuilder {
    segments: Vec<Segment>,
    merging: EmojiMerging,
    viewport_height: usize,
    cursor: usize,
    last_line: Option<i32>,
}

impl DocumentBuilder {
    /// An empty builder for the given emoji merging mode.
    pub fn new(merging: EmojiMerging) -> Self {
        Self {
            segments: Vec::new(),
            merging,
            viewport_height: 1,
            cursor: 0,
            last_line: None,
        }
    }

    /// Set the viewport height in rows used for page motion, clamped to at
    /// least 1.
    pub fn with_viewport_height(mut self, height: usize) -> Self {
        self.viewport_height = height.max(1);
        self
    }

    /// Append one painted leaf and return the document offset its text starts
    /// at, so the painter can project the selection onto it immediately.
    pub fn push(&mut self, run: Run) -> usize {
        let first_line = run.origin.line;
        let separator = block_separator(self.last_line, first_line);
        let doc_start = self.cursor + separator;
        let rows = run.layout.row_count().max(1) as i32;
        self.last_line = Some(first_line.saturating_add(rows - 1));
        self.cursor = doc_start + run.value.len();
        self.segments.push(Segment {
            doc_start,
            separator_before: separator,
            value: run.value,
            layout: run.layout,
            origin: run.origin,
            width: run.width,
            height: run.height,
            align: run.align,
            visible: run.visible,
        });
        doc_start
    }

    /// Assemble the pushed segments into a document.
    pub fn finish(self) -> SelectionDocument {
        SelectionDocument::assemble(self.segments, self.merging, self.viewport_height)
    }
}

/// An ordered description of every text leaf a selection spans.
#[derive(Debug, Clone)]
pub struct SelectionDocument {
    segments: Vec<Segment>,
    /// Total document length in bytes, including block separators.
    len: usize,
    /// Every valid caret position, ascending. They are the segments' own
    /// boundary tables shifted into document coordinates, which keeps the
    /// editor and a selectable region on one definition of a caret stop.
    boundaries: Vec<usize>,
    /// `prefix_rows[i]` is the number of painted rows before segment `i`, so a
    /// document offset maps to one flat row index across all segments.
    prefix_rows: Vec<usize>,
    merging: EmojiMerging,
    viewport_height: usize,
}

impl Default for SelectionDocument {
    fn default() -> Self {
        Self {
            segments: Vec::new(),
            len: 0,
            boundaries: vec![0],
            prefix_rows: Vec::new(),
            merging: EmojiMerging::default(),
            viewport_height: 1,
        }
    }
}

impl SelectionDocument {
    /// Recompute the tiling and the document-wide boundary and row tables, so a
    /// hand-built run list cannot disagree with the byte offsets every other
    /// operation uses. The far edge of a block separator is a caret stop in its
    /// own right; its near edge is the previous segment's last boundary.
    fn assemble(mut segments: Vec<Segment>, merging: EmojiMerging, viewport_height: usize) -> Self {
        let mut cursor = 0usize;
        for segment in &mut segments {
            segment.doc_start = cursor + segment.separator_before;
            cursor = segment.content_end();
        }
        let len = segments.last().map_or(0, Segment::content_end);
        let mut boundaries = Vec::with_capacity(segments.len() * 8);
        let mut prefix_rows = Vec::with_capacity(segments.len());
        let mut rows = 0usize;
        for segment in &segments {
            prefix_rows.push(rows);
            rows = rows.saturating_add(segment.layout.row_count());
            for boundary in segment.layout.boundaries() {
                boundaries.push(segment.doc_start + boundary);
            }
            if segment.separator_before > 0 {
                boundaries.push(segment.doc_start);
            }
        }
        boundaries.push(0);
        boundaries.push(len);
        boundaries.sort_unstable();
        boundaries.dedup();
        Self {
            segments,
            len,
            boundaries,
            prefix_rows,
            merging,
            viewport_height: viewport_height.max(1),
        }
    }

    /// The one-segment document an editor uses when its painted layout is known;
    /// `viewport_height` is clamped to at least 1.
    pub fn single(
        value: Arc<str>,
        layout: Arc<TextLayout>,
        merging: EmojiMerging,
        viewport_height: usize,
    ) -> Self {
        let height = viewport_height.max(1);
        let mut builder = DocumentBuilder::new(merging).with_viewport_height(height);
        builder.push(Run::new(
            value,
            layout,
            ScreenPosition::new(0, 0),
            0,
            height,
        ));
        builder.finish()
    }

    /// The one-segment document an editor uses before any frame is committed,
    /// with geometry derived from the value.
    pub fn from_value(value: String, merging: EmojiMerging) -> Self {
        let mut builder = DocumentBuilder::new(merging);
        let segment = Segment::from_value(value, merging);
        builder.push(Run::new(
            segment.value,
            segment.layout,
            segment.origin,
            segment.width,
            segment.height,
        ));
        builder.finish()
    }

    /// Assemble segments a paint pass already collected.
    pub fn from_runs(
        segments: Vec<Segment>,
        merging: EmojiMerging,
        viewport_height: usize,
    ) -> Self {
        Self::assemble(segments, merging, viewport_height)
    }

    /// Total document length in bytes, including block separators.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the document has no segments.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The document's ordered segments.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The emoji merging mode the document was built with.
    pub fn merging(&self) -> EmojiMerging {
        self.merging
    }

    /// The viewport height in rows used for page motion.
    pub fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    /// Every valid caret position, ascending.
    pub fn boundaries(&self) -> &[usize] {
        &self.boundaries
    }

    /// Snap an arbitrary byte offset to the nearest caret position. Ties
    /// resolve forward, which is the rule `TextLayout`'s pointer path already
    /// uses, so programmatic and pointer placement agree.
    pub fn clamp(&self, offset: usize) -> usize {
        if self.boundaries.is_empty() {
            return 0;
        }
        let offset = offset.min(self.len);
        match self.boundaries.binary_search(&offset) {
            Ok(_) => offset,
            Err(at) => {
                if at == 0 {
                    return self.boundaries[0];
                }
                if at >= self.boundaries.len() {
                    return *self.boundaries.last().expect("non-empty boundaries");
                }
                let before = self.boundaries[at - 1];
                let after = self.boundaries[at];
                if offset - before < after - offset {
                    before
                } else {
                    after
                }
            }
        }
    }

    /// The caret position before `offset`, or 0 at the document start.
    pub fn previous_boundary(&self, offset: usize) -> usize {
        if self.boundaries.is_empty() {
            return 0;
        }
        let offset = self.clamp(offset);
        match self.boundaries.binary_search(&offset) {
            Ok(0) => 0,
            Ok(at) => self.boundaries[at - 1],
            Err(at) if at > 0 => self.boundaries[at - 1],
            Err(_) => 0,
        }
    }

    /// The caret position after `offset`, or `offset` at the document end.
    pub fn next_boundary(&self, offset: usize) -> usize {
        if self.boundaries.is_empty() {
            return 0;
        }
        let offset = self.clamp(offset);
        match self.boundaries.binary_search(&offset) {
            Ok(at) => self.boundaries.get(at + 1).copied().unwrap_or(offset),
            Err(at) => self.boundaries.get(at).copied().unwrap_or(offset),
        }
    }

    /// The index of the segment that owns a document offset; offsets sitting on
    /// a segment boundary belong to the earlier segment.
    pub fn segment_of(&self, offset: usize) -> usize {
        self.segments
            .iter()
            .rposition(|segment| segment.doc_start <= offset)
            .unwrap_or_default()
    }

    /// Shift a document offset into a segment's own byte coordinates.
    fn local_of(&self, segment: &Segment, offset: usize) -> usize {
        offset
            .saturating_sub(segment.doc_start)
            .min(segment.value.len())
    }

    /// Start of the logical line containing `offset`. Logical lines - not
    /// visual rows - are what Home/End mean, which is the editor's existing
    /// behavior and stays correct under soft wrapping.
    pub fn line_start(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        if self.segments.is_empty() {
            return 0;
        }
        let segment = &self.segments[self.segment_of(offset)];
        let local = self.local_of(segment, offset);
        let start = segment.value[..local]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        segment.doc_start + start
    }

    /// End of the logical line containing `offset`, before its newline.
    pub fn line_end(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        if self.segments.is_empty() {
            return 0;
        }
        let segment = &self.segments[self.segment_of(offset)];
        let local = self.local_of(segment, offset);
        let end = segment.value[local..]
            .find('\n')
            .map_or(segment.value.len(), |index| local + index);
        segment.doc_start + end
    }

    /// Move one word toward the document start, crossing into the previous
    /// segment at the edge. The word scan itself is the editor's existing
    /// grapheme-unit rule, now shared.
    pub fn word_before(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        if self.segments.is_empty() {
            return 0;
        }
        let index = self.segment_of(offset);
        let segment = &self.segments[index];
        let local = self.local_of(segment, offset);
        let target = word_before_in(&segment.value, local, self.merging);
        if local == 0 && target == 0 && index > 0 {
            return self.segments[index - 1].content_end();
        }
        segment.doc_start + target
    }

    /// Move one word toward the document end, crossing into the next segment at
    /// the edge.
    pub fn word_after(&self, offset: usize) -> usize {
        let offset = self.clamp(offset);
        if self.segments.is_empty() {
            return 0;
        }
        let index = self.segment_of(offset);
        let segment = &self.segments[index];
        let local = self.local_of(segment, offset);
        let target = word_after_in(&segment.value, local, self.merging);
        if local == segment.value.len()
            && target == segment.value.len()
            && index + 1 < self.segments.len()
        {
            return self.segments[index + 1].doc_start;
        }
        segment.doc_start + target
    }

    /// Resolve a point inside one painted leaf to a document offset, applying
    /// the leaf's alignment inset before the hit test.
    fn hit_in(&self, segment: &Segment, line: i32, column: i32, bias: HitBias) -> usize {
        let row = (line - segment.origin.line).max(0) as usize;
        let row = row.min(segment.layout.row_count().saturating_sub(1));
        let inset = segment.row_inset(row) as i32;
        let cell = (column - segment.origin.column - inset).max(0) as usize;
        segment.doc_start + segment.layout.hit(row, cell, bias)
    }

    /// Resolve a point or an offset to a document position.
    pub fn hit(&self, point: DocPoint, bias: HitBias) -> usize {
        match point {
            DocPoint::Offset(offset) => self.clamp(offset),
            DocPoint::Point { line, column } => self.hit_point(line, column, bias),
        }
    }

    /// Resolve a painted point to a document offset. A point inside a leaf
    /// belongs to that leaf, one above every leaf is the document start, one
    /// below every leaf is the document end, and a point in the gap between two
    /// leaves is owned by the last leaf that begins at or above it.
    fn hit_point(&self, line: i32, column: i32, bias: HitBias) -> usize {
        let visible: Vec<&Segment> = self
            .segments
            .iter()
            .filter(|segment| segment.visible)
            .collect();
        let Some(first) = visible.first().copied() else {
            return self.clamp(0);
        };
        if line < first.origin.line {
            return first.doc_start;
        }
        if let Some(segment) = visible.iter().rev().find(|segment| {
            let top = segment.origin.line;
            line >= top && line < top.saturating_add(segment.height.max(1) as i32)
        }) {
            return self.hit_in(segment, line, column, bias);
        }
        let last = *visible.last().expect("non-empty visible segments");
        if line >= last.origin.line.saturating_add(last.height.max(1) as i32) {
            return self.len;
        }
        let nearest = visible
            .iter()
            .rev()
            .find(|segment| segment.origin.line <= line)
            .copied()
            .unwrap_or(first);
        self.hit_in(nearest, line, column, bias)
    }

    /// The painted caret for a document offset.
    pub fn caret(&self, offset: usize) -> CaretPoint {
        if self.segments.is_empty() {
            return CaretPoint::default();
        }
        let offset = self.clamp(offset);
        let index = self.segment_of(offset);
        let segment = &self.segments[index];
        let local = self.local_of(segment, offset);
        let (row, cell, width) = segment.layout.caret(local);
        let cell = cell.saturating_add(segment.row_inset(row));
        CaretPoint {
            segment: index,
            row,
            cell,
            width,
            line: segment.origin.line.saturating_add(row as i32),
            column: segment.origin.column.saturating_add(cell as i32),
        }
    }

    /// Flat painted-row index of a document offset, across every segment.
    pub fn row_of(&self, offset: usize) -> usize {
        if self.segments.is_empty() {
            return 0;
        }
        let offset = self.clamp(offset);
        let index = self.segment_of(offset);
        let segment = &self.segments[index];
        let local = self.local_of(segment, offset);
        self.prefix_rows[index].saturating_add(segment.layout.row_of_source(local))
    }

    /// Cell of a document offset inside its own segment.
    pub fn cell_of(&self, offset: usize) -> usize {
        if self.segments.is_empty() {
            return 0;
        }
        let offset = self.clamp(offset);
        let segment = &self.segments[self.segment_of(offset)];
        let local = self.local_of(segment, offset);
        segment.layout.caret(local).1
    }

    /// Total painted rows across every segment.
    pub fn row_count(&self) -> usize {
        self.prefix_rows.last().copied().unwrap_or(0)
            + self
                .segments
                .last()
                .map_or(0, |segment| segment.layout.row_count())
    }

    /// Resolve a flat painted row at a preferred cell to a document offset.
    /// Leading bias matches the editor's vertical motion, so a caret crossing
    /// rows lands where the user sees it.
    pub fn hit_row(&self, flat: usize, cell: usize) -> usize {
        if self.segments.is_empty() {
            return 0;
        }
        let flat = flat.min(self.row_count().saturating_sub(1));
        let index = match self.prefix_rows.binary_search(&flat) {
            Ok(index) => index,
            Err(next) => next.saturating_sub(1),
        };
        let segment = &self.segments[index];
        let row = flat.saturating_sub(self.prefix_rows[index]);
        segment.doc_start + segment.layout.hit(row, cell, HitBias::Leading)
    }

    /// The document bytes in `range`, with block separators rendered as
    /// newlines. Ranges come from caret boundaries, so the slice is always on
    /// character edges.
    pub fn slice(&self, range: Range<usize>) -> String {
        let mut out = String::new();
        for segment in &self.segments {
            let separator_start = segment.doc_start.saturating_sub(segment.separator_before);
            let content_end = segment.content_end();
            if range.end <= separator_start {
                break;
            }
            if range.start < segment.doc_start {
                let from = range.start.max(separator_start);
                let to = range.end.min(segment.doc_start);
                for _ in from..to {
                    out.push('\n');
                }
            }
            if range.start < content_end && range.end > segment.doc_start {
                let value_start = range.start.max(segment.doc_start) - segment.doc_start;
                let value_end = range.end.min(content_end) - segment.doc_start;
                if value_end > value_start {
                    out.push_str(&segment.value[value_start..value_end]);
                }
            }
        }
        out
    }
}

/// A caret position resolved against a document, in document row/cell space.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaretPoint {
    /// Index of the owning segment.
    pub segment: usize,
    /// Row within the owning segment.
    pub row: usize,
    /// Painted cell within the owning segment, including its alignment inset.
    pub cell: usize,
    /// Painted width in cells of the glyph at the caret.
    pub width: usize,
    /// Painted line in document coordinates.
    pub line: i32,
    /// Painted column in document coordinates.
    pub column: i32,
}

/// The value-derived layout an editor uses before its first committed frame:
/// no wrapping, measured at a width no terminal can reach.
pub fn value_layout(value: &str, merging: EmojiMerging) -> TextLayout {
    layout_text(
        &crate::basic::text::Text::new(value).wrap(TextWrap::NoWrap),
        MEASURE_WIDTH,
        ComputedText::default(),
        merging,
        |parent, _| parent,
        |style| *style,
    )
}

/// The byte offset of the start of the word preceding `offset`, or `offset`
/// when the scan crosses no word.
fn word_before_in(value: &str, offset: usize, merging: EmojiMerging) -> usize {
    let offset = offset.min(value.len());
    let mut boundary = 0usize;
    let mut seen_word = false;
    for unit in crate::data::display_units(value, merging) {
        if unit.start >= offset {
            break;
        }
        if is_word_unit(&value[unit.clone()]) {
            if !seen_word {
                boundary = unit.start;
            }
            seen_word = true;
        } else {
            seen_word = false;
        }
    }
    if seen_word { boundary } else { offset }
}

/// The byte offset just past the word containing or following `offset`.
fn word_after_in(value: &str, offset: usize, merging: EmojiMerging) -> usize {
    let offset = offset.min(value.len());
    let mut seen_word = false;
    for unit in crate::data::display_units(value, merging) {
        if unit.end <= offset {
            continue;
        }
        if is_word_unit(&value[unit.clone()]) {
            seen_word = true;
        } else if seen_word {
            return unit.start;
        }
    }
    value.len()
}

/// Whether a grapheme unit contains an alphanumeric character.
fn is_word_unit(unit: &str) -> bool {
    unit.chars().any(char::is_alphanumeric)
}
