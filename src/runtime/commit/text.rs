use crossterm::style::Color;

use crate::basic::text_layout::{self, ItemKind, TextLayout};
use crate::{Cell, Image, Text, TextAlign, TextOverflow};

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
