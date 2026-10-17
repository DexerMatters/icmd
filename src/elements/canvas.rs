//! Cell canvas widget: a mutable cell buffer plus the drawing callback that
//! fills it.

use std::{error::Error, fmt, sync::Arc};

use crossterm::style::{Attributes, Color};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Attr, Cell, CellEdit, CellError, Image, ImageError, ImagePosition, ImageRenderOptions, Node,
    Props, RasterImage, RasterImageError, Text,
    basic::{ComponentContext, view},
    theme::Theme,
    ui,
};

/// Failure raised while drawing on a canvas.
#[derive(Debug)]
pub enum CanvasError {
    /// A cell symbol was rejected.
    Cell(CellError),
    /// An image operation failed.
    Image(ImageError),
    /// Raster encoding or placement failed.
    Raster(RasterImageError),
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cell(error) => write!(f, "invalid canvas cell: {error}"),
            Self::Image(error) => write!(f, "canvas image error: {error}"),
            Self::Raster(error) => write!(f, "canvas raster error: {error}"),
        }
    }
}

impl Error for CanvasError {}

impl From<CellError> for CanvasError {
    fn from(value: CellError) -> Self {
        Self::Cell(value)
    }
}

impl From<ImageError> for CanvasError {
    fn from(value: ImageError) -> Self {
        Self::Image(value)
    }
}

impl From<RasterImageError> for CanvasError {
    fn from(value: RasterImageError) -> Self {
        Self::Raster(value)
    }
}

/// Mutable cell buffer passed to a canvas drawing callback.
///
/// The `set_*` methods choose the style used by subsequent drawing calls, and
/// the `*_color`/`cell_attributes` accessors read it back.
pub struct CanvasContext {
    image: Image,
    foreground: Color,
    background: Color,
    attributes: Attributes,
    /// Braille dot masks, one byte per cell.
    dots: Vec<u8>,
}

impl CanvasContext {
    /// Public construction path for callers that draw without the component
    /// (tests, custom renderers). The default theme supplies the base colors.
    pub fn new(width: u16, height: u16) -> Result<Self, CanvasError> {
        Self::try_new(width, height, &Theme::default())
    }

    fn try_new(width: u16, height: u16, theme: &Theme) -> Result<Self, CanvasError> {
        let blank = Cell::styled(
            theme.colors.foreground,
            theme.colors.background,
            Attributes::default(),
            " ",
        )
        .expect("a space is a valid canvas cell");
        Ok(Self {
            image: Image::new(usize::from(width), usize::from(height), blank)?,
            foreground: theme.colors.foreground,
            background: theme.colors.background,
            attributes: Attributes::default(),
            dots: vec![0; usize::from(width) * usize::from(height)],
        })
    }

    /// Canvas width in cells.
    pub fn width(&self) -> usize {
        self.image.width()
    }

    /// Canvas height in cells.
    pub fn height(&self) -> usize {
        self.image.height()
    }

    /// Set the foreground color used by subsequent writes.
    pub fn set_foreground(&mut self, color: Color) {
        self.foreground = color;
    }

    /// Set the background color used by subsequent writes.
    pub fn set_background(&mut self, color: Color) {
        self.background = color;
    }

    /// Set the text attributes used by subsequent writes.
    pub fn set_attributes(&mut self, attributes: Attributes) {
        self.attributes = attributes;
    }

    /// Current foreground color.
    pub fn foreground_color(&self) -> Color {
        self.foreground
    }

    /// Current background color.
    pub fn background_color(&self) -> Color {
        self.background
    }

    /// Current text attributes.
    pub fn cell_attributes(&self) -> Attributes {
        self.attributes
    }

    /// Write one `symbol` cell at cell coordinates `(x, y)`; writes outside the
    /// canvas, or wide cells that would cross the right edge, are ignored.
    pub fn set(&mut self, x: i32, y: i32, symbol: impl Into<String>) -> Result<(), CanvasError> {
        let cell = Cell::new(symbol, self.foreground, self.background, self.attributes)?;
        self.write_validated(x, y, cell)
    }

    /// Write an already-validated cell.
    ///
    /// Bounds and wide-cell clipping are checked here, so batched primitives
    /// pay validation once per operation instead of once per cell.
    fn write_validated(&mut self, x: i32, y: i32, cell: Cell) -> Result<(), CanvasError> {
        if x < 0 || y < 0 || x as usize >= self.width() || y as usize >= self.height() {
            return Ok(());
        }
        if cell.width() == 2 && x as usize + 1 >= self.width() {
            return Ok(());
        }
        self.clear_dots(x, y, cell.width() as i32);
        self.image.patch_cells(&[CellEdit {
            position: ImagePosition::new(y as usize, x as usize),
            cell,
        }])?;
        Ok(())
    }

