use crossterm::style::Color;

use crate::basic::editor_surface::{CommittedLayout, EditorSurface};
use crate::basic::text_layout::{self, ItemKind, TextLayout};
use std::sync::Arc;

use crate::{Cell, DomId, EmojiMerging, Image, Text, TextAlign, TextOverflow, TextWrap};

use super::geometry::RectI;
use super::style::slot;
use super::types::ComputedText;

pub(super) fn merge_text(parent: ComputedText, style: &crate::TextStyle) -> ComputedText {
    ComputedText {
        foreground: slot(&style.foreground).unwrap_or(parent.foreground),
        background: slot(&style.background).or(parent.background),
        attributes: style.attr.resolve(parent.attributes),
    }
}

// Shaping cost: `text_measure` below shapes a leaf once per frame unless the
// offer is narrower than the natural width of wrapping text, where it needs a
// second pass to learn the wrapped row count. `NoWrap` text and any offer at or
// above the natural width reuse the natural layout, so they stay at one shape.
// Removing the remaining second pass needs a frame-local shaping cache keyed by
// content, width, wrap, merging, and computed style — note that the natural
// layout is width-independent, so caching just that pass would suffice.
// `text_shaping_per_frame_is_bounded_by_the_leaf_count` and
// `a_no_wrap_leaf_is_shaped_once_per_frame` in `tests/limits.rs` pin the bounds.
pub(super) fn layout(
    text: &Text,
    width: usize,
    inherited: ComputedText,
    merging: EmojiMerging,
) -> TextLayout {
    let base = merge_text(inherited, &text.style);
    text_layout::layout_text(text, width, base, merging, merge_text, |computed| *computed)
}

// The natural (unwrapped) layout is width-independent, so it is worth keeping
// between frames. An entry is validated by comparing the inputs that determine
// it, which is why no hash is involved and a stale hit is impossible.
#[derive(Clone)]
pub(super) struct CachedNatural {
    pub(super) text: Box<Text>,
    pub(super) inherited: ComputedText,
    pub(super) merging: EmojiMerging,
    pub(super) layout: Arc<TextLayout>,
}

pub(super) type NaturalCache = std::cell::RefCell<std::collections::HashMap<DomId, CachedNatural>>;

// Two texts produce the same natural layout when the fields the layout reads are
// equal. The editor surface and test probe do not participate.
fn same_layout_inputs(left: &Text, right: &Text) -> bool {
    left.spans == right.spans
        && left.style == right.style
        && left.wrap == right.wrap
        && left.align == right.align
        && left.overflow == right.overflow
}

