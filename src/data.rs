//! Cell, image, and frame data types shared by the terminal surface.
//!
//! Owns the validated [`Cell`] glyph, the [`Image`] cell grid and its patch
//! operations, and the [`Frame`] operation vocabulary.

use crossterm::style::{Attributes, Color};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, OnceLock};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::RasterPlacement;

/// Maps the shared glyph error onto the cell-specific public error; the
/// variants are one-to-one and only the domain wrapper differs.
pub(crate) fn cell_error_from_glyph(error: crate::glyph::GlyphError) -> CellError {
    match error {
        crate::glyph::GlyphError::Empty => CellError::Empty,
        crate::glyph::GlyphError::SymbolTooLong(bytes) => CellError::SymbolTooLong(bytes),
        crate::glyph::GlyphError::MultipleGraphemes => CellError::MultipleGraphemes,
        crate::glyph::GlyphError::ControlCharacter => CellError::ControlCharacter,
        crate::glyph::GlyphError::UnsupportedWidth(width) => CellError::UnsupportedWidth(width),
    }
}

/// Maximum number of cells a single image surface may hold.
pub const MAX_SURFACE_CELLS: usize = 1_048_576;

/// Maximum encoded byte length of one glyph symbol.
pub const MAX_GLYPH_BYTES: usize = 256;

/// Whether an emoji sequence is painted as one unit or split into parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmojiMerging {
    /// Use the default policy, which merges.
    #[default]
    Auto,
    /// Always paint a sequence as one merged unit.
    Merge,
    /// Always split a sequence into separately positioned units.
    Separate,
}

impl EmojiMerging {
    /// Reports whether this policy paints a sequence as one unit; false only for
    /// [`EmojiMerging::Separate`].
    pub const fn merges(self) -> bool {
        !matches!(self, Self::Separate)
    }
}

/// Reports whether a codepoint joins or modifies an emoji sequence: joiners,
/// variation selectors, keycaps, skin tones, regional indicators, or tags.
pub(crate) fn is_emoji_sequence_mark(ch: char) -> bool {
    matches!(
        ch,
        '\u{200D}'
        | '\u{FE0E}' | '\u{FE0F}'
        | '\u{20E3}'
        | '\u{1F3FB}'..='\u{1F3FF}'
        | '\u{1F1E6}'..='\u{1F1FF}'
        | '\u{E0020}'..='\u{E007F}'
    )
}

/// Splits an emoji grapheme into per-codepoint units as byte-range and
/// column-width pairs, or `None` when it carries no sequence mark, contains a
/// control character, begins with a zero-width codepoint, or ends at zero width.
pub(crate) fn separate_units(grapheme: &str) -> Option<Vec<(std::ops::Range<usize>, usize)>> {
    if !grapheme.chars().any(is_emoji_sequence_mark) || grapheme.chars().any(char::is_control) {
        return None;
    }
    let mut units: Vec<(std::ops::Range<usize>, usize)> = Vec::new();
    let mut start = 0usize;
    let mut width = 0usize;
    for (index, ch) in grapheme.char_indices() {
        let ch_width = UnicodeWidthStr::width(ch.to_string().as_str());
        if ch_width == 0 {
            if index == 0 {
                return None;
            }
            continue;
        }
        if width > 0 {
            units.push((start..index, width));
        }
        start = index;
        width = ch_width;
    }
    if width == 0 {
        return None;
    }
    units.push((start..grapheme.len(), width));
    Some(units)
}

/// Returns the byte range of each display unit in `value`, splitting emoji
/// sequences into parts unless `merging` requests merged units.
pub(crate) fn display_units(value: &str, merging: EmojiMerging) -> Vec<std::ops::Range<usize>> {
    let mut units = Vec::new();
    for (index, grapheme) in value.grapheme_indices(true) {
        if !merging.merges()
            && let Some(parts) = separate_units(grapheme)
        {
            for (range, _) in parts {
                units.push(index + range.start..index + range.end);
            }
            continue;
        }
        units.push(index..index + grapheme.len());
    }
    units
}