    /// Write the same `cell` across `[left, right)` of `row` in one batched
    /// patch.
    fn write_run(
        &mut self,
        row: i32,
        left: i32,
        right: i32,
        cell: &Cell,
    ) -> Result<(), CanvasError> {
        if row < 0 || row as usize >= self.height() {
            return Ok(());
        }
        let width = self.width() as i32;
        let left = left.max(0);
        let right = right.min(width);
        if left >= right {
            return Ok(());
        }
        let cell_width = cell.width().max(1) as i32;
        let mut edits = Vec::with_capacity(((right - left) / cell_width).max(1) as usize);
        let mut column = left;
        while column + cell_width <= right {
            if column as usize >= self.width() {
                break;
            }
            edits.push(CellEdit {
                position: ImagePosition::new(row as usize, column as usize),
                cell: cell.clone(),
            });
            column += cell_width;
        }
        if !edits.is_empty() {
            self.image.patch_cells(&edits)?;
        }
        Ok(())
    }

    /// The painted cell at `(x, y)`, or `None` outside the canvas.
    ///
    /// A wide glyph reports its lead cell; its continuation has no symbol of
    /// its own.
    pub fn cell(&self, x: i32, y: i32) -> Option<&Cell> {
        if x < 0 || y < 0 || x as usize >= self.width() || y as usize >= self.height() {
            return None;
        }
        match self.image.cell_at(y as usize, x as usize) {
            crate::data::CellSlot::Lead(cell) => Some(cell),
            crate::data::CellSlot::Continuation(_) => None,
        }
    }

    /// Canvas width in braille dots: two dots per cell column.
    pub fn dot_width(&self) -> usize {
        self.width().saturating_mul(2)
    }

    /// Canvas height in braille dots: four dots per cell row.
    pub fn dot_height(&self) -> usize {
        self.height().saturating_mul(4)
    }

    /// Sets one braille dot at `(x, y)` in the 2x4 dot grid of a cell.
    ///
    /// Out-of-bounds dots are ignored. A dot composes with the cell's other
    /// dots and uses the current foreground, background, and attributes.
    pub fn set_dot(&mut self, x: i32, y: i32) -> Result<(), CanvasError> {
        let Some((cell_x, cell_y, bit)) = self.dot_position(x, y) else {
            return Ok(());
        };
        let index = cell_y * self.width() + cell_x;
        let mask = self.dots[index] | bit;
        self.dots[index] = mask;
        self.paint_braille(cell_x as i32, cell_y as i32, mask)
    }

    /// Clears one braille dot.
    pub fn clear_dot(&mut self, x: i32, y: i32) -> Result<(), CanvasError> {
        let Some((cell_x, cell_y, bit)) = self.dot_position(x, y) else {
            return Ok(());
        };
        let index = cell_y * self.width() + cell_x;
        let mask = self.dots[index] & !bit;
        self.dots[index] = mask;
        self.paint_braille(cell_x as i32, cell_y as i32, mask)
    }

    /// Sets several dots at once.
    pub fn set_dots(&mut self, dots: &[(i32, i32)]) -> Result<(), CanvasError> {
        for &(x, y) in dots {
            self.set_dot(x, y)?;
        }
        Ok(())
    }

    /// Sets a vertical braille run in one cell column from `y0` to `y1`,
    /// inclusive in either order.
    pub fn set_run(&mut self, x: i32, y0: i32, y1: i32) -> Result<(), CanvasError> {
        let (start, end) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        let mut y = start;
        while y <= end {
            self.set_dot(x, y)?;
            y = y.saturating_add(1);
        }
        Ok(())
    }

