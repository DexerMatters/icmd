//! Text shaping, measurement, and rasterization for the commit stage.
//! Owns the width-independent natural-layout cache, the canonical cell layouts
//! used for painting and hit testing, and the plain and editor cell painters.

use crossterm::style::Color;

use crate::basic::editor_surface::{CommittedLayout, EditorSurface};
use crate::basic::selection::{SelectionOverlay, SelectionStyles};
use crate::basic::text_layout::{self, ItemKind, TextLayout};
use std::sync::Arc;

use crate::{Cell, DomId, EmojiMerging, Image, Text, TextAlign, TextOverflow, TextWrap};

use super::geometry::RectI;
use super::style::slot;
use super::types::ComputedText;

/// Shape-derived half of a text leaf: its canonical layout and source text.
#[derive(Debug, Clone)]
pub struct TextGeometry {
    /// Canonical cell layout used for painting and hit testing.
    pub layout: Arc<TextLayout>,
    /// Normalized source text the layout was shaped from.
    pub value: Arc<str>,
}

pub(super) fn merge_text(parent: ComputedText, style: &crate::TextStyle) -> ComputedText {
    ComputedText {
        foreground: slot(&style.foreground).unwrap_or(parent.foreground),
        background: slot(&style.background).or(parent.background),
        attributes: style.attr.resolve(parent.attributes),
    }
}

/// Shapes `text` at most `width` cells wide, merging `inherited` into each span.
pub fn layout(
    text: &Text,
    width: usize,
    inherited: ComputedText,
    merging: EmojiMerging,
) -> TextLayout {
    let base = merge_text(inherited, &text.style);
    text_layout::layout_text(text, width, base, merging, merge_text, |computed| *computed)
}

/// Width-independent natural layout kept between frames and validated by value.
#[derive(Clone)]
pub struct CachedNatural {
    /// Text the layout was shaped from, compared by value on lookup.
    pub text: Box<Text>,
    /// Inherited computed text style the layout was shaped with.
    pub inherited: ComputedText,
    /// Emoji merging policy the layout was shaped with.
    pub merging: EmojiMerging,
    /// Cached unwrapped layout, shared by reference across frames.
    pub layout: Arc<TextLayout>,
}

/// Per-node natural-layout cache owned by a commit stage and borrowed while measuring.
pub type NaturalCache = std::cell::RefCell<std::collections::HashMap<DomId, CachedNatural>>;

fn same_layout_inputs(left: &Text, right: &Text) -> bool {
    left.spans == right.spans
        && left.style == right.style
        && left.wrap == right.wrap
        && left.align == right.align
        && left.overflow == right.overflow
}

/// Measures a text leaf as `(width, height)` in cells under the optional width and
/// height offers, caching the width-independent natural layout by node id when both
/// `id` and `cache` are given; an editor surface measures from its placeholder or value.
pub fn text_measure(
    id: Option<DomId>,
    cache: Option<&NaturalCache>,
    text: &Text,
    offered_width: Option<i32>,
    offered_height: Option<i32>,
    inherited: ComputedText,
    merging: EmojiMerging,
) -> (i32, i32) {
    let (natural, cached): (Arc<TextLayout>, bool) = match (id, cache) {
        (Some(id), Some(cache)) => {
            if let Some(entry) = cache.borrow().get(&id)
                && entry.inherited == inherited
                && entry.merging == merging
                && same_layout_inputs(&entry.text, text)
            {
                (entry.layout.clone(), true)
            } else {
                let layout = Arc::new(layout(text, usize::MAX / 4, inherited, merging));
                cache.borrow_mut().insert(
                    id,
                    CachedNatural {
                        text: Box::new(text.clone()),
                        inherited,
                        merging,
                        layout: layout.clone(),
                    },
                );
                (layout, true)
            }
        }
        _ => (
            Arc::new(layout(text, usize::MAX / 4, inherited, merging)),
            false,
        ),
    };
    let _ = cached;
    if let Some(surface) = &text.editor {
        return editor_measure(surface, offered_width, offered_height, text.wrap, merging);
    }
    let natural_width = natural.max_row_width() as i32;
    let width = offered_width.map_or(natural_width, |value| natural_width.min(value.max(0)));
    let offered = offered_width.unwrap_or(natural_width.max(1)).max(1) as usize;
    let rows = if matches!(text.wrap, TextWrap::NoWrap)
        || natural_width <= 0
        || offered >= natural_width as usize
    {
        natural.row_count()
    } else {
        layout(text, offered, inherited, merging).row_count()
    };
    (width, rows as i32)
}

