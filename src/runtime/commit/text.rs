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
pub fn layout(
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
pub struct CachedNatural {
    pub text: Box<Text>,
    pub inherited: ComputedText,
    pub merging: EmojiMerging,
    pub layout: Arc<TextLayout>,
}

pub type NaturalCache = std::cell::RefCell<std::collections::HashMap<DomId, CachedNatural>>;

// Two texts produce the same natural layout when the fields the layout reads are
// equal. The editor surface and test probe do not participate.
fn same_layout_inputs(left: &Text, right: &Text) -> bool {
    left.spans == right.spans
        && left.style == right.style
        && left.wrap == right.wrap
        && left.align == right.align
        && left.overflow == right.overflow
}

pub fn text_measure(
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
pub fn cell_symbol(text: &str, item: &text_layout::Item) -> String {
    let symbol = item.symbol(text);
    if symbol == "\t" {
        " ".repeat(item.width)
    } else {
        symbol.to_string()
    }
}

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