    fn dot_position(&self, x: i32, y: i32) -> Option<(usize, usize, u8)> {
        if x < 0 || y < 0 {
            return None;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.dot_width() || y >= self.dot_height() {
            return None;
        }
        let cell_x = x / 2;
        let cell_y = y / 4;
        let right = (x % 2) as u8;
        let vertical = (y % 4) as u8;
        let bit = match (right, vertical) {
            (0, 0) => 0x01,
            (0, 1) => 0x02,
            (0, 2) => 0x04,
            (0, 3) => 0x40,
            (1, 0) => 0x08,
            (1, 1) => 0x10,
            (1, 2) => 0x20,
            (1, 3) => 0x80,
            _ => return None,
        };
        Some((cell_x, cell_y, bit))
    }

    fn paint_braille(&mut self, x: i32, y: i32, mask: u8) -> Result<(), CanvasError> {
        let symbol = if mask == 0 {
            " ".to_string()
        } else {
            char::from_u32(0x2800 + u32::from(mask))
                .expect("braille bitmask is a valid code point")
                .to_string()
        };
        let cell = Cell::new(symbol, self.foreground, self.background, self.attributes)?;
        self.image.patch_cells(&[CellEdit {
            position: ImagePosition::new(y as usize, x as usize),
            cell,
        }])?;
        Ok(())
    }

    fn clear_dots(&mut self, x: i32, y: i32, width: i32) {
        if x < 0 || y < 0 || x as usize >= self.width() || y as usize >= self.height() {
            return;
        }
        for column in x..x.saturating_add(width.max(1)) {
            if column >= 0 && (column as usize) < self.width() {
                let index = y as usize * self.width() + column as usize;
                self.dots[index] = 0;
            }
        }
    }

    /// Reset every cell to a blank cell in the current style.
    pub fn clear(&mut self) -> Result<(), CanvasError> {
        let blank = Cell::styled(self.foreground, self.background, self.attributes, " ")?;
        self.image = Image::new(self.width(), self.height(), blank)?;
        self.dots.fill(0);
        Ok(())
    }

    /// Fill a `width` x `height` cell rectangle at `(x, y)` with `symbol`,
    /// clamped to the canvas.
    ///
    /// The symbol is validated once and written as one batched patch per row.
    pub fn fill_rect(
        &mut self,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
        symbol: impl Into<String>,
    ) -> Result<(), CanvasError> {
        let cell = Cell::new(
            symbol.into(),
            self.foreground,
            self.background,
            self.attributes,
        )?;
        let left = x.max(0);
        let top = y.max(0);
        let right = x.saturating_add(i32::from(width)).min(self.width() as i32);
        let bottom = y
            .saturating_add(i32::from(height))
            .min(self.height() as i32);
        if left >= right || top >= bottom {
            return Ok(());
        }
        for row in top..bottom {
            self.write_run(row, left, right, &cell)?;
        }
        Ok(())
    }

    /// Draw a single-line box outline of `width` x `height` cells at `(x, y)`.
    pub fn stroke_rect(
        &mut self,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
    ) -> Result<(), CanvasError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let right = x.saturating_add(i32::from(width)).saturating_sub(1);
        let bottom = y.saturating_add(i32::from(height)).saturating_sub(1);
        self.set(x, y, "┌")?;
        self.set(right, y, "┐")?;
        self.set(x, bottom, "└")?;
        self.set(right, bottom, "┘")?;
        for column in x.saturating_add(1)..right {
            self.set(column, y, "─")?;
            self.set(column, bottom, "─")?;
        }
        for row in y.saturating_add(1)..bottom {
            self.set(x, row, "│")?;
            self.set(right, row, "│")?;
        }
        Ok(())
    }

    /// Draw a `symbol` line from `(x0, y0)` to `(x1, y1)`, clipped to the
    /// canvas.
    pub fn line(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        symbol: impl Into<String>,
    ) -> Result<(), CanvasError> {
        let cell = Cell::new(
            symbol.into(),
            self.foreground,
            self.background,
            self.attributes,
        )?;
        let Some((mut x0, mut y0, x1, y1)) = clip_line(
            i64::from(x0),
            i64::from(y0),
            i64::from(x1),
            i64::from(y1),
            self.width() as i64,
            self.height() as i64,
        ) else {
            return Ok(());
        };
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.write_validated(x0 as i32, y0 as i32, cell.clone())?;
            if x0 == x1 && y0 == y1 {
                break;
            }
            let twice = error.saturating_mul(2);
            if twice >= dy {
                error = error.saturating_add(dy);
                x0 += sx;
            }
            if twice <= dx {
                error = error.saturating_add(dx);
                y0 += sy;
            }
        }
        Ok(())
    }

    /// Draw `text` grapheme by grapheme from `(x, y)`, moving to the next row
    /// after each `\n`.
    ///
    /// A grapheme the cell layer rejects is drawn as the replacement character.
    pub fn fill_text(&mut self, text: &str, x: i32, y: i32) -> Result<(), CanvasError> {
        let mut column = x;
        let mut row = y;
        for grapheme in text.graphemes(true) {
            if grapheme == "\n" {
                row = row.saturating_add(1);
                column = x;
                continue;
            }
            let (symbol, width) =
                match Cell::new(grapheme, self.foreground, self.background, self.attributes) {
                    Ok(cell) => (grapheme, cell.width() as i32),
                    Err(_) => ("�", 1),
                };
            self.set(column, row, symbol)?;
            column = column.saturating_add(width);
        }
        Ok(())
    }

    /// Draw `source` into the `width` x `height` cell box at `(x, y)` with
    /// default render options.
    pub fn draw_image(
        &mut self,
        source: &RasterImage,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
    ) -> Result<(), CanvasError> {
        self.draw_image_with(source, x, y, width, height, ImageRenderOptions::default())
    }

    /// Draw `source` into the `width` x `height` cell box at `(x, y)` with
    /// explicit render options, clamped to the canvas.
    pub fn draw_image_with(
        &mut self,
        source: &RasterImage,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
        options: ImageRenderOptions,
    ) -> Result<(), CanvasError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let rendered = crate::raster::symbols(source, width, height, options)?;
        let mut edits = Vec::new();
        for row in 0..rendered.height() {
            for column in 0..rendered.width() {
                let target_x = x.saturating_add(column as i32);
                let target_y = y.saturating_add(row as i32);
                if target_x < 0
                    || target_y < 0
                    || target_x as usize >= self.width()
                    || target_y as usize >= self.height()
                {
                    continue;
                }
                if let crate::data::CellSlot::Lead(cell) = rendered.cell_at(row, column) {
                    if cell.width() == 2 && target_x as usize + 1 >= self.width() {
                        continue;
                    }
                    edits.push(CellEdit {
                        position: ImagePosition::new(target_y as usize, target_x as usize),
                        cell: cell.clone(),
                    });
                    self.clear_dots(target_x, target_y, cell.width() as i32);
                }
            }
        }
        if !edits.is_empty() {
            self.image.patch_cells(&edits)?;
        }
        Ok(())
    }

    /// Consume the context and return its painted image.
    fn finish(self) -> Image {
        self.image
    }
}

