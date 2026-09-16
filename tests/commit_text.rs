// Text measurement tests. The measure path runs inside the commit stage, so its
// entry points are reached through the hidden module.
use crossterm::style::{Attributes, Color};
use unicode_segmentation::UnicodeSegmentation;

use icmd::__private::{
    ComputedText, EditorSurface, Item, ItemKind, RectI, TextLayout, cell_symbol, editor_raster,
    layout, layout_for, surface_layout, text_measure,
};
use icmd::{EmojiMerging, Text, TextWrap};

fn inherited() -> ComputedText {
    ComputedText {
        foreground: Color::Reset,
        background: None,
        attributes: Attributes::default(),
    }
}

fn strings(value: &str, wrap: icmd::TextWrap, width: usize) -> Vec<String> {
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
    assert_eq!(strings("abcdef", icmd::TextWrap::Soft, 3), ["abc", "def"]);
}

#[test]
fn soft_uses_unicode_break_opportunities() {
    assert_eq!(
        strings("hello world", icmd::TextWrap::Soft, 5),
        ["hello", "world"]
    );
}

#[test]
fn soft_wraps_cjk_at_unicode_breaks() {
    assert_eq!(
        strings("你好世界", icmd::TextWrap::Soft, 4),
        ["你好", "世界"]
    );
}

#[test]
fn trailing_spaces_are_not_lost() {
    assert_eq!(strings("a   ", icmd::TextWrap::Soft, 4), ["a   "]);
}

#[test]
fn hard_wraps_trailing_spaces_instead_of_overflowing() {
    assert_eq!(strings("     ", icmd::TextWrap::Hard, 2), ["  ", "  ", " "]);
}

#[test]
fn zero_and_one_cell_widths_do_not_panic() {
    assert!(!strings("界", icmd::TextWrap::Soft, 0).is_empty());
    assert!(!strings("界", icmd::TextWrap::Hard, 1).is_empty());
}

#[test]
fn hard_splits_only_between_graphemes() {
    assert_eq!(
        strings("a\u{301}b界", icmd::TextWrap::Hard, 2),
        ["a\u{301}b", "界"]
    );
}

#[test]
fn hard_wraps_at_the_cell_limit_instead_of_an_earlier_word_break() {
    assert_eq!(strings("ab cd", icmd::TextWrap::Hard, 4), ["ab c", "d"]);
    assert_eq!(strings("ab cd", icmd::TextWrap::Soft, 4), ["ab", "cd"]);
}

#[test]
fn explicit_empty_and_trailing_lines_are_retained() {
    assert_eq!(strings("a\n\n", icmd::TextWrap::Soft, 3), ["a", "", ""]);
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
        &Text::new("abcdef").wrap(icmd::TextWrap::Soft),
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
        placeholder_style: icmd::TextStyle::default(),
        selection_style: icmd::TextStyle::default(),
        selection_inactive_style: icmd::TextStyle::default(),
        caret_style: icmd::TextStyle::default(),
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
                    icmd::__private::CellSlot::Continuation(_) => " ".to_string(),
                    icmd::__private::CellSlot::Lead(cell) => cell.symbol().to_string(),
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
                    icmd::__private::CellSlot::Continuation(_) => " ".to_string(),
                    icmd::__private::CellSlot::Lead(cell) => cell.symbol().to_string(),
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
    let mut leaders: Vec<Option<&Item>> = vec![None; document];
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
                    let layout =
                        surface_layout(&surface, document, TextWrap::NoWrap, EmojiMerging::Merge);
                    for (index, painted) in
                        editor_window_rows(&surface, document, start, viewport, layout.row_count())
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

fn focused_surface(value: &str, placeholder: &str, wrap: TextWrap, caret: usize) -> EditorSurface {
    let mut surface = editor_surface(value, placeholder, wrap);
    surface.focused = true;
    surface.caret = caret;
    surface.caret_style = icmd::TextStyle::default().background(Color::Magenta);
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
