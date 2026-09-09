use crossterm::style::Color;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{Cell, Image, MAX_GLYPH_BYTES, Text, TextAlign, TextOverflow, TextStyle, TextWrap};

use super::geometry::RectI;
use super::style::slot;
use super::types::ComputedText;

#[derive(Clone)]
struct Glyph {
    symbol: String,
    width: usize,
    style: ComputedText,
}

pub(super) fn merge_text(parent: ComputedText, style: &TextStyle) -> ComputedText {
    ComputedText {
        foreground: slot(&style.foreground).unwrap_or(parent.foreground),
        background: slot(&style.background).or(parent.background),
        attributes: style.attr.resolve(parent.attributes),
    }
}

fn glyphs(text: &Text, inherited: ComputedText) -> Vec<Vec<Glyph>> {
    let mut lines = vec![Vec::new()];
    for span in &text.spans {
        let style = merge_text(inherited, &span.style);
        let content = span.content.replace("\r\n", "\n").replace('\r', "\n");
        for grapheme in content.graphemes(true) {
            if grapheme == "\n" {
                lines.push(Vec::new());
                continue;
            }
            if grapheme == "\t" {
                let column = lines
                    .last()
                    .expect("text line missing")
                    .iter()
                    .map(|glyph: &Glyph| glyph.width)
                    .fold(0usize, usize::saturating_add);
                lines
                    .last_mut()
                    .expect("text line missing")
                    .extend((0..4 - column % 4).map(|_| Glyph {
                        symbol: " ".into(),
                        width: 1,
                        style,
                    }));
                continue;
            }
            let mut symbol = if grapheme.chars().any(char::is_control) {
                "�".to_string()
            } else {
                grapheme.to_string()
            };
            let mut width = UnicodeWidthStr::width(symbol.as_str());
            if symbol.len() > MAX_GLYPH_BYTES || !(1..=2).contains(&width) {
                symbol = "�".to_string();
                width = 1;
            }
            if width > 0 {
                lines.last_mut().expect("text line missing").push(Glyph {
                    symbol,
                    width,
                    style,
                });
            }
        }
    }
    lines
}

pub(super) fn text_measure(
    text: &Text,
    offered_width: Option<i32>,
    inherited: ComputedText,
) -> (i32, i32) {
    let lines = glyphs(text, merge_text(inherited, &text.style));
    let max_width = lines
        .iter()
        .map(|line| {
            line.iter()
                .fold(0i32, |sum, glyph| sum.saturating_add(glyph.width as i32))
        })
        .max()
        .unwrap_or(0);
    let width = offered_width.map_or(max_width, |value| max_width.min(value.max(0)));
    let wrapped = wrap_lines(lines, text.wrap, offered_width.unwrap_or(max_width).max(1));
    (width, wrapped.len() as i32)
}

fn wrap_lines(lines: Vec<Vec<Glyph>>, wrap: TextWrap, width: i32) -> Vec<Vec<Glyph>> {
    let mut result = Vec::new();
    for line in lines {
        if wrap == TextWrap::NoWrap {
            result.push(line);
            continue;
        }
        let mut current = Vec::new();
        let mut used: i32 = 0;
        for glyph in line {
            if used.saturating_add(glyph.width as i32) > width && !current.is_empty() {
                if wrap == TextWrap::Word
                    && let Some(index) = current
                        .iter()
                        .rposition(|item: &Glyph| item.symbol.chars().all(char::is_whitespace))
                {
                    let tail = current.split_off(index + 1);
                    result.push(current);
                    current = tail;
                    used = current
                        .iter()
                        .fold(0i32, |sum, item| sum.saturating_add(item.width as i32));
                } else {
                    result.push(std::mem::take(&mut current));
                    used = 0;
                }
            }
            if glyph.width as i32 <= width || current.is_empty() {
                used = used.saturating_add(glyph.width as i32);
                current.push(glyph);
            }
        }
        result.push(current);
    }
    if result.is_empty() {
        result.push(Vec::new());
    }
    result
}