/// Clip the segment to a `width` x `height` canvas and return the clipped
/// endpoints, or `None` when it lies entirely outside.
fn clip_line(
    mut x0: i64,
    mut y0: i64,
    mut x1: i64,
    mut y1: i64,
    width: i64,
    height: i64,
) -> Option<(i64, i64, i64, i64)> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let code = |x: i64, y: i64| {
        (if x < 0 {
            1
        } else if x >= width {
            2
        } else {
            0
        }) | (if y < 0 {
            4
        } else if y >= height {
            8
        } else {
            0
        })
    };
    loop {
        let first = code(x0, y0);
        let second = code(x1, y1);
        if first | second == 0 {
            return Some((x0, y0, x1, y1));
        }
        if first & second != 0 {
            return None;
        }
        let outside = if first != 0 { first } else { second };
        let (nx, ny) = if outside & 8 != 0 {
            let y = height - 1;
            let dy = y1 - y0;
            let x = if dy == 0 {
                x0
            } else {
                x0 + ((x1 - x0) * (y - y0)) / dy
            };
            (x, y)
        } else if outside & 4 != 0 {
            let y = 0;
            let dy = y1 - y0;
            let x = if dy == 0 {
                x0
            } else {
                x0 + ((x1 - x0) * (y - y0)) / dy
            };
            (x, y)
        } else if outside & 2 != 0 {
            let x = width - 1;
            let dx = x1 - x0;
            let y = if dx == 0 {
                y0
            } else {
                y0 + ((y1 - y0) * (x - x0)) / dx
            };
            (x, y)
        } else {
            let x = 0;
            let dx = x1 - x0;
            let y = if dx == 0 {
                y0
            } else {
                y0 + ((y1 - y0) * (x - x0)) / dx
            };
            (x, y)
        };
        if outside == first {
            x0 = nx;
            y0 = ny;
        } else {
            x1 = nx;
            y1 = ny;
        }
    }
}

/// Drawing callback stored in [`CanvasProps::draw`]; it receives the mutable
/// canvas.
pub type CanvasDraw = Arc<dyn Fn(&mut CanvasContext) + Send + Sync>;

/// Configuration for [`canvas`].
#[derive(Clone, Default)]
pub struct CanvasProps {
    /// Canvas width in cells; defaults to `1` and is floored at `1`.
    pub width: Attr<u16>,
    /// Canvas height in cells; defaults to `1` and is floored at `1`.
    pub height: Attr<u16>,
    /// Drawing callback; defaults to a no-op.
    pub draw: Attr<CanvasDraw>,
}

impl fmt::Debug for CanvasProps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanvasProps")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("draw", &"<drawing callback>")
            .finish()
    }
}

/// Fixed-size canvas painted by a drawing callback; see [`CanvasProps`] for its
/// configuration.
pub fn canvas(cx: &mut ComponentContext, props: &Props<CanvasProps>) -> Node {
    let theme = cx.use_theme();
    let width = (props.width | 1).max(1);
    let height = (props.height | 1).max(1);
    let Ok(mut drawing) = CanvasContext::try_new(width, height, &theme) else {
        return ui! { <view dom={props.dom.clone()}>{Text::new("canvas exceeds the supported size")}</view> };
    };
    let draw = props.draw.clone() | Arc::new(|_: &mut CanvasContext| {});
    draw(&mut drawing);
    ui! { <view dom={props.dom.clone()}>{drawing.finish()}</view> }
}