/// A signed line and column position on the terminal screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct ScreenPosition {
    /// Line index, counted downward from the top.
    pub line: i32,
    /// Column index, counted rightward from the left.
    pub column: i32,
}

impl ScreenPosition {
    /// Creates a position from a line and column.
    pub const fn new(line: i32, column: i32) -> Self {
        Self { line, column }
    }
}

/// A non-negative line and column position inside an image surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct ImagePosition {
    /// Line index, counted downward from the top.
    pub line: usize,
    /// Column index, counted rightward from the left.
    pub column: usize,
}

impl ImagePosition {
    /// Creates a position from a line and column.
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// A width and height measured in terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Size {
    /// Width in columns.
    pub width: u16,
    /// Height in lines.
    pub height: u16,
}

impl Size {
    /// Creates a size from a width and height.
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// An axis-aligned rectangle in cell coordinates, anchored at its top-left
/// corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Line of the top edge.
    pub line: usize,
    /// Column of the left edge.
    pub column: usize,
    /// Width in columns.
    pub width: usize,
    /// Height in lines.
    pub height: usize,
}

impl Rect {
    /// Creates a rectangle from its top-left corner and extent.
    pub const fn new(line: usize, column: usize, width: usize, height: usize) -> Self {
        Self {
            line,
            column,
            width,
            height,
        }
    }
    /// Returns the column just past the right edge, saturating on overflow.
    pub fn right(self) -> usize {
        self.column.saturating_add(self.width)
    }
    /// Returns the line just past the bottom edge, saturating on overflow.
    pub fn bottom(self) -> usize {
        self.line.saturating_add(self.height)
    }
    /// Returns the smallest rectangle covering both inputs.
    pub fn union(self, other: Self) -> Self {
        let left = self.column.min(other.column);
        let top = self.line.min(other.line);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self::new(
            top,
            left,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        )
    }
    /// Returns the rectangle grown by `amount` on every side, clamped at zero.
    pub fn expand(self, amount: usize) -> Self {
        let left = self.column.saturating_sub(amount);
        let top = self.line.saturating_sub(amount);
        let right = self.right().saturating_add(amount);
        let bottom = self.bottom().saturating_add(amount);
        Self::new(
            top,
            left,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        )
    }
}

/// An opaque identifier for a surface within one frame batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageId(
    /// The underlying identifier value.
    pub u64,
);

/// One terminal cell: a single grapheme with its colors, attributes, and
/// display width of one or two columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub(crate) foreground: Color,
    pub(crate) background: Color,
    pub(crate) attributes: Attributes,
    pub(crate) symbol: Arc<str>,
    width: u8,
}

/// Why a cell symbol was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellError {
    /// The symbol was empty.
    Empty,
    /// The symbol contained more than one grapheme.
    MultipleGraphemes,
    /// The symbol contained a control character.
    ControlCharacter,
    /// The symbol exceeded [`MAX_GLYPH_BYTES`]; carries its byte length.
    SymbolTooLong(usize),
    /// The symbol's display width was outside the supported range; carries the
    /// measured width.
    UnsupportedWidth(usize),
}

impl fmt::Display for CellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "cell symbol is empty"),
            Self::MultipleGraphemes => write!(f, "cell symbol must contain one grapheme"),
            Self::ControlCharacter => write!(f, "cell symbol contains a control character"),
            Self::SymbolTooLong(bytes) => {
                write!(
                    f,
                    "cell symbol is {bytes} bytes; maximum is {MAX_GLYPH_BYTES}"
                )
            }
            Self::UnsupportedWidth(width) => {
                write!(f, "cell display width {width} is not supported")
            }
        }
    }
}
impl Error for CellError {}

impl Cell {
    /// Creates a cell with reset colors and default attributes.
    pub fn plain(symbol: impl Into<String>) -> Result<Self, CellError> {
        Self::new(symbol, Color::Reset, Color::Reset, Attributes::default())
    }

    /// Creates a cell with explicit colors and attributes.
    pub fn styled(
        foreground: Color,
        background: Color,
        attributes: Attributes,
        symbol: impl Into<String>,
    ) -> Result<Self, CellError> {
        Self::new(symbol, foreground, background, attributes)
    }

