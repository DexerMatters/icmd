use crossterm::style::Color;

use crate::basic::editor_surface::{CommittedLayout, EditorSurface};
use crate::basic::text_layout::{self, ItemKind, TextLayout};
use crate::{Cell, Image, Text, TextAlign, TextOverflow, TextWrap};

use super::geometry::RectI;
use super::style::slot;
use super::types::ComputedText;

/// Merge a span's text style over the inherited computed style.
pub(super) fn merge_text(parent: ComputedText, style: &crate::TextStyle) -> ComputedText {
    ComputedText {
        foreground: slot(&style.foreground).unwrap_or(parent.foreground),
        background: slot(&style.background).or(parent.background),
        attributes: style.attr.resolve(parent.attributes),
    }
}

/// Build the canonical layout of `text` at `width`, resolving span styles over
/// `inherited`.
pub(super) fn layout(text: &Text, width: usize, inherited: ComputedText) -> TextLayout {
    text_layout::layout_text(text, width, inherited, merge_text, |computed| *computed)
}

pub(super) fn text_measure(
    text: &Text,
    offered_width: Option<i32>,
    inherited: ComputedText,
) -> (i32, i32) {
    if let Some(surface) = &text.editor {
        return editor_measure(surface, offered_width, text.wrap);
    }
    // The intrinsic width is the widest unwrapped logical line. `NoWrap` at a
    // large width breaks only at explicit newlines, so its widest row is
    // exactly that intrinsic width.
    let natural = layout(text, usize::MAX / 4, inherited);
    let natural_width = natural.max_row_width() as i32;
    let width = offered_width.map_or(natural_width, |value| natural_width.min(value.max(0)));
    let wrapped = layout(
        text,
        offered_width.unwrap_or(natural_width).max(1) as usize,
        inherited,
    );
    (width, wrapped.row_count() as i32)
}

/// Measure an editor surface.
///
/// `NoWrap` reports the intrinsic width of the widest logical line so a scroll
/// host can pan across it. Wrapped modes report the offered width: their rows
/// are built at whatever width the parent finally grants, so wrapping is
/// correct in the same frame with no measurement feedback.
fn editor_measure(
    surface: &EditorSurface,
    offered_width: Option<i32>,
    wrap: TextWrap,
) -> (i32, i32) {
    // An empty control sizes to its placeholder so the hint is visible and the
    // box does not collapse to zero cells. An empty placeholder still occupies
    // one cell, which is the caret.
    if surface.value.is_empty() {
        let width = if surface.placeholder.is_empty() {
            1
        } else {
            placeholder_width(&surface.placeholder)
        };
        return (width as i32, 1);
    }
    let natural = surface_layout(surface, usize::MAX / 4, wrap);
    match wrap {
        // An unwrapped editor reports the intrinsic width of its widest logical
        // line so a scroll host can pan across it.
        TextWrap::NoWrap => (natural.max_row_width() as i32, natural.row_count() as i32),
        // A wrapped editor reports the offered content width. With no offer yet
        // it reports its natural width; the parent's offer then drives the wrap.
        TextWrap::Soft | TextWrap::Hard => match offered_width.filter(|width| *width > 0) {
            Some(width) => {
                let layout = surface_layout(surface, width as usize, wrap);
                (width, layout.row_count() as i32)
            }
            None => (natural.max_row_width() as i32, natural.row_count() as i32),
        },
    }
}

fn placeholder_width(placeholder: &str) -> usize {
    unicode_segmentation::UnicodeSegmentation::graphemes(placeholder, true)
        .map(|grapheme| {
            if grapheme == "\t" {
                4
            } else {
                unicode_width::UnicodeWidthStr::width(grapheme).max(1)
            }
        })
        .sum()
}

/// Build the canonical layout of an editor surface at a content width.
pub(super) fn surface_layout(surface: &EditorSurface, width: usize, wrap: TextWrap) -> TextLayout {
    text_layout::layout_text(
        &Text::new(surface.value.as_str()).wrap(wrap),
        width.max(1),
        ComputedText::default(),
        |parent, _| parent,
        |style| *style,
    )
}