pub(super) fn text_measure(
    id: Option<DomId>,
    cache: Option<&NaturalCache>,
    text: &Text,
    offered_width: Option<i32>,
    offered_height: Option<i32>,
    inherited: ComputedText,
    merging: EmojiMerging,
) -> (i32, i32) {
    // The natural layout is width-independent and reused across frames when the
    // inputs are unchanged; validation is by value, so a stale hit is
    // impossible. Falling back to a fresh layout keeps every caller correct.
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
    // The intrinsic width is the widest unwrapped logical line. `NoWrap` at a
    // large width breaks only at explicit newlines, so its widest row is
    // exactly that intrinsic width.
    let natural_width = natural.max_row_width() as i32;
    let width = offered_width.map_or(natural_width, |value| natural_width.min(value.max(0)));
    // Wrapping at or above the natural width cannot break any row, so the
    // natural layout already answers the row count. Only a narrower offer
    // needs a second shaping pass, which is what keeps an unconstrained leaf
    // to one shape instead of two per frame.
    let offered = offered_width.unwrap_or(natural_width.max(1)).max(1) as usize;
    // `NoWrap` breaks rows only at explicit newlines, so no offer can change the
    // row count and the second pass is never needed. Otherwise only an offer
    // narrower than the natural width can introduce a break.
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
    // An empty control sizes to its placeholder so the hint is visible and the
    // box does not collapse to zero cells. An empty placeholder still occupies
    // one cell, which is the caret.
    if surface.value.is_empty() {
        // An empty control sizes to its placeholder so the hint is visible and
        // the box does not collapse to zero cells. An empty placeholder still
        // occupies one cell, which is the caret. The placeholder is shaped by
        // the same engine as the value, so tabs and wide graphemes measure
        // identically in both.
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
    // The document height is whatever the value wraps to. A parent that grants
    // a shorter box clips and scrolls it (that is the scroll host's job); a
    // parent that leaves the height auto gets the whole document, so multiline
    // content is visible without scrolling.
    let rows = |width: i32| {
        surface_layout(surface, width.max(1) as usize, wrap, merging).row_count() as i32
    };
    match wrap {
        // An unwrapped editor reports the intrinsic width of its widest logical
        // line so a scroll host can pan across it.
        TextWrap::NoWrap => {
            let _ = offered_height;
            (natural.max_row_width() as i32, natural.row_count() as i32)
        }
        // A wrapped editor reports the offered content width; with no offer yet
        // it reports its natural width and the parent's offer then drives the
        // wrap.
        TextWrap::Soft | TextWrap::Hard => match offered_width.filter(|width| *width > 0) {
            Some(width) => (width, rows(width)),
            None => (natural.max_row_width() as i32, natural.row_count() as i32),
        },
    }
}

fn layout_for(value: &str, width: usize, wrap: TextWrap, merging: EmojiMerging) -> TextLayout {
    text_layout::layout_text(
        &Text::new(value).wrap(wrap),
        width.max(1),
        ComputedText::default(),
        merging,
        |parent, _| parent,
        |style| *style,
    )
}

pub(super) fn surface_layout(
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
    let visible = rect.intersection(visible)?;
    if rect.width <= 0 || rect.height <= 0 {
        return None;
    }
    let text_style = merge_text(inherited, &text.style);
    let layout = layout(text, rect.width as usize, inherited, merging);
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
    blank: &impl Fn(ComputedText) -> Cell,
) -> Vec<Cell> {
    let visible_start = visible_start.min(rect_width);
    let visible_end = visible_end.min(rect_width).max(visible_start);
    let ellipsis_start = rect_width.saturating_sub(1);
    let content_end = if ellipsis { ellipsis_start } else { rect_width };
    // Only the visible window is indexed, so a long offscreen line costs
    // O(visible width) memory rather than O(document width). Items that start
    // before the window are not recorded: the original indexed them at their own
    // start cell, which the visible loop could never reach.
    let window = visible_end.saturating_sub(visible_start);
    let mut glyphs_at: Vec<Option<&text_layout::Item>> = vec![None; window];
    let mut visual = offset.min(rect_width);
    for item in items {
        if item.kind == ItemKind::Newline || item.width == 0 {
            continue;
        }
        // Nothing later can start inside the window.
        if visual >= visible_end {
            break;
        }
        let glyph_end = visual.saturating_add(item.width);
        // A separator occupies cells but is deliberately not painted; the cell
        // is left blank. Recording it here would draw the whitespace that the
        // wrap decision removed, which the editor surface never draws.
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
            && let Ok(cell) = Cell::with_width(
                cell_symbol(layout_text, item),
                item.style.foreground,
                item.style.background.unwrap_or(backdrop),
                item.style.attributes,
                item.width,
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

// The rendered symbol for an item: a tab becomes its measured blank run, and
// every other item is the exact slice of the layout's normalized text.
fn cell_symbol(text: &str, item: &text_layout::Item) -> String {
    let symbol = item.symbol(text);
    if symbol == "\t" {
        " ".repeat(item.width)
    } else {
        symbol.to_string()
    }
}

fn editor_raster(
    text: &Text,
    surface: &EditorSurface,
    rect: RectI,
    visible: RectI,
    backdrop: Color,
    scroll: (i32, i32),
    merging: EmojiMerging,
) -> Option<Image> {
    let wrap = text.wrap;
    // An empty control paints its placeholder through the same canonical
    // engine as a value, so tab stops and wide graphemes agree.
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
        // The offset the frame is really painted with: the scroll containers
        // around this surface shifted it by the values the runtime *applied*,
        // which are the component's request clamped to the current extent.
        // Publishing the request instead would disagree with the pixels as soon
        // as a resize shrinks the extent, and the first pointer interaction
        // after that resize would land in the wrong column. `visible.width` and
        // `visible.height` stay the clip the scroll host can paint, which may be
        // smaller than the surface's document box.
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
    // The scroll host already shifts this surface's painted rectangle by the
    // applied offset, so `visible` is expressed in document coordinates and the
    // first visible row is read directly from it.
    let row_start = (visible.line - rect.line) as usize;
    let row_end = row_start + visible.height.max(0) as usize;
    let col_start = (visible.column - rect.column).max(0) as usize;
    let col_end = col_start + visible.width as usize;
    let mut rows = Vec::with_capacity(visible.height as usize);
    for row_index in row_start..row_end {
        let items: &[text_layout::Item] = layout.row_items(row_index);
        // Build the row as one entry per grapheme. `Image::from_rows` supplies
        // the continuation slot for a wide cell, so a row's cell columns are the
        // running sum of its entries' widths.
        let mut cells: Vec<Cell> = Vec::with_capacity(rect.width as usize);
        let mut columns = 0usize;
        let target = rect.width.max(0) as usize;
        // Where the caret style was applied, in document columns. The grid below
        // rewrites a multi-cell glyph the window clips into a plain blank, which
        // would drop the style; this span lets the windowing pass tell whether
        // the user can actually see a caret and repaint it at the visible part
        // of its span if not.
        let mut caret_span: Option<(usize, usize)> = None;
        let caret_row_matches;
        if showing_placeholder {
            // A focused empty control draws its caret on the placeholder's own
            // insertion point, which is the first cell of its first row.
            let caret_here = surface.focused && row_index == 0;
            caret_row_matches = caret_here;
            let mut caret_painted = false;
            for item in items {
                if item.width == 0 {
                    continue;
                }
                // An item that does not fit inside the box is clipped, and every
                // later item sits further right, so none of them fit either.
                // Breaking here keeps the items before it in their canonical
                // columns; skipping just this item would leave the running column
                // count short, so everything after an over-wide tab or grapheme
                // would slide left into the wrong cell.
                if item.cell + item.width > target {
                    break;
                }
                // A clipped item's cells stay blank rather than closing the gap.
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
                        // A tab expands to several cells, which no single cell
                        // can carry, so it becomes one styled blank per expanded
                        // column. Dropping its cells would paint every later
                        // placeholder glyph too far left.
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
            // The first placeholder item was clipped by the box, so its cell is
            // a blank. The caret must still be visible there instead of
            // disappearing with the hint it sat on.
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
                // Clipped at the box edge: see the placeholder path above. The
                // item and everything after it stay blank instead of shifting
                // left, so painted columns keep matching `item.cell`.
                if item.cell + item.width > target {
                    break;
                }
                while columns < item.cell {
                    cells.push(blank(base));
                    columns += 1;
                }
                // A separator owns cells but is not painted. A caret inside its
                // run still belongs to this row, so it is drawn on the first of
                // the separator's cells rather than lost or pushed to the row's
                // end.
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
                        // A tab expands to several cells, which no single cell
                        // can carry, so it becomes one styled blank per expanded
                        // column. Pushing a single blank would leave every later
                        // glyph on the row painted too far left.
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
            // A caret that is not sitting on a painted grapheme of this row is
            // drawn as a reverse blank in the caret's own cell. That covers the
            // end of a non-final line, a soft-wrap boundary, and a caret on a
            // grapheme the box clipped: the caret keeps the column the canonical
            // layout gives it, and it must stay visible even when the grapheme
            // under it did not fit. A caret that did land on a painted grapheme
            // was already reversed above, so painting a second marker would show
            // two carets.
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
        // Pad to the viewport so every row has the same cell width.
        while columns < target {
            cells.push(blank(base));
            columns += 1;
        }
        // Place each cell into the visible grid by cell column. A wide cell
        // that only partially overlaps the visible span contributes a blank for
        // the part that is visible, so the cells after it keep the columns the
        // caret and hit tables give them, and every row ends up the same width.
        let target = visible.width.max(0) as usize;
        // One slot per visible cell column. A wide cell fills its own slot with
        // the glyph and the next with a continuation marker, so the row's total
        // measured width equals the viewport.
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
                // A glyph is emitted only when EVERY one of its columns is
                // visible and it fits the grid, so its continuation is always
                // inside the row. Any partially visible cell - clipped at the
                // left edge, the right edge, or a multi-cell grapheme wider than
                // the viewport - contributes blanks instead. That keeps each
                // row's measured width equal to the viewport, which
                // `Image::from_rows` requires, and keeps later cells in the
                // columns the caret and hit tables assign them.
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
        // A caret whose span the window only partly shows lost its style in the
        // rewrite above: the glyph became a plain blank. The caret is a
        // coordinate, so it must stay visible on the part of its span the window
        // does show. A span that is wholly visible needs nothing here, because
        // the glyph or the fallback blank already carried the style, and
        // repainting would show two carets. Replacing a slot keeps the row width
        // unchanged: a partially visible cell only ever fills its slots with
        // single-cell blanks, and an empty slot would have been padded anyway.
        if caret_row_matches && let Some((cell, width)) = caret_span {
            let end = cell.saturating_add(width.max(1));
            if !(cell >= col_start && end <= col_end) {
                let first = cell.max(col_start);
                if first < end.min(col_end) {
                    let at = first - col_start;
                    if let Some(slot) = grid.get_mut(at) {
                        // The column belongs to the caret's own cell, which is
                        // only partly visible and so filled its slots with
                        // single-cell blanks. Guard anyway: replacing a
                        // continuation slot would make the row wider than the
                        // viewport, which `from_rows` rejects.
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
        // A glyph's continuation is not a separate cell entry: `from_rows`
        // creates it from the glyph's width. Emitting one blank per slot would
        // double-count the wide grapheme's second column.
        let mut visible_row: Vec<Cell> = Vec::with_capacity(target);
        for slot in grid {
            match slot {
                Slot::Glyph(cell) => visible_row.push(cell),
                Slot::Continuation => {}
                Slot::Empty => visible_row.push(blank(base)),
            }
        }
        // Pad any trailing columns the grid did not fill.
        while visible_row.iter().map(|cell| cell.width()).sum::<usize>() < target {
            visible_row.push(blank(base));
        }
        rows.push(visible_row);
    }
    Image::from_rows(rows).ok()
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

    fn strings(value: &str, wrap: crate::TextWrap, width: usize) -> Vec<String> {
        let layout = layout(
            &Text::new(value).wrap(wrap),
            width,
            inherited(),
            EmojiMerging::Merge,
        );
        (0..layout.row_count())
            .map(|index| {
                layout
                    .row_items(index)
                    .iter()
                    .filter(|item| item.kind != ItemKind::Separator)
                    .map(|item| cell_symbol(layout.text(), item))
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
        let (width, height) = text_measure(
            None,
            None,
            &Text::new("ab\ncdef"),
            None,
            None,
            inherited(),
            EmojiMerging::Merge,
        );
        assert_eq!((width, height), (4, 2));
        // Without a wrap policy the text stays one row and is clipped to the
        // offered width; with soft wrapping it grows to two rows.
        let (width, height) = text_measure(
            None,
            None,
            &Text::new("abcdef"),
            Some(3),
            None,
            inherited(),
            EmojiMerging::Merge,
        );
        assert_eq!((width, height), (3, 1));
        let (width, height) = text_measure(
            None,
            None,
            &Text::new("abcdef").wrap(crate::TextWrap::Soft),
            Some(3),
            None,
            inherited(),
            EmojiMerging::Merge,
        );
        assert_eq!((width, height), (3, 2));
    }

    #[test]
    fn measure_counts_graphemes_not_bytes() {
        let value = "e\u{301}x";
        assert_eq!(value.graphemes(true).count(), 2);
        let (width, height) = text_measure(
            None,
            None,
            &Text::new(value),
            None,
            None,
            inherited(),
            EmojiMerging::Merge,
        );
        assert_eq!((width, height), (2, 1));
    }

    #[test]
    fn emoji_merging_mode_changes_a_sequence_measurement() {
        for (value, merged, separate) in [
            ("👩\u{200D}💻", 2, 4),
            ("👍🏽", 2, 4),
            ("🇺🇸", 2, 2),
            ("❤\u{FE0F}", 2, 1),
        ] {
            let (width, height) = text_measure(
                None,
                None,
                &Text::new(value),
                None,
                None,
                inherited(),
                EmojiMerging::Merge,
            );
            assert_eq!((width, height), (merged, 1), "{value:?} merged");
            let (width, height) = text_measure(
                None,
                None,
                &Text::new(value),
                None,
                None,
                inherited(),
                EmojiMerging::Separate,
            );
            assert_eq!((width, height), (separate, 1), "{value:?} separate");
        }
    }

    fn editor_surface(value: &str, placeholder: &str, wrap: TextWrap) -> EditorSurface {
        EditorSurface {
            value: value.into(),
            selection: None,
            caret: 0,
            focused: false,
            placeholder: placeholder.into(),
            wrap,
            placeholder_style: crate::TextStyle::default(),
            selection_style: crate::TextStyle::default(),
            selection_inactive_style: crate::TextStyle::default(),
            caret_style: crate::TextStyle::default(),
            scroll_x: 0,
            scroll_y: 0,
        }
    }

    fn editor_rows(surface: &EditorSurface, width: usize, height: usize) -> Vec<String> {
        let text = Text::new("").wrap(surface.wrap);
        let rect = RectI::new(0, 0, width as i32, height as i32);
        let image = editor_raster(
            &text,
            surface,
            rect,
            rect,
            Color::Reset,
            (0, 0),
            EmojiMerging::Merge,
        )
        .expect("raster");
        (0..image.height())
            .map(|row| {
                (0..image.width())
                    .map(|column| match image.cell_at(row, column) {
                        crate::data::CellSlot::Continuation(_) => " ".to_string(),
                        crate::data::CellSlot::Lead(cell) => cell.symbol().to_string(),
                    })
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn a_clipped_item_leaves_its_cells_blank_instead_of_shifting_later_items() {
        // Every painted glyph must sit in the cell the canonical layout gives it,
        // even when an earlier item does not fit in the box. Skipping such an
        // item used to leave the running column count short, so everything after
        // it slid left.
        //
        // "a\tb" in a two-cell box: the tab covers cells 1..4 and is clipped, so
        // 'b' belongs at cell 4 and must not appear at column 1.
        let surface = editor_surface("", "a\tb", TextWrap::Soft);
        assert_eq!(editor_rows(&surface, 2, 1), ["a "]);
        // A wide grapheme that does not fit leaves its columns blank too.
        let surface = editor_surface("", "a界a", TextWrap::Soft);
        assert_eq!(editor_rows(&surface, 2, 1), ["a "]);
        // The value path shares the rule, so the clipped value matches what an
        // ordinary `Text` of the same width paints.
        let surface = editor_surface("ab界c", "", TextWrap::NoWrap);
        assert_eq!(editor_rows(&surface, 3, 1), ["ab "]);
    }

    fn clipped_row(layout: &TextLayout, row: usize, width: usize) -> String {
        let mut slots = vec![" ".to_string(); width];
        for item in layout.row_items(row) {
            if item.width == 0 || item.kind == ItemKind::Separator {
                continue;
            }
            if item.cell + item.width > width {
                continue;
            }
            if item.is_tab(layout.text()) {
                // A tab is wider than one cell, so it paints one blank per
                // expanded column rather than a single symbol.
                for offset in 0..item.width {
                    if let Some(slot) = slots.get_mut(item.cell + offset) {
                        *slot = " ".to_string();
                    }
                }
                continue;
            }
            if let Some(slot) = slots.get_mut(item.cell) {
                *slot = item.symbol(layout.text()).to_string();
            }
            for offset in 1..item.width {
                if let Some(slot) = slots.get_mut(item.cell + offset) {
                    *slot = " ".to_string();
                }
            }
        }
        slots.concat()
    }

    fn editor_window_rows(
        surface: &EditorSurface,
        document: usize,
        start: usize,
        viewport: usize,
        height: usize,
    ) -> Vec<String> {
        let text = Text::new("").wrap(surface.wrap);
        let rect = RectI::new(0, 0, document as i32, height as i32);
        let visible = RectI::new(0, start as i32, viewport as i32, height as i32);
        let image = editor_raster(
            &text,
            surface,
            rect,
            visible,
            Color::Reset,
            (0, start as i32),
            EmojiMerging::Merge,
        )
        .expect("raster");
        (0..image.height())
            .map(|row| {
                (0..image.width())
                    .map(|column| match image.cell_at(row, column) {
                        crate::data::CellSlot::Continuation(_) => " ".to_string(),
                        crate::data::CellSlot::Lead(cell) => cell.symbol().to_string(),
                    })
                    .collect::<String>()
            })
            .collect()
    }

    fn clipped_window(
        layout: &TextLayout,
        row: usize,
        document: usize,
        start: usize,
        viewport: usize,
    ) -> String {
        let mut leaders: Vec<Option<&text_layout::Item>> = vec![None; document];
        for item in layout.row_items(row) {
            if item.width == 0 || item.kind == ItemKind::Separator {
                continue;
            }
            if item.cell + item.width > document {
                continue;
            }
            if let Some(slot) = leaders.get_mut(item.cell) {
                *slot = Some(item);
            }
        }
        (start..start + viewport)
            .map(
                |column| match leaders.get(column).and_then(|item| item.as_ref()) {
                    Some(item) if column + item.width <= start + viewport => {
                        if item.is_tab(layout.text()) {
                            // A tab paints one blank per expanded column, and the
                            // columns after its leading one are blank either way.
                            " ".to_string()
                        } else {
                            item.symbol(layout.text()).to_string()
                        }
                    }
                    _ => " ".to_string(),
                },
            )
            .collect()
    }

    #[test]
    fn every_painted_editor_cell_matches_the_canonical_layout() {
        // The strongest form of the column contract: whatever the layout says a
        // row's cells are, that is exactly what the editor paints, for every
        // value, wrap mode, and narrowly granted width.
        let values = [
            "a\tb",
            "a界a",
            "ab界c",
            "界界",
            "abcdef",
            "a b",
            "",
            "\t",
            "界",
            "a\u{301}b",
        ];
        for wrap in [TextWrap::NoWrap, TextWrap::Soft, TextWrap::Hard] {
            for value in values {
                for width in 1..8usize {
                    let surface = editor_surface(value, "", wrap);
                    let layout = surface_layout(&surface, width, wrap, EmojiMerging::Merge);
                    for (index, painted) in editor_rows(&surface, width, layout.row_count())
                        .iter()
                        .enumerate()
                    {
                        assert_eq!(
                            *painted,
                            clipped_row(&layout, index, width),
                            "value={value:?} wrap={wrap:?} width={width} row={index}"
                        );
                    }
                }
            }
        }
        // The placeholder is shaped by the same engine and painted by the same
        // column rule, so it gets the same guarantee.
        for placeholder in ["a\tb", "a界a", "界界", "ab"] {
            for width in 1..8usize {
                let surface = editor_surface("", placeholder, TextWrap::Soft);
                let layout = layout_for(placeholder, width, TextWrap::NoWrap, EmojiMerging::Merge);
                for (index, painted) in editor_rows(&surface, width, layout.row_count())
                    .iter()
                    .enumerate()
                {
                    assert_eq!(
                        *painted,
                        clipped_row(&layout, index, width),
                        "placeholder={placeholder:?} width={width} row={index}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_scrolled_editor_window_keeps_the_canonical_columns() {
        // Scrolling must move the window, not the coordinate space: every
        // painted column still resolves to the document cell the layout assigns
        // it, and a grapheme straddling either window edge is blank instead of
        // sliding its neighbours.
        for value in ["a界bcdef", "界界界", "a\tbcdef", "ab界"] {
            for document in [2usize, 3, 4, 6, 12] {
                let last_start = 6.min(document);
                for start in 0..last_start {
                    let max_viewport = (document - start).min(4);
                    for viewport in 1..=max_viewport {
                        let mut surface = editor_surface(value, "", TextWrap::NoWrap);
                        surface.scroll_x = start;
                        let layout = surface_layout(
                            &surface,
                            document,
                            TextWrap::NoWrap,
                            EmojiMerging::Merge,
                        );
                        for (index, painted) in editor_window_rows(
                            &surface,
                            document,
                            start,
                            viewport,
                            layout.row_count(),
                        )
                        .iter()
                        .enumerate()
                        {
                            assert_eq!(
                                *painted,
                                clipped_window(&layout, index, document, start, viewport),
                                "value={value:?} document={document} start={start} \
                                 viewport={viewport} row={index}"
                            );
                        }
                    }
                }
            }
        }
    }

    fn caret_cells(surface: &EditorSurface, width: usize, height: usize) -> Vec<(usize, usize)> {
        let text = Text::new("").wrap(surface.wrap);
        let rect = RectI::new(0, 0, width as i32, height as i32);
        let image = editor_raster(
            &text,
            surface,
            rect,
            rect,
            Color::Reset,
            (0, 0),
            EmojiMerging::Merge,
        )
        .expect("raster");
        let mut found = Vec::new();
        for row in 0..image.height() {
            for column in 0..image.width() {
                if image.cell_at(row, column).cell().background() == Color::Magenta {
                    found.push((row, column));
                }
            }
        }
        found
    }

    fn focused_surface(
        value: &str,
        placeholder: &str,
        wrap: TextWrap,
        caret: usize,
    ) -> EditorSurface {
        let mut surface = editor_surface(value, placeholder, wrap);
        surface.focused = true;
        surface.caret = caret;
        surface.caret_style = crate::TextStyle::default().background(Color::Magenta);
        surface
    }

    #[test]
    fn a_caret_on_a_clipped_grapheme_is_still_painted() {
        // A tab or wide grapheme that does not fit in the box leaves its cells
        // blank, but the caret the canonical layout places on it must stay
        // visible in the column the layout assigns it.
        let cases: [(&str, TextWrap, usize, usize, usize); 3] = [
            // value, wrap, box width, caret offset, expected caret row
            ("a\tb", TextWrap::Soft, 2, 1, 1),
            ("界", TextWrap::Soft, 1, 0, 0),
            ("\t", TextWrap::Soft, 2, 0, 0),
        ];
        for (value, wrap, width, offset, row) in cases {
            let surface = focused_surface(value, "", wrap, offset);
            let layout = surface_layout(&surface, width, wrap, EmojiMerging::Merge);
            let (caret_row, caret_cell, _) = layout.caret(offset);
            assert_eq!(caret_row, row, "value={value:?} offset={offset}");
            assert!(
                caret_cell < width,
                "the caret must be inside the box for this case: {value:?}"
            );
            assert_eq!(
                caret_cells(&surface, width, layout.row_count()),
                [(row, caret_cell)],
                "value={value:?} width={width}: the caret must be painted at the \
                 column the layout gives it, even when the grapheme under it was \
                 clipped"
            );
        }
    }

    #[test]
    fn an_empty_control_keeps_its_caret_when_the_placeholder_is_clipped() {
        // A tab placeholder in a two-cell box paints blanks, and the focused
        // empty control must still show its caret on that first cell. A wide
        // grapheme that fits is reversed across both of its cells.
        for (placeholder, expected) in [
            ("\t", vec![(0, 0)]),
            ("界", vec![(0, 0), (0, 1)]),
            ("a\tb", vec![(0, 0)]),
        ] {
            let surface = focused_surface("", placeholder, TextWrap::Soft, 0);
            assert_eq!(
                caret_cells(&surface, 2, 1),
                expected,
                "placeholder={placeholder:?}: the caret must stay on the cell the \
                 layout gives it"
            );
        }
    }

    #[test]
    fn the_painted_caret_covers_exactly_the_layouts_caret_span() {
        // The caret is a coordinate too: for every offset the painted caret must
        // start in the cell the canonical layout reports and cover exactly the
        // span that layout gives it, and it must not appear at all when its cell
        // is outside the box.
        let values = ["a\tb", "ab界c", "界界", "abc", "a\nb", "", "\t"];
        for wrap in [TextWrap::NoWrap, TextWrap::Soft, TextWrap::Hard] {
            for value in values {
                for width in 1..7usize {
                    let layout = surface_layout(
                        &editor_surface(value, "", wrap),
                        width,
                        wrap,
                        EmojiMerging::Merge,
                    );
                    for offset in 0..=value.len() {
                        if !value.is_char_boundary(offset) {
                            continue;
                        }
                        let surface = focused_surface(value, "", wrap, offset);
                        let (row, cell, caret_width) = layout.caret(offset);
                        let painted = caret_cells(&surface, width, layout.row_count());
                        let expected: Vec<(usize, usize)> = if cell >= width {
                            Vec::new()
                        } else if cell + caret_width <= width {
                            (cell..cell + caret_width)
                                .map(|column| (row, column))
                                .collect()
                        } else {
                            // A caret whose span the box clips keeps its leading
                            // cell and drops the rest.
                            vec![(row, cell)]
                        };
                        assert_eq!(
                            painted, expected,
                            "value={value:?} wrap={wrap:?} width={width} offset={offset}"
                        );
                    }
                }
            }
        }
    }

    fn window_caret_cells(
        surface: &EditorSurface,
        document: usize,
        start: usize,
        viewport: usize,
        height: usize,
    ) -> Vec<(usize, usize)> {
        let text = Text::new("").wrap(surface.wrap);
        let rect = RectI::new(0, 0, document as i32, height as i32);
        let visible = RectI::new(0, start as i32, viewport as i32, height as i32);
        let image = editor_raster(
            &text,
            surface,
            rect,
            visible,
            Color::Reset,
            (0, start as i32),
            EmojiMerging::Merge,
        )
        .expect("raster");
        let mut found = Vec::new();
        for row in 0..image.height() {
            for column in 0..image.width() {
                if image.cell_at(row, column).cell().background() == Color::Magenta {
                    found.push((row, column));
                }
            }
        }
        found
    }

    #[test]
    fn a_caret_windowed_by_the_viewport_stays_visible() {
        // The surface's own box can be wider than the window the scroll host
        // shows. A caret on a wide grapheme straddling a window edge must be
        // painted on the part of its span the window shows, instead of vanishing
        // with the blank that replaced the clipped glyph.
        for value in ["a界b", "界界", "x界y界z"] {
            for wrap in [TextWrap::NoWrap, TextWrap::Soft, TextWrap::Hard] {
                for document in 2..8usize {
                    let layout = surface_layout(
                        &editor_surface(value, "", wrap),
                        document,
                        wrap,
                        EmojiMerging::Merge,
                    );
                    for offset in 0..=value.len() {
                        if !value.is_char_boundary(offset) {
                            continue;
                        }
                        let (row, cell, width) = layout.caret(offset);
                        let end = cell + width;
                        for viewport in 1..=document {
                            for start in 0..=(document - viewport) {
                                let surface = focused_surface(value, "", wrap, offset);
                                let painted = window_caret_cells(
                                    &surface,
                                    document,
                                    start,
                                    viewport,
                                    layout.row_count(),
                                );
                                let first = cell.max(start);
                                let last = end.min(start + viewport);
                                let expected: Vec<(usize, usize)> = if first >= last {
                                    Vec::new()
                                } else if cell >= start && end <= start + viewport {
                                    (first..last).map(|column| (row, column - start)).collect()
                                } else {
                                    // Only the leading visible column survives the
                                    // clipping.
                                    vec![(row, first - start)]
                                };
                                assert_eq!(
                                    painted, expected,
                                    "value={value:?} wrap={wrap:?} document={document} \
                                     start={start} viewport={viewport} offset={offset}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