    /// Validates `symbol` as one printable grapheme of width 1 or 2 and returns
    /// the styled cell; a rejected symbol yields the matching [`CellError`].
    pub fn new(
        symbol: impl Into<String>,
        foreground: Color,
        background: Color,
        attributes: Attributes,
    ) -> Result<Self, CellError> {
        let glyph = crate::glyph::validate_terminal_glyph(
            symbol.into().as_str(),
            crate::glyph::AllowedGlyphWidth::OneOrTwo,
        )
        .map_err(cell_error_from_glyph)?;
        Ok(Self {
            foreground,
            background,
            attributes,
            width: glyph.width() as u8,
            symbol: glyph.into_text(),
        })
    }
    /// Creates a cell like [`Cell::new`] but with an explicit width of 1 or 2.
    pub(crate) fn with_width(
        symbol: impl Into<String>,
        foreground: Color,
        background: Color,
        attributes: Attributes,
        width: usize,
    ) -> Result<Self, CellError> {
        let mut cell = Self::new(symbol, foreground, background, attributes)?;
        if !(1..=2).contains(&width) {
            return Err(CellError::UnsupportedWidth(width));
        }
        cell.width = width as u8;
        Ok(cell)
    }

    /// Returns the canonical blank cell: a reset-colored single space.
    pub fn blank() -> Self {
        Self {
            foreground: Color::Reset,
            background: Color::Reset,
            attributes: Attributes::default(),
            symbol: blank_symbol(),
            width: 1,
        }
    }

    /// Reports whether this is the canonical blank cell, testing the stored
    /// fields rather than constructing one.
    pub(crate) fn is_blank(&self) -> bool {
        self.foreground == Color::Reset
            && self.background == Color::Reset
            && self.attributes == Attributes::default()
            && self.width == 1
            && &*self.symbol == " "
    }
    /// Returns the display width in columns, 1 or 2.
    pub fn width(&self) -> usize {
        self.width as usize
    }
    /// Returns the grapheme text.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }
    /// Returns the foreground color.
    pub fn foreground(&self) -> Color {
        self.foreground
    }
    /// Returns the background color.
    pub fn background(&self) -> Color {
        self.background
    }
    /// Returns the text attributes.
    pub fn attributes(&self) -> Attributes {
        self.attributes
    }
    /// Returns a copy with the same colors and attributes but a single blank
    /// space.
    pub fn as_blank(&self) -> Self {
        Self {
            foreground: self.foreground,
            background: self.background,
            attributes: self.attributes,
            symbol: blank_symbol(),
            width: 1,
        }
    }
}

fn blank_symbol() -> Arc<str> {
    static BLANK: OnceLock<Arc<str>> = OnceLock::new();
    BLANK.get_or_init(|| Arc::from(" ")).clone()
}

/// One stored slot of an image row: a glyph's leading cell or its continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellSlot {
    /// The slot holding the glyph itself.
    Lead(Cell),
    /// The blank slot holding the second column of a width-2 glyph.
    Continuation(Cell),
}

impl CellSlot {
    /// Reports whether this is a blank lead cell.
    pub(crate) fn is_default(&self) -> bool {
        matches!(self, Self::Lead(cell) if cell.is_blank())
    }

    /// Returns the stored cell for either slot kind.
    pub fn cell(&self) -> &Cell {
        match self {
            Self::Lead(cell) | Self::Continuation(cell) => cell,
        }
    }
}

/// A rectangular grid of cell slots with a shared, copy-on-write backing store.
///
/// A width-2 glyph occupies a lead slot followed by a continuation slot. Clones
/// share the store until a patch detaches it.
#[derive(Debug, Clone)]
pub struct Image {
    width: usize,
    height: usize,
    cells: Arc<Vec<CellSlot>>,
}

impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && (Arc::ptr_eq(&self.cells, &other.cells) || self.cells == other.cells)
    }
}

impl Eq for Image {}