fn editor_measure(
    surface: &EditorSurface,
    offered_width: Option<i32>,
    offered_height: Option<i32>,
    wrap: TextWrap,
    merging: EmojiMerging,
) -> (i32, i32) {
    if surface.value.is_empty() {
        if surface.placeholder.is_empty() {
            return (1, 1);
        }
        let layout = layout_for(
            &surface.placeholder,
            usize::MAX / 4,
            TextWrap::NoWrap,
            merging,
        );
        return (layout.max_row_width() as i32, layout.row_count() as i32);
    }
    let natural = surface_layout(surface, usize::MAX / 4, wrap, merging);
    let rows = |width: i32| {
        surface_layout(surface, width.max(1) as usize, wrap, merging).row_count() as i32
    };
    match wrap {
        TextWrap::NoWrap => {
            let _ = offered_height;
            (
                natural.max_row_width() as i32 + 1,
                natural.row_count() as i32,
            )
        }
        TextWrap::Soft | TextWrap::Hard => match offered_width.filter(|width| *width > 0) {
            Some(width) => (width, rows(width)),
            None => (natural.max_row_width() as i32, natural.row_count() as i32),
        },
    }
}

/// Shapes a plain value at `width` cells (minimum one) with the default computed style.
pub fn layout_for(value: &str, width: usize, wrap: TextWrap, merging: EmojiMerging) -> TextLayout {
    text_layout::layout_text(
        &Text::new(value).wrap(wrap),
        width.max(1),
        ComputedText::default(),
        merging,
        |parent, _| parent,
        |style| *style,
    )
}

/// Shapes an editor surface's value at `width` cells (minimum one).
pub fn surface_layout(
    surface: &EditorSurface,
    width: usize,
    wrap: TextWrap,
    merging: EmojiMerging,
) -> TextLayout {
    layout_for(&surface.value, width, wrap, merging)
}

pub(super) fn raster_text(
    text: &Text,
    rect: RectI,
    visible: RectI,
    inherited: ComputedText,
    backdrop: Color,
    scroll: (i32, i32),
    merging: EmojiMerging,
) -> Option<Image> {
    if let Some(surface) = &text.editor {
        return editor_raster(text, surface, rect, visible, backdrop, scroll, merging);
    }
    let layout = layout(text, rect.width as usize, inherited, merging);
    raster_plain_with_layout(text, &layout, rect, visible, inherited, backdrop, None)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn raster_plain_with_layout(
    text: &Text,
    layout: &TextLayout,
    rect: RectI,
    visible: RectI,
    inherited: ComputedText,
    backdrop: Color,
    overlay: Option<&SelectionOverlay>,
) -> Option<Image> {
    let visible = rect.intersection(visible)?;
    if rect.width <= 0 || rect.height <= 0 {
        return None;
    }
    let text_style = merge_text(inherited, &text.style);
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
            layout.text(),
            items,
            offset,
            rect.width as usize,
            col_start,
            col_end,
            ellipsis,
            text_style,
            backdrop,
            overlay,
            &blank,
        ));
    }
    Image::from_rows(rows).ok()
}

