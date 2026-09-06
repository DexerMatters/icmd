use crossterm::style::{Attributes, Color};
use std::error::Error;
use std::fmt;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A position in terminal coordinates. Positions may be negative so that an
/// image can be moved partially outside the viewport.
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

/// A position in an image, measured in terminal columns and rows.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
    pub(crate) symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellError {
    Empty,
    MultipleGraphemes,
    ControlCharacter,
    UnsupportedWidth(usize),
}

impl fmt::Display for CellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "cell symbol is empty"),
            Self::MultipleGraphemes => write!(f, "cell symbol must contain one grapheme"),
            Self::ControlCharacter => write!(f, "cell symbol contains a control character"),
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
        let symbol = symbol.into();
        if symbol.is_empty() {
            return Err(CellError::Empty);
        }
        if symbol.graphemes(true).count() != 1 {
            return Err(CellError::MultipleGraphemes);
        }
        if symbol.chars().any(char::is_control) {
            return Err(CellError::ControlCharacter);
        }
        let width = UnicodeWidthStr::width(symbol.as_str());
        if !(1..=2).contains(&width) {
            return Err(CellError::UnsupportedWidth(width));
        }
        Ok(Self {
            foreground,
            background,
            attributes,
            symbol,
        })
    }
    pub fn blank() -> Self {
        Self {
            foreground: Color::Reset,
            background: Color::Reset,
            attributes: Attributes::default(),
            symbol: " ".into(),
        }
    }
    pub fn width(&self) -> usize {
        UnicodeWidthStr::width(self.symbol.as_str())
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
            symbol: " ".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CellSlot {
    Lead(Cell),
    Continuation(Cell),
}

impl CellSlot {
    pub(crate) fn is_default(&self) -> bool {
        matches!(self, Self::Lead(cell) if *cell == Cell::blank())
    }

    pub(crate) fn cell(&self) -> &Cell {
        match self {
            Self::Lead(cell) | Self::Continuation(cell) => cell,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    width: usize,
    height: usize,
    cells: Vec<CellSlot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    Empty,
    WrongCellCount,
    UnequalRowWidths,
    OutOfBounds,
    RowWidthMismatch,
    WideCellAtEdge,
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "image error: {:?}", self)
    }
}
impl Error for ImageError {}

impl Image {
    pub fn new(width: usize, height: usize, fill: Cell) -> Result<Self, ImageError> {
        Self::blank(width, height, fill)
    }

    pub fn blank(width: usize, height: usize, fill: Cell) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty);
        }
        if fill.width() != 1 {
            return Err(ImageError::WideCellAtEdge);
        }
        Ok(Self {
            width,
            height,
            cells: vec![CellSlot::Lead(fill); width * height],
        })
    }

    pub fn from_rows(rows: Vec<Vec<Cell>>) -> Result<Self, ImageError> {
        if rows.is_empty() || rows[0].is_empty() {
            return Err(ImageError::Empty);
        }
        let height = rows.len();
        let width = rows[0].iter().map(Cell::width).sum::<usize>();
        if width == 0 {
            return Err(ImageError::Empty);
        }
        let mut cells = Vec::with_capacity(width * height);
        for row in rows {
            if row.iter().map(Cell::width).sum::<usize>() != width {
                return Err(ImageError::UnequalRowWidths);
            }
            for cell in row {
                let cell_width = cell.width();
                cells.push(CellSlot::Lead(cell.clone()));
                if cell_width == 2 {
                    cells.push(CellSlot::Continuation(cell.as_blank()));
                }
            }
        }
        Ok(Self {
            width,
            height,
            cells,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }

    pub(crate) fn validate_rect(&self, rect: Rect, rows: &[Vec<Cell>]) -> Result<(), ImageError> {
        if rect.width == 0
            || rect.height == 0
            || rect.right() > self.width
            || rect.bottom() > self.height
        {
            return Err(ImageError::OutOfBounds);
        }
        if rows.len() != rect.height {
            return Err(ImageError::WrongCellCount);
        }
        for row in rows {
            if row.iter().map(Cell::width).sum::<usize>() != rect.width {
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
            if edit.cell.width() == 2 && edit.position.column + 1 >= self.width {
                return Err(ImageError::WideCellAtEdge);
            }
        }
        Ok(())
    }

    fn index(&self, position: ImagePosition) -> usize {
        position.line * self.width + position.column
    }

    fn glyph_span(&self, index: usize) -> (usize, usize) {
        let mut start = index;
        while start > 0 && matches!(self.cells[start], CellSlot::Continuation(_)) {
            start -= 1;
        }
        let end = if matches!(self.cells[start], CellSlot::Lead(_))
            && start + 1 < self.cells.len()
            && matches!(self.cells[start + 1], CellSlot::Continuation(_))
        {
            start + 2
        } else {
            start + 1
        };
        (start, end)
    }

    fn clear_span(&mut self, start: usize, end: usize) {
        for cell in &mut self.cells[start..end] {
            *cell = CellSlot::Lead(Cell::blank());
        }
    }

    pub(crate) fn patch_rect(
        &mut self,
        rect: Rect,
        rows: &[Vec<Cell>],
    ) -> Result<Rect, ImageError> {
        self.validate_rect(rect, rows)?;
        let mut affected_left = rect.column;
        let mut affected_right = rect.right();
        for line in rect.line..rect.bottom() {
            let mut left = rect.column;
            let mut right = rect.right();
            for column in rect.column..rect.right() {
                let (start, end) = self.glyph_span(self.index(ImagePosition::new(line, column)));
                let start_column = start % self.width;
                let end_column = if end % self.width == 0 {
                    self.width
                } else {
                    end % self.width
                };
                left = left.min(start_column);
                right = right.max(end_column);
            }
            let start = self.index(ImagePosition::new(line, left));
            let end = self.index(ImagePosition::new(line, right.min(self.width)));
            self.clear_span(start, end);
            affected_left = affected_left.min(left);
            affected_right = affected_right.max(right);
        }
        for (row_offset, row) in rows.iter().enumerate() {
            let mut column = rect.column;
            for cell in row {
                let index = self.index(ImagePosition::new(rect.line + row_offset, column));
                self.cells[index] = CellSlot::Lead(cell.clone());
                if cell.width() == 2 {
                    self.cells[index + 1] = CellSlot::Continuation(cell.as_blank());
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
        for edit in edits {
            let index = self.index(edit.position);
            let (old_start, old_end) = self.glyph_span(index);
            let new_end = index + edit.cell.width();
            self.clear_span(old_start, old_end.max(new_end).min(self.cells.len()));
            self.cells[index] = CellSlot::Lead(edit.cell.clone());
            if edit.cell.width() == 2 {
                self.cells[index + 1] = CellSlot::Continuation(edit.cell.as_blank());
            }
            let start_col = old_start % self.width;
            let end_absolute = old_end.max(new_end) - 1;
            let end_col = end_absolute % self.width;
            let rect = Rect::new(edit.position.line, start_col, end_col - start_col + 1, 1);
            affected = Some(affected.map_or(rect, |value| value.union(rect)));
        }
        Ok(affected.unwrap_or(Rect::new(0, 0, 0, 0)))
    }

    pub(crate) fn cell_at(&self, line: usize, column: usize) -> &CellSlot {
        &self.cells[line * self.width + column]
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
    /// Changes the tie-break order between images at the same level.
    SetOrder {
        id: ImageId,
        order: u64,
    },
    Replace {
        id: ImageId,
        image: Image,
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