/// Why an image construction, patch, or crop failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// A requested dimension was zero.
    Empty,
    /// The supplied rows did not match the target height.
    WrongCellCount,
    /// Rows of one image had different total widths.
    UnequalRowWidths,
    /// A rectangle or edit fell outside the surface.
    OutOfBounds,
    /// A row's total width differed from the rectangle width.
    RowWidthMismatch,
    /// Summing a row's cell widths overflowed `usize`.
    RowWidthOverflow,
    /// A width-2 glyph would not fit in the surface or rectangle.
    WideCellAtEdge,
    /// The surface exceeds [`MAX_SURFACE_CELLS`].
    SurfaceTooLarge {
        /// Requested width in columns.
        width: usize,
        /// Requested height in lines.
        height: usize,
        /// Requested total cell count.
        cells: usize,
    },
    /// The cell buffer could not be allocated.
    AllocationFailed {
        /// Requested total cell count.
        cells: usize,
    },
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SurfaceTooLarge {
                width,
                height,
                cells,
            } => write!(
                f,
                "image surface {width}x{height} requests {cells} cells; maximum is {MAX_SURFACE_CELLS}"
            ),
            Self::AllocationFailed { cells } => {
                write!(f, "image allocation failed for {cells} cells")
            }
            Self::RowWidthOverflow => write!(f, "image row width overflowed while validating"),
            other => write!(f, "image error: {other:?}"),
        }
    }
}
impl Error for ImageError {}