pub(super) fn raster_text(
    text: &Text,
    rect: RectI,
    visible: RectI,
    inherited: ComputedText,
    backdrop: Color,
) -> Option<Image> {
    let visible = rect.intersection(visible)?;
    if rect.width <= 0 || rect.height <= 0 {
        return None;
    }
    let text_style = merge_text(inherited, &text.style);
    let lines = wrap_lines(glyphs(text, text_style), text.wrap, rect.width);
    let horizontal_overflow = lines.iter().any(|line| {
        line.iter()
            .fold(0i32, |sum, glyph| sum.saturating_add(glyph.width as i32))
            > rect.width
    });
    let blank = |style: ComputedText| {
        Cell::styled(
            style.foreground,
            style.background.unwrap_or(backdrop),
            style.attributes,
            " ",
        )
        .expect("space is valid")
    };
    let row_start = (visible.line - rect.line) as usize;
    let row_end = (visible.bottom() - rect.line) as usize;
    let col_start = (visible.column - rect.column).max(0) as usize;
    let col_end = col_start + visible.width as usize;
    let mut rows = Vec::with_capacity(visible.height as usize);
    for row_index in row_start..row_end {
        let line = lines.get(row_index).cloned().unwrap_or_default();
        let line_width = line
            .iter()
            .fold(0i32, |sum, glyph| sum.saturating_add(glyph.width as i32));
        let offset = match text.align {
            TextAlign::Center => ((rect.width - line_width) / 2).max(0),
            TextAlign::End => (rect.width - line_width).max(0),
            TextAlign::Start => 0,
        } as usize;
        let ellipsis = text.overflow == TextOverflow::Ellipsis
            && (lines.len() > rect.height as usize || horizontal_overflow)
            && row_index + 1 == rect.height as usize;
        rows.push(raster_line(
            &line,
            offset,
            rect.width as usize,
            col_start,
            col_end,
            ellipsis,
            text_style,
            backdrop,
            &blank,
        ));
    }
    Image::from_rows(rows).ok()
}

#[allow(clippy::too_many_arguments)]
fn raster_line(
    line: &[Glyph],
    offset: usize,
    rect_width: usize,
    visible_start: usize,
    visible_end: usize,
    ellipsis: bool,
    text_style: ComputedText,
    backdrop: Color,
    blank: &impl Fn(ComputedText) -> Cell,
) -> Vec<Cell> {
    let visible_start = visible_start.min(rect_width);
    let visible_end = visible_end.min(rect_width).max(visible_start);
    let ellipsis_start = rect_width.saturating_sub(1);
    let content_end = if ellipsis { ellipsis_start } else { rect_width };
    let mut glyphs_at = vec![None; rect_width];
    let mut visual = offset.min(rect_width);
    for glyph in line {
        let glyph_end = visual.saturating_add(glyph.width);
        if visual < rect_width && glyph_end <= rect_width {
            glyphs_at[visual] = Some(glyph);
        }
        visual = glyph_end;
    }

    let mut cells = Vec::new();
    let mut column = visible_start;
    while column < visible_end {
        if ellipsis && column == ellipsis_start {
            cells.push(
                Cell::styled(
                    text_style.foreground,
                    text_style.background.unwrap_or(backdrop),
                    text_style.attributes,
                    "…",
                )
                .unwrap_or_else(|_| blank(text_style)),
            );
            column += 1;
            continue;
        }
        if column >= content_end {
            cells.push(blank(text_style));
            column += 1;
            continue;
        }
        if let Some(glyph) = glyphs_at[column]
            && column.saturating_add(glyph.width) <= content_end
            && column.saturating_add(glyph.width) <= visible_end
            && let Ok(cell) = Cell::styled(
                glyph.style.foreground,
                glyph.style.background.unwrap_or(backdrop),
                glyph.style.attributes,
                glyph.symbol.clone(),
            )
        {
            cells.push(cell);
            column += glyph.width;
            continue;
        }
        cells.push(blank(text_style));
        column += 1;
    }
    cells
}