#[allow(clippy::too_many_arguments)]
fn raster_line(
    layout_text: &str,
    items: &[text_layout::Item],
    offset: usize,
    rect_width: usize,
    visible_start: usize,
    visible_end: usize,
    ellipsis: bool,
    text_style: ComputedText,
    backdrop: Color,
    overlay: Option<&SelectionOverlay>,
    blank: &impl Fn(ComputedText) -> Cell,
) -> Vec<Cell> {
    let visible_start = visible_start.min(rect_width);
    let visible_end = visible_end.min(rect_width).max(visible_start);
    let ellipsis_start = rect_width.saturating_sub(1);
    let content_end = if ellipsis { ellipsis_start } else { rect_width };
    let window = visible_end.saturating_sub(visible_start);
    let mut glyphs_at: Vec<Option<&text_layout::Item>> = vec![None; window];
    let mut visual = offset.min(rect_width);
    for item in items {
        if item.kind == ItemKind::Newline || item.width == 0 {
            continue;
        }
        if visual >= visible_end {
            break;
        }
        let glyph_end = visual.saturating_add(item.width);
        if item.kind != ItemKind::Separator
            && visual < rect_width
            && glyph_end <= rect_width
            && visual >= visible_start
        {
            glyphs_at[visual - visible_start] = Some(item);
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
        if let Some(item) = glyphs_at[column - visible_start]
            && column.saturating_add(item.width) <= content_end
            && column.saturating_add(item.width) <= visible_end
        {
            let selected = overlay.is_some_and(|overlay| {
                overlay.range.start < item.source.end && item.source.start < overlay.range.end
            });
            let style = match overlay {
                Some(overlay) => {
                    selection_merge(item.style, selected, overlay.focused, &overlay.styles)
                }
                None => item.style,
            };
            if let Ok(cell) = Cell::with_width(
                cell_symbol(layout_text, item),
                style.foreground,
                style.background.unwrap_or(backdrop),
                style.attributes,
                item.width,
            ) {
                cells.push(cell);
                column += item.width;
                continue;
            }
        }
        cells.push(blank(text_style));
        column += 1;
    }
    cells
}

pub(super) fn selection_merge(
    item_style: ComputedText,
    selected: bool,
    focused: bool,
    styles: &SelectionStyles,
) -> ComputedText {
    if !selected {
        return item_style;
    }
    let style = if focused {
        &styles.active
    } else {
        &styles.inactive
    };
    merge_text(item_style, style)
}

/// Returns the rendered symbol for `item`, expanding a tab into `item.width` blank cells.
pub fn cell_symbol(text: &str, item: &text_layout::Item) -> String {
    let symbol = item.symbol(text);
    if symbol == "\t" {
        " ".repeat(item.width)
    } else {
        symbol.to_string()
    }
}

/// Paints an editor surface into a cell image clipped to `visible`, publishing its
/// layout to `text.probe`; `scroll` is the applied `(line, column)` offset in cells.
pub fn editor_raster(
    text: &Text,
    surface: &EditorSurface,
    rect: RectI,
    visible: RectI,
    backdrop: Color,
    scroll: (i32, i32),
    merging: EmojiMerging,
) -> Option<Image> {
    let wrap = text.wrap;
    let showing_placeholder = surface.value.is_empty() && !surface.placeholder.is_empty();
    let layout = if showing_placeholder {
        layout_for(
            &surface.placeholder,
            rect.width.max(1) as usize,
            TextWrap::NoWrap,
            merging,
        )
    } else {
        surface_layout(surface, rect.width.max(1) as usize, wrap, merging)
    };
    if let Some(probe) = &text.probe {
        probe.publish(CommittedLayout {
            layout: std::sync::Arc::new(layout.clone()),
            viewport_width: visible.width.max(1) as usize,
            viewport_height: visible.height.max(1) as usize,
            applied_x: scroll.1.max(0) as usize,
            applied_y: scroll.0.max(0) as usize,
            emoji_merging: merging,
        });
    }
    let visible = rect.intersection(visible)?;
    if rect.width <= 0 || rect.height <= 0 {
        return None;
    }
    let base = merge_text(ComputedText::default(), &text.style);
    let placeholder_style = merge_text(base, &surface.placeholder_style);
    let selection_styles = SelectionStyles {
        active: surface.selection_style.clone(),
        inactive: surface.selection_inactive_style.clone(),
    };
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
    let row_end = row_start + visible.height.max(0) as usize;
    let col_start = (visible.column - rect.column).max(0) as usize;
    let col_end = col_start + visible.width as usize;
    let mut rows = Vec::with_capacity(visible.height as usize);
    for row_index in row_start..row_end {
        let items: &[text_layout::Item] = layout.row_items(row_index);
        let mut cells: Vec<Cell> = Vec::with_capacity(rect.width as usize);
        let mut columns = 0usize;
        let target = rect.width.max(0) as usize;
        let mut caret_span: Option<(usize, usize)> = None;
        let caret_row_matches;
        if showing_placeholder {
            let caret_here = surface.focused && row_index == 0;
            caret_row_matches = caret_here;
            let mut caret_painted = false;
            for item in items {
                if item.width == 0 {
                    continue;
                }
                if item.cell + item.width > target {
                    break;
                }
                while columns < item.cell {
                    cells.push(blank(base));
                    columns += 1;
                }
                let symbol = cell_symbol(layout.text(), item);
                let style = if caret_here && !caret_painted {
                    caret_painted = true;
                    caret_span = Some((item.cell, item.width));
                    caret_style
                } else {
                    placeholder_style
                };
                match Cell::with_width(
                    symbol,
                    style.foreground,
                    style.background.unwrap_or(backdrop),
                    style.attributes,
                    item.width,
                ) {
                    Ok(cell) => {
                        columns += cell.width();
                        cells.push(cell);
                    }
                    Err(_) => {
                        let blank = Cell::styled(
                            style.foreground,
                            style.background.unwrap_or(backdrop),
                            style.attributes,
                            " ",
                        )
                        .unwrap_or_else(|_| blank(base));
                        for _ in 0..item.width {
                            cells.push(blank.clone());
                            columns += 1;
                        }
                    }
                }
            }
            if caret_here
                && !caret_painted
                && columns == 0
                && target > 0
                && let Ok(cell) = Cell::styled(
                    caret_style.foreground,
                    caret_style.background.unwrap_or(backdrop),
                    caret_style.attributes,
                    " ",
                )
            {
                columns += cell.width();
                cells.push(cell);
                caret_span = Some((0, 1));
            }
        } else {
            let mut caret_painted = false;
            for item in items {
                if item.width == 0 {
                    continue;
                }
                if item.cell + item.width > target {
                    break;
                }
                while columns < item.cell {
                    cells.push(blank(base));
                    columns += 1;
                }
                if item.kind == ItemKind::Separator {
                    let on_caret = surface.focused
                        && surface.caret >= item.source.start
                        && surface.caret < item.source.end;
                    if on_caret {
                        caret_painted = true;
                        caret_span = Some((item.cell, 1));
                    }
                    for offset in 0..item.width {
                        let cell = if on_caret && offset == 0 {
                            Cell::styled(
                                caret_style.foreground,
                                caret_style.background.unwrap_or(backdrop),
                                caret_style.attributes,
                                " ",
                            )
                            .unwrap_or_else(|_| blank(base))
                        } else {
                            blank(base)
                        };
                        cells.push(cell);
                    }
                    columns += item.width;
                    continue;
                }
                let symbol = cell_symbol(layout.text(), item);
                let on_caret = surface.focused
                    && surface.caret >= item.source.start
                    && surface.caret < item.source.end;
                let style = if on_caret {
                    caret_painted = true;
                    caret_span = Some((item.cell, item.width));
                    caret_style
                } else {
                    let selected = surface.selection.as_ref().is_some_and(|range| {
                        item.source.start < range.end && item.source.end > range.start
                    });
                    selection_merge(base, selected, surface.focused, &selection_styles)
                };
                match Cell::with_width(
                    symbol,
                    style.foreground,
                    style.background.unwrap_or(backdrop),
                    style.attributes,
                    item.width,
                ) {
                    Ok(cell) => {
                        columns += cell.width();
                        cells.push(cell);
                    }
                    Err(_) => {
                        let blank = Cell::styled(
                            style.foreground,
                            style.background.unwrap_or(backdrop),
                            style.attributes,
                            " ",
                        )
                        .unwrap_or_else(|_| blank(base));
                        for _ in 0..item.width {
                            cells.push(blank.clone());
                            columns += 1;
                        }
                    }
                }
            }
            let (caret_row, caret_cell, _) = layout.caret(surface.caret);
            caret_row_matches = surface.focused && caret_row == row_index;
            if caret_row_matches && !caret_painted {
                while columns < caret_cell.min(target) {
                    cells.push(blank(base));
                    columns += 1;
                }
                if columns < target
                    && let Ok(cell) = Cell::styled(
                        caret_style.foreground,
                        caret_style.background.unwrap_or(backdrop),
                        caret_style.attributes,
                        " ",
                    )
                {
                    columns += cell.width();
                    cells.push(cell);
                    caret_span = Some((caret_cell, 1));
                }
            }
        }
        while columns < target {
            cells.push(blank(base));
            columns += 1;
        }
        let target = visible.width.max(0) as usize;
        enum Slot {
            Empty,
            Glyph(Cell),
            Continuation,
        }
        let mut grid: Vec<Slot> = (0..target).map(|_| Slot::Empty).collect();
        let mut column = 0usize;
        for cell in cells {
            let width = cell.width();
            let end = column.saturating_add(width);
            for visible_column in column.max(col_start)..end.min(col_end) {
                let at = visible_column - col_start;
                if at >= target {
                    continue;
                }
                let fully_visible = column >= col_start && end <= col_end;
                grid[at] = if fully_visible && visible_column == column {
                    Slot::Glyph(cell.clone())
                } else if fully_visible {
                    Slot::Continuation
                } else {
                    Slot::Glyph(blank(base))
                };
            }
            column = end;
        }
        if caret_row_matches && let Some((cell, width)) = caret_span {
            let end = cell.saturating_add(width.max(1));
            if !(cell >= col_start && end <= col_end) {
                let first = cell.max(col_start);
                if first < end.min(col_end) {
                    let at = first - col_start;
                    if let Some(slot) = grid.get_mut(at) {
                        debug_assert!(
                            !matches!(slot, Slot::Continuation),
                            "a partly visible cell never emits a continuation"
                        );
                        if !matches!(slot, Slot::Continuation) {
                            *slot = Slot::Glyph(blank(caret_style));
                        }
                    }
                }
            }
        }
        let mut visible_row: Vec<Cell> = Vec::with_capacity(target);
        for slot in grid {
            match slot {
                Slot::Glyph(cell) => visible_row.push(cell),
                Slot::Continuation => {}
                Slot::Empty => visible_row.push(blank(base)),
            }
        }
        while visible_row.iter().map(|cell| cell.width()).sum::<usize>() < target {
            visible_row.push(blank(base));
        }
        rows.push(visible_row);
    }
    Image::from_rows(rows).ok()
}