impl Image {
    /// Creates a `width` by `height` surface filled with `fill`, which must be
    /// one column wide.
    pub fn new(width: usize, height: usize, fill: Cell) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty);
        }
        let count = width
            .checked_mul(height)
            .ok_or(ImageError::SurfaceTooLarge {
                width,
                height,
                cells: usize::MAX,
            })?;
        if count > MAX_SURFACE_CELLS {
            return Err(ImageError::SurfaceTooLarge {
                width,
                height,
                cells: count,
            });
        }
        if fill.width() != 1 {
            return Err(ImageError::WideCellAtEdge);
        }
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(count)
            .map_err(|_| ImageError::AllocationFailed { cells: count })?;
        cells.resize(count, CellSlot::Lead(fill));
        Ok(Self {
            width,
            height,
            cells: Arc::new(cells),
        })
    }

    /// Builds an image from owned rows, requiring equal total row widths; each
    /// width-2 cell gains a derived blank continuation slot.
    pub fn from_rows(rows: Vec<Vec<Cell>>) -> Result<Self, ImageError> {
        if rows.is_empty() || rows[0].is_empty() {
            return Err(ImageError::Empty);
        }
        let height = rows.len();
        let width = rows[0]
            .iter()
            .try_fold(0usize, |sum, cell| sum.checked_add(cell.width()))
            .ok_or(ImageError::SurfaceTooLarge {
                width: usize::MAX,
                height,
                cells: usize::MAX,
            })?;
        if width == 0 {
            return Err(ImageError::Empty);
        }
        let count = width
            .checked_mul(height)
            .ok_or(ImageError::SurfaceTooLarge {
                width,
                height,
                cells: usize::MAX,
            })?;
        if count > MAX_SURFACE_CELLS {
            return Err(ImageError::SurfaceTooLarge {
                width,
                height,
                cells: count,
            });
        }
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(count)
            .map_err(|_| ImageError::AllocationFailed { cells: count })?;
        for row in rows {
            let row_width = row
                .iter()
                .try_fold(0usize, |sum, cell| sum.checked_add(cell.width()))
                .ok_or(ImageError::SurfaceTooLarge {
                    width,
                    height,
                    cells: usize::MAX,
                })?;
            if row_width != width {
                return Err(ImageError::UnequalRowWidths);
            }
            for cell in row {
                let continuation = (cell.width() == 2).then(|| cell.as_blank());
                cells.push(CellSlot::Lead(cell));
                if let Some(blank) = continuation {
                    cells.push(CellSlot::Continuation(blank));
                }
            }
        }
        Ok(Self {
            width,
            height,
            cells: Arc::new(cells),
        })
    }

    /// Returns the surface width in columns.
    pub fn width(&self) -> usize {
        self.width
    }
    /// Returns the surface height in lines.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Checks that `rect` fits and that `rows` match its height and total width.
    pub(crate) fn validate_rect(&self, rect: Rect, rows: &[Vec<Cell>]) -> Result<(), ImageError> {
        self.validate_bounds(rect)?;
        if rows.len() != rect.height {
            return Err(ImageError::WrongCellCount);
        }
        for row in rows {
            let Some(row_width) = row
                .iter()
                .try_fold(0usize, |sum, cell| sum.checked_add(cell.width()))
            else {
                return Err(ImageError::RowWidthOverflow);
            };
            if row_width != rect.width {
                return Err(ImageError::RowWidthMismatch);
            }
        }
        Ok(())
    }

    /// Checks that every edit is in bounds and that no width-2 glyph crosses the
    /// right edge.
    pub(crate) fn validate_edits(&self, edits: &[CellEdit]) -> Result<(), ImageError> {
        for edit in edits {
            if edit.position.column >= self.width || edit.position.line >= self.height {
                return Err(ImageError::OutOfBounds);
            }
            if edit.cell.width() == 2 && edit.position.column.saturating_add(1) >= self.width {
                return Err(ImageError::WideCellAtEdge);
            }
        }
        Ok(())
    }

    fn glyph_span(cells: &[CellSlot], index: usize) -> (usize, usize) {
        let mut start = index;
        while start > 0 && matches!(cells[start], CellSlot::Continuation(_)) {
            start -= 1;
        }
        let end = if matches!(cells[start], CellSlot::Lead(_))
            && start + 1 < cells.len()
            && matches!(cells[start + 1], CellSlot::Continuation(_))
        {
            start + 2
        } else {
            start + 1
        };
        (start, end)
    }

    fn clear_span(cells: &mut [CellSlot], start: usize, end: usize) {
        cells[start..end].fill(CellSlot::Lead(Cell::blank()));
    }

    fn validate_bounds(&self, rect: Rect) -> Result<(), ImageError> {
        (rect.width > 0
            && rect.height > 0
            && rect.right() <= self.width
            && rect.bottom() <= self.height)
            .then_some(())
            .ok_or(ImageError::OutOfBounds)
    }

    /// Replaces the cells inside `rect` with `rows` and returns the affected
    /// area, widened to cover every wide glyph the patch touches.
    pub fn patch_rect(&mut self, rect: Rect, rows: &[Vec<Cell>]) -> Result<Rect, ImageError> {
        self.validate_rect(rect, rows)?;
        let mut affected_left = rect.column;
        let mut affected_right = rect.right();
        let cells = Arc::make_mut(&mut self.cells);
        for line in rect.line..rect.bottom() {
            let mut left = rect.column;
            let mut right = rect.right();
            for column in rect.column..rect.right() {
                let (start, end) = Self::glyph_span(cells, line * self.width + column);
                let start_column = start % self.width;
                let end_column = if end % self.width == 0 {
                    self.width
                } else {
                    end % self.width
                };
                left = left.min(start_column);
                right = right.max(end_column);
            }
            let start = line * self.width + left;
            let end = line * self.width + right.min(self.width);
            Self::clear_span(cells, start, end);
            affected_left = affected_left.min(left);
            affected_right = affected_right.max(right);
        }
        for (row_offset, row) in rows.iter().enumerate() {
            let mut column = rect.column;
            for cell in row {
                let index = (rect.line + row_offset) * self.width + column;
                cells[index] = CellSlot::Lead(cell.clone());
                if cell.width() == 2 {
                    cells[index + 1] = CellSlot::Continuation(cell.as_blank());
                }
                column += cell.width();
            }
        }
        Ok(Rect::new(
            rect.line,
            affected_left,
            affected_right.saturating_sub(affected_left),
            rect.height,
        ))
    }

    /// Applies single-cell edits and returns their combined affected area.
    pub(crate) fn patch_cells(&mut self, edits: &[CellEdit]) -> Result<Rect, ImageError> {
        self.validate_edits(edits)?;
        let mut affected: Option<Rect> = None;
        let width = self.width;
        let cells = Arc::make_mut(&mut self.cells);
        let cells_len = cells.len();
        for edit in edits {
            let index = edit.position.line * width + edit.position.column;
            let (old_start, old_end) = Self::glyph_span(cells, index);
            let new_end = index + edit.cell.width();
            Self::clear_span(cells, old_start, old_end.max(new_end).min(cells_len));
            cells[index] = CellSlot::Lead(edit.cell.clone());
            if edit.cell.width() == 2 {
                cells[index + 1] = CellSlot::Continuation(edit.cell.as_blank());
            }
            let start_col = old_start % self.width;
            let end_absolute = old_end.max(new_end) - 1;
            let end_col = end_absolute % self.width;
            let rect = Rect::new(edit.position.line, start_col, end_col - start_col + 1, 1);
            affected = Some(affected.map_or(rect, |value| value.union(rect)));
        }
        Ok(affected.unwrap_or(Rect::new(0, 0, 0, 0)))
    }

    /// Returns the slot at `line` and `column` without bounds checking.
    pub fn cell_at(&self, line: usize, column: usize) -> &CellSlot {
        &self.cells[line * self.width + column]
    }

    /// Returns the sub-image at `rect`, replacing any partial wide glyph with a
    /// blank.
    pub(crate) fn crop(&self, rect: Rect) -> Result<Self, ImageError> {
        self.validate_bounds(rect)?;
        let mut rows = Vec::with_capacity(rect.height);
        for line in rect.line..rect.bottom() {
            let mut row = Vec::with_capacity(rect.width);
            let mut column = rect.column;
            while column < rect.right() {
                match self.cell_at(line, column) {
                    CellSlot::Lead(cell) if cell.width() == 2 => {
                        if column + 1 < rect.right() {
                            row.push(cell.clone());
                            column += 2;
                        } else {
                            row.push(cell.as_blank());
                            column += 1;
                        }
                    }
                    CellSlot::Lead(cell) => {
                        row.push(cell.clone());
                        column += 1;
                    }
                    CellSlot::Continuation(cell) => {
                        row.push(cell.as_blank());
                        column += 1;
                    }
                }
            }
            rows.push(row);
        }
        Self::from_rows(rows)
    }

    /// Returns the smallest patch turning `self` into `next`, or `None` when the
    /// sizes differ, the content matches, or a full repaint is no larger.
    pub fn diff_patch_rect(&self, next: &Self) -> Option<(Rect, Vec<Vec<Cell>>)> {
        if self.width != next.width || self.height != next.height || self == next {
            return None;
        }
        let mut top = self.height;
        let mut bottom = 0usize;
        let mut left = self.width;
        let mut right = 0usize;
        for line in 0..self.height {
            for column in 0..self.width {
                if self.cell_at(line, column) != next.cell_at(line, column) {
                    top = top.min(line);
                    bottom = bottom.max(line + 1);
                    left = left.min(column);
                    right = right.max(column + 1);
                }
            }
        }
        if top >= bottom || left >= right {
            return None;
        }

        loop {
            let before = (left, right);
            for line in top..bottom {
                for image in [self, next] {
                    while left > 0 && matches!(image.cell_at(line, left), CellSlot::Continuation(_))
                    {
                        left -= 1;
                    }
                    while right < self.width
                        && matches!(image.cell_at(line, right - 1), CellSlot::Lead(cell) if cell.width() == 2)
                    {
                        right += 1;
                    }
                }
            }
            if before == (left, right) {
                break;
            }
        }
        let rect = Rect::new(top, left, right - left, bottom - top);
        if rect.width.saturating_mul(rect.height) >= self.width.saturating_mul(self.height) {
            return None;
        }
        let mut rows = Vec::with_capacity(rect.height);
        for line in rect.line..rect.bottom() {
            let mut row = Vec::with_capacity(rect.width);
            let mut column = rect.column;
            while column < rect.right() {
                match next.cell_at(line, column) {
                    CellSlot::Lead(cell) if cell.width() == 2 && column + 1 < rect.right() => {
                        row.push(cell.clone());
                        column += 2;
                    }
                    CellSlot::Lead(cell) => {
                        row.push(cell.clone());
                        column += 1;
                    }
                    CellSlot::Continuation(cell) => {
                        row.push(cell.as_blank());
                        column += 1;
                    }
                }
            }
            rows.push(row);
        }
        Some((rect, rows))
    }
}

