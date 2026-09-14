use crossterm::style::{Attributes, Color};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, OnceLock};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::RasterPlacement;

// Translation from the shared glyph error to the cell-specific public error.
// The variants are intentionally one-to-one; only the domain wrapper differs.
pub(crate) fn cell_error_from_glyph(error: crate::glyph::GlyphError) -> CellError {
    match error {
        crate::glyph::GlyphError::Empty => CellError::Empty,
        crate::glyph::GlyphError::SymbolTooLong(bytes) => CellError::SymbolTooLong(bytes),
        crate::glyph::GlyphError::MultipleGraphemes => CellError::MultipleGraphemes,
        crate::glyph::GlyphError::ControlCharacter => CellError::ControlCharacter,
        crate::glyph::GlyphError::UnsupportedWidth(width) => CellError::UnsupportedWidth(width),
    }
}

pub const MAX_SURFACE_CELLS: usize = 1_048_576;

pub const MAX_GLYPH_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmojiMerging {
    #[default]
    Auto,
    Merge,
    Separate,
}

impl EmojiMerging {
    pub const fn merges(self) -> bool {
        !matches!(self, Self::Separate)
    }
}

pub(crate) fn is_emoji_sequence_mark(ch: char) -> bool {
    matches!(
        ch,
        '\u{200D}'                  // zero-width joiner
        | '\u{FE0E}' | '\u{FE0F}'   // variation selectors
        | '\u{20E3}'                // combining enclosing keycap
        | '\u{1F3FB}'..='\u{1F3FF}' // skin-tone modifiers
        | '\u{1F1E6}'..='\u{1F1FF}' // regional indicators
        | '\u{E0020}'..='\u{E007F}' // tag characters
    )
}

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
                // A leading zero-width codepoint has no unit to attach to.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct ScreenPosition {
    pub line: i32,
    pub column: i32,
}

impl ScreenPosition {
    pub const fn new(line: i32, column: i32) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct ImagePosition {
    pub line: usize,
    pub column: usize,
}

impl ImagePosition {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

impl Size {
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub line: usize,
    pub column: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    pub const fn new(line: usize, column: usize, width: usize, height: usize) -> Self {
        Self {
            line,
            column,
            width,
            height,
        }
    }
    pub fn right(self) -> usize {
        self.column.saturating_add(self.width)
    }
    pub fn bottom(self) -> usize {
        self.line.saturating_add(self.height)
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub(crate) foreground: Color,
    pub(crate) background: Color,
    pub(crate) attributes: Attributes,
    pub(crate) symbol: Arc<str>,
    width: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellError {
    Empty,
    MultipleGraphemes,
    ControlCharacter,
    SymbolTooLong(usize),
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
    pub fn plain(symbol: impl Into<String>) -> Result<Self, CellError> {
        Self::new(symbol, Color::Reset, Color::Reset, Attributes::default())
    }

    pub fn styled(
        foreground: Color,
        background: Color,
        attributes: Attributes,
        symbol: impl Into<String>,
    ) -> Result<Self, CellError> {
        Self::new(symbol, foreground, background, attributes)
    }

    pub fn new(
        symbol: impl Into<String>,
        foreground: Color,
        background: Color,
        attributes: Attributes,
    ) -> Result<Self, CellError> {
        // One shared validator owns the terminal-glyph invariant; `Cell`
        // translates its error into the cell-specific public error.
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

    pub fn blank() -> Self {
        Self {
            foreground: Color::Reset,
            background: Color::Reset,
            attributes: Attributes::default(),
            symbol: blank_symbol(),
            width: 1,
        }
    }

    // Canonical blank test without constructing a cell.
    pub(crate) fn is_blank(&self) -> bool {
        self.foreground == Color::Reset
            && self.background == Color::Reset
            && self.attributes == Attributes::default()
            && self.width == 1
            && &*self.symbol == " "
    }
    pub fn width(&self) -> usize {
        self.width as usize
    }
    pub fn symbol(&self) -> &str {
        &self.symbol
    }
    pub fn foreground(&self) -> Color {
        self.foreground
    }
    pub fn background(&self) -> Color {
        self.background
    }
    pub fn attributes(&self) -> Attributes {
        self.attributes
    }
    pub(crate) fn as_blank(&self) -> Self {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellSlot {
    Lead(Cell),
    Continuation(Cell),
}

impl CellSlot {
    pub(crate) fn is_default(&self) -> bool {
        // Field test rather than constructing a blank cell and comparing: the
        // previous form built (and symbol-cloned) a `Cell` on every call.
        matches!(self, Self::Lead(cell) if cell.is_blank())
    }

    pub fn cell(&self) -> &Cell {
        match self {
            Self::Lead(cell) | Self::Continuation(cell) => cell,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Image {
    width: usize,
    height: usize,
    // Most images travel through several retained stages unchanged. Sharing
    // keeps those clones constant-time; patch operations detach only when a
    // frame still retains an older view of the surface.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    Empty,
    WrongCellCount,
    UnequalRowWidths,
    OutOfBounds,
    RowWidthMismatch,
    RowWidthOverflow,
    WideCellAtEdge,
    SurfaceTooLarge {
        width: usize,
        height: usize,
        cells: usize,
    },
    AllocationFailed {
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
                // The row is owned, so each cell is moved into the surface
                // instead of cloned. The wide-cell continuation is derived
                // before the move so no clone is needed.
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

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }

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

    pub fn patch_rect(&mut self, rect: Rect, rows: &[Vec<Cell>]) -> Result<Rect, ImageError> {
        self.validate_rect(rect, rows)?;
        let mut affected_left = rect.column;
        let mut affected_right = rect.right();
        let cells = Arc::make_mut(&mut self.cells);
        for line in rect.line..rect.bottom() {
            // Wide glyphs may extend by a different amount on every row;
            // never reuse one row's clear span for another row.
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

    pub fn cell_at(&self, line: usize, column: usize) -> &CellSlot {
        &self.cells[line * self.width + column]
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellEdit {
    pub position: ImagePosition,
    pub cell: Cell,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub operations: Vec<Operation>,
    pub viewport: Option<Size>,
    pub force_redraw: bool,
}

impl Frame {
    pub fn new(operations: Vec<Operation>) -> Self {
        Self {
            operations,
            viewport: None,
            force_redraw: false,
        }
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn with_operation(operation: Operation) -> Self {
        Self::new(vec![operation])
    }

    pub fn resize(mut self, viewport: Size) -> Self {
        self.viewport = Some(viewport);
        self
    }

    pub fn invalidate(mut self) -> Self {
        self.force_redraw = true;
        self
    }
}

#[derive(Debug, Clone)]
pub enum Operation {
    Create {
        id: ImageId,
        image: Image,
        position: ScreenPosition,
        level: i32,
    },
    CreateRaster {
        id: ImageId,
        raster: RasterPlacement,
        position: ScreenPosition,
        level: i32,
    },
    Remove {
        id: ImageId,
    },
    Move {
        id: ImageId,
        position: ScreenPosition,
    },
    SetLevel {
        id: ImageId,
        level: i32,
    },
    SetOrder {
        id: ImageId,
        order: u64,
    },
    Replace {
        id: ImageId,
        image: Image,
    },
    ReplaceRaster {
        id: ImageId,
        raster: RasterPlacement,
    },
    SetRasterClip {
        id: ImageId,
        clip: Option<Rect>,
    },
    PatchRect {
        id: ImageId,
        rect: Rect,
        rows: Vec<Vec<Cell>>,
    },
    PatchCells {
        id: ImageId,
        edits: Vec<CellEdit>,
    },
}