pub(super) fn raster_text(
    text: &Text,
    rect: RectI,
    visible: RectI,
    inherited: ComputedText,
    backdrop: Color,
) -> Option<Image> {
    if let Some(surface) = &text.editor {
        return editor_raster(text, surface, rect, visible, backdrop);
    }
    let visible = rect.intersection(visible)?;
    if rect.width <= 0 || rect.height <= 0 {
        return None;
    }
    let text_style = merge_text(inherited, &text.style);
    let layout = layout(text, rect.width as usize, inherited);
    let horizontal_overflow = layout.width() > rect.width as usize;
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
        let items = layout.row_items(row_index);
        let line_width = layout.row_width(row_index);
        let offset = match text.align {
            TextAlign::Center => rect.width as usize - (rect.width as usize).min(line_width),
            TextAlign::End => (rect.width as usize).saturating_sub(line_width),
            TextAlign::Start => 0,
        };
        let offset = match text.align {
            TextAlign::Center => offset / 2,
            _ => offset,
        };
        let ellipsis = text.overflow == TextOverflow::Ellipsis
            && (layout.row_count() > rect.height as usize || horizontal_overflow)
            && row_index + 1 == rect.height as usize;
        rows.push(raster_line(
            items,
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
    items: &[text_layout::Item],
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
    let mut glyphs_at: Vec<Option<&text_layout::Item>> = vec![None; rect_width];
    let mut visual = offset.min(rect_width);
    for item in items {
        if item.kind == ItemKind::Newline || item.width == 0 {
            continue;
        }
        let glyph_end = visual.saturating_add(item.width);
        if visual < rect_width && glyph_end <= rect_width {
            glyphs_at[visual] = Some(item);
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
        if let Some(item) = glyphs_at[column]
            && column.saturating_add(item.width) <= content_end
            && column.saturating_add(item.width) <= visible_end
            && let Ok(cell) = Cell::styled(
                item.style.foreground,
                item.style.background.unwrap_or(backdrop),
                item.style.attributes,
                cell_symbol(item),
            )
        {
            cells.push(cell);
            column += item.width;
            continue;
        }
        cells.push(blank(text_style));
        column += 1;
    }
    cells
}

/// The painted symbol of a laid-out glyph: a tab expands to its cell width.
fn cell_symbol(item: &text_layout::Item) -> String {
    if item.symbol == "\t" {
        " ".repeat(item.width)
    } else {
        item.symbol.clone()
    }
}

/// Rasterize an editor surface from the canonical layout at the committed
/// content width, and publish that layout into the surface's probe.
fn editor_raster(
    text: &Text,
    surface: &EditorSurface,
    rect: RectI,
    visible: RectI,
    backdrop: Color,
) -> Option<Image> {
    let wrap = text.wrap;
    let layout = surface_layout(surface, rect.width.max(1) as usize, wrap);
    if let Some(probe) = &text.probe {
        // The offsets the paint path actually shifted the surface by: the
        // runtime clamps the requested offset to the real extent, so publishing
        // the requested value would double-count what the pointer already sees.
        let (applied_x, applied_y) = text
            .applied_scroll
            .unwrap_or((surface.scroll_x, surface.scroll_y));
        probe.publish(CommittedLayout {
            layout: std::sync::Arc::new(layout.clone()),
            width: rect.width.max(0) as usize,
            height: rect.height.max(0) as usize,
            wrap,
            applied_x,
            applied_y,
            content_line: 0,
            content_column: 0,
        });
    }
    let visible = rect.intersection(visible)?;
    if rect.width <= 0 || rect.height <= 0 {
        return None;
    }
    let base = merge_text(ComputedText::default(), &text.style);
    let placeholder_style = merge_text(base, &surface.placeholder_style);
    let selection_style = merge_text(base, &surface.selection_style);
    let selection_inactive_style = merge_text(base, &surface.selection_inactive_style);
    let caret_style = merge_text(base, &surface.caret_style);
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
        let items: &[text_layout::Item] = layout.row_items(row_index);
        let mut cells = vec![blank(base); rect.width as usize];
        if surface.value.is_empty() {
            if row_index == 0 {
                let mut column = 0usize;
                for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(
                    surface.placeholder.as_str(),
                    true,
                ) {
                    if column >= rect.width as usize {
                        break;
                    }
                    let style = if surface.focused && column == 0 {
                        caret_style
                    } else {
                        placeholder_style
                    };
                    if let Ok(cell) = Cell::styled(
                        style.foreground,
                        style.background.unwrap_or(backdrop),
                        style.attributes,
                        grapheme,
                    ) {
                        cells[column] = cell;
                    }
                    column = column.saturating_add(grapheme_width(grapheme));
                }
            }
        } else {
            let mut column = 0usize;
            for item in items {
                if item.kind == ItemKind::Separator || item.width == 0 {
                    continue;
                }
                let symbol = if item.symbol == "\t" {
                    " ".repeat(item.width)
                } else {
                    item.symbol.clone()
                };
                let style = if surface.focused
                    && surface.caret >= item.source.start
                    && surface.caret < item.source.end
                {
                    caret_style
                } else if surface
                    .selection
                    .is_some_and(|(start, end)| item.source.start < end && item.source.end > start)
                {
                    if surface.focused {
                        selection_style
                    } else {
                        selection_inactive_style
                    }
                } else {
                    base
                };
                let end = column.saturating_add(item.width);
                if column < rect.width as usize
                    && end <= rect.width as usize
                    && let Ok(cell) = Cell::styled(
                        style.foreground,
                        style.background.unwrap_or(backdrop),
                        style.attributes,
                        symbol,
                    )
                {
                    cells[column] = cell;
                }
                column = end;
            }
            if surface.focused
                && surface.caret >= layout.source_len()
                && column < rect.width as usize
                && let Ok(cell) = Cell::styled(
                    caret_style.foreground,
                    caret_style.background.unwrap_or(backdrop),
                    caret_style.attributes,
                    " ",
                )
            {
                cells[column] = cell;
            }
        }
        // `Image::from_rows` requires every row to occupy the same number of
        // terminal cells. A row holding a wide grapheme can end up one cell
        // short of the viewport even though its cell count matches, so pad by
        // measured cell width rather than by cell count.
        let start = col_start.min(cells.len());
        let end = col_end.min(cells.len()).max(start);
        let target = visible.width.max(0) as usize;
        let mut visible_row = Vec::with_capacity(target + 1);
        let mut painted = 0usize;
        for cell in cells[start..end].iter() {
            let width = cell.width();
            if painted + width > target {
                break;
            }
            visible_row.push(cell.clone());
            painted += width;
        }
        while painted < target {
            visible_row.push(blank(base));
            painted += 1;
        }
        rows.push(visible_row);
    }
    Image::from_rows(rows).ok()
}

fn grapheme_width(grapheme: &str) -> usize {
    if grapheme == "\t" {
        4
    } else {
        unicode_width::UnicodeWidthStr::width(grapheme).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::style::{Attributes, Color};
    use unicode_segmentation::UnicodeSegmentation;

    fn inherited() -> ComputedText {
        ComputedText {
            foreground: Color::Reset,
            background: None,
            attributes: Attributes::default(),
        }
    }

    /// Painted row strings of `text`, with separators omitted.
    fn strings(value: &str, wrap: crate::TextWrap, width: usize) -> Vec<String> {
        let layout = layout(&Text::new(value).wrap(wrap), width, inherited());
        (0..layout.row_count())
            .map(|index| {
                layout
                    .row_items(index)
                    .iter()
                    .filter(|item| item.kind != ItemKind::Separator)
                    .map(cell_symbol)
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn soft_splits_an_overlong_unbreakable_fragment() {
        assert_eq!(strings("abcdef", crate::TextWrap::Soft, 3), ["abc", "def"]);
    }

    #[test]
    fn soft_uses_unicode_break_opportunities() {
        assert_eq!(
            strings("hello world", crate::TextWrap::Soft, 5),
            ["hello", "world"]
        );
    }

    #[test]
    fn soft_wraps_cjk_at_unicode_breaks() {
        assert_eq!(
            strings("你好世界", crate::TextWrap::Soft, 4),
            ["你好", "世界"]
        );
    }

    #[test]
    fn trailing_spaces_are_not_lost() {
        assert_eq!(strings("a   ", crate::TextWrap::Soft, 4), ["a   "]);
    }

    #[test]
    fn hard_wraps_trailing_spaces_instead_of_overflowing() {
        assert_eq!(
            strings("     ", crate::TextWrap::Hard, 2),
            ["  ", "  ", " "]
        );
    }

    #[test]
    fn zero_and_one_cell_widths_do_not_panic() {
        assert!(!strings("界", crate::TextWrap::Soft, 0).is_empty());
        assert!(!strings("界", crate::TextWrap::Hard, 1).is_empty());
    }

    #[test]
    fn hard_splits_only_between_graphemes() {
        assert_eq!(
            strings("a\u{301}b界", crate::TextWrap::Hard, 2),
            ["a\u{301}b", "界"]
        );
    }

    #[test]
    fn hard_wraps_at_the_cell_limit_instead_of_an_earlier_word_break() {
        assert_eq!(strings("ab cd", crate::TextWrap::Hard, 4), ["ab c", "d"]);
        assert_eq!(strings("ab cd", crate::TextWrap::Soft, 4), ["ab", "cd"]);
    }

    #[test]
    fn explicit_empty_and_trailing_lines_are_retained() {
        assert_eq!(strings("a\n\n", crate::TextWrap::Soft, 3), ["a", "", ""]);
    }

    #[test]
    fn measure_uses_the_widest_unwrapped_line() {
        let (width, height) = text_measure(&Text::new("ab\ncdef"), None, inherited());
        assert_eq!((width, height), (4, 2));
        // Without a wrap policy the text stays one row and is clipped to the
        // offered width; with soft wrapping it grows to two rows.
        let (width, height) = text_measure(&Text::new("abcdef"), Some(3), inherited());
        assert_eq!((width, height), (3, 1));
        let (width, height) = text_measure(
            &Text::new("abcdef").wrap(crate::TextWrap::Soft),
            Some(3),
            inherited(),
        );
        assert_eq!((width, height), (3, 2));
    }

    #[test]
    fn measure_counts_graphemes_not_bytes() {
        let value = "e\u{301}x";
        assert_eq!(value.graphemes(true).count(), 2);
        let (width, height) = text_measure(&Text::new(value), None, inherited());
        assert_eq!((width, height), (2, 1));
    }
}