/// A single cell replacement at a position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellEdit {
    /// Target position within the image surface.
    pub position: ImagePosition,
    /// Replacement cell.
    pub cell: Cell,
}

/// A batch of surface operations plus optional viewport and redraw hints.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Operations to apply in order.
    pub operations: Vec<Operation>,
    /// Terminal size to resize to, when the frame requests one.
    pub viewport: Option<Size>,
    /// Whether the renderer must repaint every cell.
    pub force_redraw: bool,
}

impl Frame {
    /// Creates a frame from operations with no viewport change and no forced
    /// redraw.
    pub fn new(operations: Vec<Operation>) -> Self {
        Self {
            operations,
            viewport: None,
            force_redraw: false,
        }
    }

    /// Creates a frame with no operations.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Creates a frame holding a single operation.
    pub fn with_operation(operation: Operation) -> Self {
        Self::new(vec![operation])
    }

    /// Sets the terminal size this frame resizes to.
    pub fn resize(mut self, viewport: Size) -> Self {
        self.viewport = Some(viewport);
        self
    }

    /// Requests a full repaint for this frame.
    pub fn invalidate(mut self) -> Self {
        self.force_redraw = true;
        self
    }
}

/// One mutation of a named surface within a frame.
#[derive(Debug, Clone)]
pub enum Operation {
    /// Creates a cell surface.
    Create {
        /// Identifier the surface is created under.
        id: ImageId,
        /// Initial image content.
        image: Image,
        /// Screen position of the surface's top-left corner.
        position: ScreenPosition,
        /// Stacking level.
        level: i32,
    },
    /// Creates a raster surface.
    CreateRaster {
        /// Identifier the surface is created under.
        id: ImageId,
        /// Raster source and render options.
        raster: RasterPlacement,
        /// Screen position of the surface's top-left corner.
        position: ScreenPosition,
        /// Stacking level.
        level: i32,
    },
    /// Removes a surface.
    Remove {
        /// Identifier of the surface to remove.
        id: ImageId,
    },
    /// Moves a surface to a new position.
    Move {
        /// Identifier of the surface to move.
        id: ImageId,
        /// New screen position of the surface's top-left corner.
        position: ScreenPosition,
    },
    /// Sets a surface's stacking level.
    SetLevel {
        /// Identifier of the target surface.
        id: ImageId,
        /// New stacking level.
        level: i32,
    },
    /// Sets a surface's order within its level.
    SetOrder {
        /// Identifier of the target surface.
        id: ImageId,
        /// New order value.
        order: u64,
    },
    /// Replaces a cell surface's image.
    Replace {
        /// Identifier of the target surface.
        id: ImageId,
        /// Replacement image content.
        image: Image,
    },
    /// Replaces a raster surface's placement.
    ReplaceRaster {
        /// Identifier of the target surface.
        id: ImageId,
        /// Replacement raster source and render options.
        raster: RasterPlacement,
    },
    /// Sets or clears a raster surface's clip rectangle.
    SetRasterClip {
        /// Identifier of the target surface.
        id: ImageId,
        /// Clip rectangle, or `None` to clear clipping.
        clip: Option<Rect>,
    },
    /// Replaces a rectangular region of a cell surface.
    PatchRect {
        /// Identifier of the target surface.
        id: ImageId,
        /// Region to replace.
        rect: Rect,
        /// Replacement rows, whose widths must match `rect`.
        rows: Vec<Vec<Cell>>,
    },
    /// Applies single-cell edits to a cell surface.
    PatchCells {
        /// Identifier of the target surface.
        id: ImageId,
        /// Edits to apply in order.
        edits: Vec<CellEdit>,
    },
}
