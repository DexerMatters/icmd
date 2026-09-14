// Text-layout behavior, parity, property, and mode-agreement tests. The
// layout internals are reached through the hidden module.
#![allow(unused_imports)]

use icmd::__private::*;
use std::ops::Range;

use icmd::{EmojiMerging, Span, Text, TextAlign, TextOverflow, TextWrap};

pub(crate) fn layout_for_test(value: &str, wrap: TextWrap, width: usize) -> TextLayout {
    layout_text(
        &Text::new(value).wrap(wrap),
        width,
        ComputedText::default(),
        EmojiMerging::Merge,
        |parent, _| parent,
        |style| *style,
    )
}

pub(crate) fn layout_for_test_with(
    value: &str,
    wrap: TextWrap,
    width: usize,
    merging: EmojiMerging,
) -> TextLayout {
    layout_text(
        &Text::new(value).wrap(wrap),
        width,
        ComputedText::default(),
        merging,
        |parent, _| parent,
        |style| *style,
    )
}

// Test-only surface for the layout's index queries. The layout itself is
// crate-private, so integration tests need a narrow, documented view.

// Behavior tests for the layout's rows, wrapping, and queries.
mod behavior {
    use super::*;

    fn build(value: &str, wrap: TextWrap, width: usize) -> TextLayout {
        layout_text(
            &Text::new(value).wrap(wrap),
            width,
            ComputedText::default(),
            EmojiMerging::Merge,
            |parent, _| parent,
            |style| *style,
        )
    }

    fn rows(layout: &TextLayout) -> Vec<String> {
        layout
            .items()
            .iter()
            .map(|item| {
                if item.kind == ItemKind::Newline {
                    "\n".to_string()
                } else {
                    item.symbol(layout.text()).to_string()
                }
            })
            .collect()
    }

    fn row_strings(layout: &TextLayout) -> Vec<String> {
        (0..layout.row_count())
            .map(|index| {
                layout
                    .row_items(index)
                    .iter()
                    .filter(|item| item.kind == ItemKind::Glyph)
                    .map(|item| item.symbol(layout.text()))
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn empty_input_has_one_empty_row() {
        let layout = build("", TextWrap::Soft, 4);
        assert_eq!(layout.row_count(), 1);
        assert_eq!(layout.width(), 0);
        assert!(layout.row_items(0).is_empty());
    }

    #[test]
    fn items_partition_the_source_except_line_breaks() {
        for value in [
            "abc",
            "hello world",
            "a\n\nb",
            "你好世界",
            "a\tb",
            "e\u{301}x",
        ] {
            for wrap in [TextWrap::NoWrap, TextWrap::Soft, TextWrap::Hard] {
                for width in [1usize, 4, 80] {
                    let layout = build(value, wrap, width);
                    // Items are strictly ordered and never overlap, and the
                    // only bytes they may skip are explicit newlines, which are
                    // unowned line breaks rather than painted glyphs.
                    let mut cursor = 0usize;
                    for item in layout.items() {
                        assert!(
                            item.source.start >= cursor,
                            "overlap in {value:?} {wrap:?} {width}"
                        );
                        let skipped = &value[cursor..item.source.start];
                        assert!(
                            skipped.chars().all(|ch| ch == '\n'),
                            "unowned bytes {skipped:?} in {value:?} {wrap:?} {width}"
                        );
                        cursor = item.source.end;
                    }
                    assert!(
                        value[cursor..].chars().all(|ch| ch == '\n'),
                        "unowned tail in {value:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn rows_tile_the_source() {
        for value in ["abc", "hello world", "a\n\nb", "a\n", "\n\n", "你好 世界"] {
            for wrap in [TextWrap::NoWrap, TextWrap::Soft, TextWrap::Hard] {
                for width in [1usize, 3, 5, 80] {
                    let layout = build(value, wrap, width);
                    let mut cursor = 0usize;
                    for index in 0..layout.row_count() {
                        let start = layout.row_start(index);
                        let end = layout.row_source_end(index);
                        assert!(
                            start >= cursor,
                            "row {index} overlaps for {value:?} {wrap:?} {width}"
                        );
                        cursor = end;
                    }
                    assert_eq!(cursor, value.len(), "rows must cover {value:?}");
                }
            }
        }
    }

    #[test]
    fn every_row_positions_its_items_in_cell_order() {
        for value in ["abc", "hello world", "你好 世界", "e\u{301}x"] {
            let layout = build(value, TextWrap::Soft, 3);
            for index in 0..layout.row_count() {
                let mut cell = 0usize;
                let mut painted = 0usize;
                for item in layout.row_items(index) {
                    assert_eq!(item.cell, cell, "item cell order in row {index}");
                    cell += item.width;
                    if item.kind != ItemKind::Separator {
                        painted += item.width;
                    }
                }
                assert_eq!(layout.row_width(index), painted);
            }
        }
    }

    #[test]
    fn soft_wraps_at_line_break_opportunities() {
        let layout = build("hello world", TextWrap::Soft, 5);
        assert_eq!(row_strings(&layout), ["hello", "world"]);
    }

    #[test]
    fn soft_splits_overlong_fragments() {
        let layout = build("abcdef", TextWrap::Soft, 3);
        assert_eq!(row_strings(&layout), ["abc", "def"]);
    }

    #[test]
    fn hard_wraps_at_the_cell_limit() {
        let layout = build("ab cd", TextWrap::Hard, 4);
        assert_eq!(row_strings(&layout), ["ab c", "d"]);
        let layout = build("ab cd", TextWrap::Soft, 4);
        assert_eq!(row_strings(&layout), ["ab", "cd"]);
    }

    #[test]
    fn dropped_separator_stays_navigable() {
        let layout = build("hello world", TextWrap::Soft, 5);
        // The space lives on the first row as a separator and is not painted.
        let separators: Vec<&Item> = layout
            .items()
            .iter()
            .filter(|item| item.kind == ItemKind::Separator)
            .collect();
        assert_eq!(separators.len(), 1);
        assert_eq!(&layout.text()[separators[0].text.clone()], " ");
    }

    #[test]
    fn explicit_empty_and_trailing_lines_are_retained() {
        let layout = build("a\n\n", TextWrap::Soft, 3);
        assert_eq!(layout.row_count(), 3);
        assert_eq!(row_strings(&layout), ["a", "", ""]);
    }

    #[test]
    fn hit_test_round_trips_every_boundary() {
        for value in ["abc", "hello world", "你好世界", "e\u{301}x"] {
            let layout = build(value, TextWrap::Hard, 4);
            let mut boundary = 0usize;
            loop {
                assert_eq!(layout.clamp(boundary), boundary, "{value:?} at {boundary}");
                if boundary >= value.len() {
                    break;
                }
                boundary = layout.next_boundary(boundary);
            }
        }
    }

    #[test]
    fn vertical_movement_keeps_a_preferred_column() {
        // Two rows of the same 6-cell width; moving down from column 5 lands on
        // the short middle row's final boundary, and carrying that preferred
        // column forward returns to column 5 on the third row.
        let layout = build("abcdef\nxy\nabcdef", TextWrap::Soft, 3);
        assert_eq!(row_strings(&layout), ["abc", "def", "xy", "abc", "def"]);
        assert_eq!(layout.row_of_source(5), 1);
        assert_eq!(layout.column(5), 2);
        let (moved, preferred) = layout.vertical(5, 1, None);
        // Row 1 is "def" (source 6..9). Column 2 lands after "de" at source 9.
        assert_eq!(moved, 9);
        assert_eq!(preferred, Some(2));
        // Carrying the preferred column forward reaches column 2 of row 2.
        let (moved, _) = layout.vertical(moved, 1, preferred);
        assert_eq!(layout.column(moved), 2);
    }

    #[test]
    fn separators_inside_a_row_are_painted() {
        let layout = build("ab cd efgh", TextWrap::Soft, 6);
        let painted: Vec<String> = (0..layout.row_count())
            .map(|index| {
                layout
                    .row_items(index)
                    .iter()
                    .filter(|item| item.kind != ItemKind::Separator)
                    .map(|item| item.symbol(layout.text()).to_string())
                    .collect()
            })
            .collect();
        assert_eq!(
            painted,
            ["ab cd", "efgh"],
            "the space inside the first row is painted; only the trailing one \
             that pushed 'efgh' onto the next row is dropped"
        );
        // The caret table and the painted cells agree: 'c' is at cell 3 because
        // the space before it occupies cell 2.
        assert_eq!(layout.caret(3).1, 3, "'c' follows the painted space");
        assert_eq!(layout.hit(0, 2, HitBias::Leading), 2);
    }

    #[test]
    fn zero_width_viewport_is_clamped() {
        let layout = build("界", TextWrap::Soft, 0);
        assert!(layout.row_count() >= 1);
        let layout = build("界", TextWrap::Hard, 0);
        assert!(layout.row_count() >= 1);
    }

    #[test]
    fn every_item_has_a_row() {
        let layout = build("a\n\nbc", TextWrap::Soft, 2);
        for item in layout.items() {
            assert!(item.row < layout.row_count());
        }
        let _ = rows(&layout);
    }
}

mod parity {
    pub(crate) fn normalized(value: &str) -> String {
        value
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
            .collect()
    }
}

mod property_tests {
    use super::parity::normalized;
    use super::*;
    #[allow(unused_imports)]
    use crate::TextWrap as W;

    fn corpus() -> Vec<&'static str> {
        vec![
            "",
            "a",
            "abc",
            "abcdef",
            "hello world",
            "a   ",
            "     ",
            "ab cd",
            "a\n",
            "a\n\n",
            "a\n\nb",
            "\n",
            "\n\n",
            "line one\nline two",
            "line one\n\nline three",
            "你好世界",
            "你好 世界",
            "a\u{301}b界",
            "👩‍💻x",
            "a\tb",
            "\t\tx",
            "a\tb\nc\td",
            "Long Unicode text: 这是一个很长的示例文本，含有 emoji 👩‍💻 and an unbreakable-token-for-hard-wrap.",
            "  leading",
            "trailing  ",
            "word ",
            " \n ",
            "a\r\nb",
            "a\rb",
            "e\u{301}\u{301}f",
            "🇺🇸🇨🇳",
            "👍🏽ok",
            // Emoji sequences whose clusters merge at runtime.
            "👩\u{200D}💻",
            "👨\u{200D}👩\u{200D}👧\u{200D}👦",
            "👩\u{200D}",
            "\u{200D}💻",
            "a\u{200D}b",
            "\u{200D}",
            "\u{FE0F}",
            "\u{301}",
            "1\u{FE0F}\u{20E3}",
            "❤\u{FE0F}",
            "❤",
            "👍👍",
            "🇺🇸🇨🇳🇯🇵",
            "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
        ]
    }

    const WRAPS: [W; 3] = [W::NoWrap, W::Soft, W::Hard];
    const WIDTHS: [usize; 10] = [1, 2, 3, 4, 5, 6, 8, 12, 24, 80];

    #[test]
    fn every_returned_position_is_a_grapheme_boundary() {
        use unicode_segmentation::UnicodeSegmentation;
        for value in corpus() {
            let value = normalized(value);
            let boundaries: std::collections::HashSet<usize> = std::iter::once(0)
                .chain(
                    value
                        .grapheme_indices(true)
                        .map(|(index, grapheme)| index + grapheme.len()),
                )
                .collect();
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test(&value, wrap, width);
                    let mut position = 0usize;
                    let mut guard = 0usize;
                    loop {
                        assert!(
                            boundaries.contains(&position),
                            "{position} is not a boundary of {value:?}"
                        );
                        if position >= value.len() {
                            break;
                        }
                        let next = layout.next_boundary(position);
                        assert!(
                            next > position,
                            "next_boundary({position}) must advance for {value:?} \
                             {wrap:?} {width}, returned {next}"
                        );
                        position = next;
                        guard += 1;
                        assert!(guard <= value.len() + 2, "walk must terminate");
                    }
                    for index in 0..layout.row_count() {
                        assert!(boundaries.contains(&layout.row_start(index)));
                        assert!(boundaries.contains(&layout.row_end(index)));
                        assert!(boundaries.contains(&layout.row_source_end(index)));
                    }
                }
            }
        }
    }

    #[test]
    fn hit_test_round_trips_caret_positions() {
        for value in corpus() {
            let value = normalized(value);
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test(&value, wrap, width);
                    let mut position = 0usize;
                    loop {
                        let (row, cell, caret_width) = layout.caret(position);
                        let hit = layout.hit(row, cell, HitBias::Leading);
                        // A caret past the last painted cell of its row shares
                        // that cell with the following row's first boundary, and
                        // a wide grapheme's trailing edge shares its second cell.
                        // Those positions are visually ambiguous by definition;
                        // the plan only requires the unambiguous ones to round
                        // trip.
                        let past_last_cell = cell >= layout.row_width(row);
                        let ambiguous = past_last_cell || (caret_width > 1 && cell > 0);
                        assert!(
                            hit == position || ambiguous,
                            "round trip failed for {value:?} {wrap:?} {width}: \
                             position={position} row={row} cell={cell} hit={hit}"
                        );
                        if position >= value.len() {
                            break;
                        }
                        position = layout.next_boundary(position);
                    }
                }
            }
        }
    }

    #[test]
    fn every_displayed_cell_is_reachable() {
        for value in corpus() {
            let value = normalized(value);
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test(&value, wrap, width);
                    for index in 0..layout.row_count() {
                        let cells = layout.row_width(index);
                        for cell in 0..cells {
                            let hit = layout.hit(index, cell, HitBias::Trailing);
                            // A hit resolves to a boundary; that boundary's
                            // caret belongs to this row, except at the very end
                            // of a row, where the caret continues on the next
                            // one. Both are correct: what must never happen is a
                            // hit landing on an unrelated earlier row.
                            let (row, _, _) = layout.caret(hit);
                            assert!(
                                row == index || row == index + 1,
                                "cell {cell} of row {index} resolved to row {row} \
                                 for {value:?} {wrap:?} {width}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn measure_and_paint_agree_on_extents() {
        for value in corpus() {
            let value = normalized(value);
            for wrap in WRAPS {
                for width in [0usize, 1, 2, 3, 4, 5, 8, 24] {
                    let layout = layout_for_test(&value, wrap, width);
                    assert!(layout.row_count() >= 1, "a layout always has one row");
                    let widest = (0..layout.row_count())
                        .map(|index| layout.row_width(index))
                        .max()
                        .unwrap_or(0);
                    assert_eq!(layout.max_row_width(), widest);
                    assert_eq!(layout.width(), widest);
                }
            }
        }
    }

    #[test]
    fn wrap_modes_preserve_source_and_respect_the_viewport() {
        for value in corpus() {
            let value = normalized(value);
            for wrap in WRAPS {
                for width in [1usize, 3, 5, 12, 80] {
                    let layout = layout_for_test(&value, wrap, width);
                    let mut cursor = 0usize;
                    for index in 0..layout.row_count() {
                        let start = layout.row_start(index);
                        let end = layout.row_source_end(index);
                        assert!(start >= cursor, "rows must not overlap");
                        cursor = end;
                        // A wrapped row fits its viewport. Two documented
                        // exceptions: a grapheme wider than the viewport cannot
                        // be split, and `NoWrap` deliberately overflows so a
                        // scroll host can pan across the logical line.
                        if wrap != W::NoWrap {
                            let unbreakable = layout
                                .row_items(index)
                                .iter()
                                .any(|item| item.kind == ItemKind::Glyph && item.width > width);
                            assert!(
                                unbreakable || layout.row_width(index) <= width,
                                "row {index} is {} cells wide in a {width}-cell box \
                                 for {value:?} {wrap:?}",
                                layout.row_width(index)
                            );
                        }
                    }
                    assert_eq!(cursor, value.len(), "rows must cover {value:?}");
                }
            }
        }
    }

    #[test]
    fn wide_graphemes_in_a_one_cell_viewport_are_safe() {
        // The wide grapheme overflows its one-cell viewport, but it still owns
        // exactly one row and never produces a half continuation cell.
        let layout = layout_for_test("界a", W::Hard, 1);
        let row = layout.row_of_source(0);
        assert_eq!(
            layout.row_items(row)[0].source,
            0..3,
            "the wide grapheme starts its row"
        );
        assert_eq!(layout.row_items(row)[0].width, 2);
        assert!(
            layout.row_count() >= 2,
            "the following glyph needs its own row"
        );
        // The leading boundary sits at the start of the wide grapheme's row;
        // its trailing boundary begins the following row, which is what lets a
        // caret step past a glyph the viewport cannot show in full.
        assert_eq!(layout.row_of_source(0), row);
        // The caret at the grapheme's leading boundary starts its row at cell
        // zero, and its trailing boundary starts the next row.
        assert_eq!(
            layout.caret(0),
            (row, 0, 2),
            "the caret precedes both cells"
        );
        assert_eq!(
            layout.caret(3).0,
            row + 1,
            "the caret moves to the next row"
        );
    }

    #[test]
    fn wide_glyphs_are_indivisible() {
        for value in ["界a", "你好", "👍🏽ok"] {
            for wrap in WRAPS {
                let layout = layout_for_test(value, wrap, 4);
                for item in layout.items() {
                    if item.kind != ItemKind::Glyph || item.width < 2 {
                        continue;
                    }
                    let graphemes: Vec<Range<usize>> = {
                        use unicode_segmentation::UnicodeSegmentation;
                        value
                            .grapheme_indices(true)
                            .map(|(index, grapheme)| index..index + grapheme.len())
                            .collect()
                    };
                    assert!(
                        graphemes.contains(&item.source),
                        "a wide item must span exactly one grapheme: {:?}",
                        item.source
                    );
                }
            }
        }
    }

    #[test]
    fn merging_sequences_are_single_graphemes() {
        use unicode_segmentation::UnicodeSegmentation;
        use unicode_width::UnicodeWidthStr;
        for value in [
            "👩\u{200D}💻",
            "👨\u{200D}👩\u{200D}👧\u{200D}👦",
            "👩\u{200D}",
            "👍🏽",
            "🇺🇸",
            "1\u{FE0F}\u{20E3}",
            "❤\u{FE0F}",
            "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
        ] {
            assert_eq!(value.graphemes(true).count(), 1, "{value:?} is one cluster");
            let measured = UnicodeWidthStr::width(value);
            assert!(
                (1..=2).contains(&measured),
                "{value:?} measures {measured} cells"
            );
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test(value, wrap, width);
                    assert_eq!(
                        layout.items().len(),
                        1,
                        "{value:?} {wrap:?} {width} must shape one item"
                    );
                    let item = &layout.items()[0];
                    assert_eq!(
                        item.source,
                        0..value.len(),
                        "{value:?} {wrap:?} {width} owns its whole source range"
                    );
                    assert_eq!(
                        item.width, measured,
                        "{value:?} {wrap:?} {width} keeps its measured width"
                    );
                }
            }
        }
    }

    fn separate_corpus() -> Vec<(&'static str, usize)> {
        vec![
            ("👩\u{200D}💻", 4),
            ("👨\u{200D}👩\u{200D}👧\u{200D}👦", 8),
            ("👍🏽", 4),
            ("🇺🇸", 2),
            ("1\u{FE0F}\u{20E3}", 1),
            ("❤\u{FE0F}", 1),
            ("a👩\u{200D}💻b", 6),
            ("x👍🏽y", 6),
        ]
    }

    #[test]
    fn separate_merging_measures_each_codepoint() {
        for (value, expected) in separate_corpus() {
            for wrap in WRAPS {
                for width in WIDTHS {
                    let layout = layout_for_test_with(value, wrap, width, EmojiMerging::Separate);
                    // Wrapping only moves whole groups between rows, so the
                    // cells painted across the value are the same at every
                    // width.
                    let painted: usize = layout
                        .items()
                        .iter()
                        .filter(|item| item.kind == ItemKind::Glyph)
                        .map(|item| item.width)
                        .sum();
                    assert_eq!(
                        painted, expected,
                        "{value:?} {wrap:?} {width} must paint {expected} cells"
                    );
                    // No grapheme is ever painted in two rows: every item that
                    // shares a source range shares a row.
                    let mut ranges: Vec<Range<usize>> = layout
                        .items()
                        .iter()
                        .map(|item| item.source.clone())
                        .collect();
                    ranges.sort_by_key(|range| (range.start, range.end));
                    ranges.dedup();
                    for range in ranges {
                        let rows: Vec<usize> = layout
                            .items()
                            .iter()
                            .filter(|item| item.source == range)
                            .map(|item| item.row)
                            .collect();
                        assert!(
                            rows.windows(2).all(|pair| pair[0] == pair[1]),
                            "a grapheme split across rows for {value:?} {wrap:?} {width}"
                        );
                    }
                    // Every part still resolves to the grapheme's own
                    // boundaries, so navigation steps over the sequence whole.
                    assert_eq!(layout.clamp(value.len()), value.len());
                }
            }
        }
    }

    #[test]
    fn separate_merging_tiles_the_source_without_splitting_a_unit() {
        let sequence = "👩\u{200D}💻";
        let value = format!("ab{sequence}cd");
        for wrap in [W::Soft, W::Hard] {
            for width in [1usize, 2, 3, 4, 6] {
                let layout = layout_for_test_with(&value, wrap, width, EmojiMerging::Separate);
                let mut cursor = 0usize;
                for index in 0..layout.row_count() {
                    assert_eq!(
                        layout.row_start(index),
                        cursor,
                        "{wrap:?} {width} row start"
                    );
                    cursor = layout.row_source_end(index);
                }
                assert_eq!(cursor, value.len(), "{wrap:?} {width} rows cover the value");
                // The sequence shapes two units, each two cells, and the value
                // keeps its six cells in total.
                let parts: Vec<_> = layout
                    .items()
                    .iter()
                    .filter(|item| item.source.start >= 2 && item.source.end <= 2 + sequence.len())
                    .collect();
                assert_eq!(parts.len(), 2, "{wrap:?} {width}: two parts");
                assert!(
                    parts.iter().all(|item| item.width == 2),
                    "{wrap:?} {width}: each part is two cells"
                );
            }
        }
    }

    #[test]
    fn separate_merging_refines_the_merged_boundaries() {
        for (value, _) in separate_corpus() {
            for wrap in WRAPS {
                let merged = layout_for_test_with(value, wrap, 4, EmojiMerging::Merge);
                let separate = layout_for_test_with(value, wrap, 4, EmojiMerging::Separate);
                assert_eq!(merged.source_len(), separate.source_len(), "{value:?}");
                for layout in [&merged, &separate] {
                    let mut cursor = 0usize;
                    for index in 0..layout.row_count() {
                        assert_eq!(layout.row_start(index), cursor, "{value:?} row start");
                        cursor = layout.row_source_end(index);
                    }
                    assert_eq!(cursor, value.len(), "{value:?} rows must cover the value");
                }
                // Every merged boundary is a separate boundary.
                let mut merged_walk = 0usize;
                loop {
                    let mut candidate = 0usize;
                    let mut found = false;
                    loop {
                        if candidate == merged_walk {
                            found = true;
                            break;
                        }
                        if candidate >= value.len() {
                            break;
                        }
                        candidate = separate.next_boundary(candidate);
                    }
                    assert!(found, "{value:?}: {merged_walk} is not a separate boundary");
                    if merged_walk >= value.len() {
                        break;
                    }
                    merged_walk = merged.next_boundary(merged_walk);
                }
            }
        }
    }

    #[test]
    fn vertical_movement_keeps_a_preferred_column() {
        let layout = layout_for_test("abcdef\nxy\nabcdef", W::Soft, 3);
        assert_eq!(layout.row_count(), 5);
        // Source 5 is the last cell of row 1 ("def"); its caret column is 2.
        assert_eq!(layout.caret(5).1, 2);
        // Moving down lands at column 2 of row 2 ("xy"), whose content is only
        // two cells, so that is the row's own end.
        let (first, preferred) = layout.vertical(5, 1, None);
        assert_eq!(preferred, Some(2));
        assert_eq!(first, 9, "column 2 of the two-cell row is its end");
        assert_eq!(layout.row_of_source(first), 2);

        // Repeating the preferred column from there reaches row 3 unchanged.
        let (second, _) = layout.vertical(first, 1, preferred);
        assert_eq!(layout.row_of_source(second), 3);
        assert_eq!(layout.column(second), 2, "the preferred column is kept");

        // Moving back up returns to row 2's column 2 and then row 1's.
        let (back, _) = layout.vertical(second, -1, preferred);
        assert_eq!(layout.row_of_source(back), 2);
    }

    #[test]
    fn multi_span_source_ranges_are_global() {
        let text = Text::from_spans([Span::new("ab"), Span::new("cd"), Span::new("ef")]);
        let layout = layout_text(
            &text,
            80,
            ComputedText::default(),
            EmojiMerging::Merge,
            |parent, _| parent,
            |style| *style,
        );
        let ranges: Vec<Range<usize>> = layout.items().iter().map(|i| i.source.clone()).collect();
        assert_eq!(
            ranges,
            vec![0..1, 1..2, 2..3, 3..4, 4..5, 5..6],
            "spans must not restart source offsets"
        );
        assert_eq!(layout.source_len(), 6);
        // Every returned boundary is still valid and the row owns the range.
        assert_eq!(layout.row_source_end(0), 6);
        assert_eq!(layout.caret(4).1, 4);
    }

    #[test]
    fn painted_columns_match_item_cells_across_separators() {
        for (value, wrap, width) in [
            ("ab cd efgh", TextWrap::Soft, 6usize),
            ("hello world again", TextWrap::Soft, 8),
            ("a b c d e f g h", TextWrap::Soft, 5),
        ] {
            let layout = super::layout_for_test(value, wrap, width);
            for index in 0..layout.row_count() {
                let mut column = 0usize;
                for item in layout.row_items(index) {
                    assert_eq!(
                        item.cell,
                        column,
                        "{value:?} row {index}: item {:?} reports cell {} but the \
                         painter reaches it at column {column}",
                        item.symbol(layout.text()),
                        item.cell
                    );
                    // Every item advances, separators included: they own cells
                    // even though they are not painted.
                    column = column.saturating_add(item.width);
                }
                // The painted width is what remains after separators, but the
                // final column still counts them.
                let painted: usize = layout
                    .row_items(index)
                    .iter()
                    .filter(|item| item.kind != ItemKind::Separator)
                    .map(|item| item.width)
                    .sum();
                assert!(
                    column >= painted,
                    "the full column extent includes separators"
                );
            }
        }
    }

    #[test]
    fn debug_wide_rows() {
        for width in [4usize, 5] {
            let layout = super::layout_for_test("你好世界", TextWrap::Soft, width);
            eprintln!("width {width}: {} rows", layout.row_count());
            for index in 0..layout.row_count() {
                eprintln!(
                    "  row {index} w={} items={:?}",
                    layout.row_width(index),
                    layout
                        .row_items(index)
                        .iter()
                        .map(|i| (i.symbol(layout.text()).to_string(), i.cell, i.width))
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn hits_are_monotonic_within_a_row() {
        for (value, wrap, width) in [
            ("ab cd efgh", TextWrap::Soft, 6usize),
            ("hello world again", TextWrap::Soft, 8),
            ("a b c d e f", TextWrap::Soft, 4),
        ] {
            let layout = super::layout_for_test(value, wrap, width);
            for index in 0..layout.row_count() {
                // Probe one cell past the painted content to cover a trailing
                // dropped separator.
                let cells = layout.row_width(index) + 2;
                let mut previous = None;
                for cell in 0..cells {
                    let hit = layout.hit(index, cell, HitBias::Leading);
                    if let Some(previous) = previous {
                        assert!(
                            hit >= previous,
                            "{value:?} row {index}: cell {cell} resolved backwards \
                             ({hit} after {previous})"
                        );
                    }
                    previous = Some(hit);
                }
            }
        }
    }

    #[test]
    fn zero_width_geometry_is_clamped() {
        for value in corpus() {
            for wrap in WRAPS {
                let layout = layout_for_test(value, wrap, 0);
                assert!(layout.row_count() >= 1);
                assert!(layout.row_count() <= value.chars().count().max(1) * 2 + 2);
            }
        }
    }
}

mod mode_agreement_tests {
    use super::*;

    fn layout_for(value: &str, wrap: TextWrap, width: usize) -> TextLayout {
        super::layout_for_test(value, wrap, width)
    }

    #[test]
    fn wrap_policies_have_consistent_row_counts() {
        for value in ["hello world", "abcdefghij", "abc def", "a b c d e f"] {
            let no_wrap = layout_for(value, TextWrap::NoWrap, 4);
            let soft = layout_for(value, TextWrap::Soft, 4);
            let hard = layout_for(value, TextWrap::Hard, 4);
            assert_eq!(no_wrap.row_count(), 1, "NoWrap keeps one logical line");
            // Hard wrapping fills every cell, so it never needs more rows than
            // soft wrapping, which leaves a row early to keep words whole.
            assert!(
                hard.row_count() <= soft.row_count(),
                "{value:?}: hard wrapping packs more per row than soft ({} vs {})",
                hard.row_count(),
                soft.row_count()
            );
            for layout in [&soft, &hard] {
                for index in 0..layout.row_count() {
                    assert!(
                        layout.row_width(index) <= 4,
                        "{value:?}: a wrapped row must fit the viewport"
                    );
                }
            }
            // Soft keeps whole words together where they fit.
            if value.contains(' ') {
                let rows: Vec<String> = (0..soft.row_count())
                    .map(|index| {
                        soft.row_items(index)
                            .iter()
                            .map(|item| item.symbol(soft.text()).to_string())
                            .collect()
                    })
                    .collect();
                assert!(
                    rows.iter().all(|row| !row.starts_with(' ')),
                    "{value:?}: a soft-wrapped row does not start with a space: {rows:?}"
                );
            }
        }
    }

    #[test]
    fn navigable_ranges_survive_every_policy() {
        for value in ["hello world", "a\n\nb", "a\n", "  leading", "trailing  "] {
            for wrap in [TextWrap::NoWrap, TextWrap::Soft, TextWrap::Hard] {
                let layout = layout_for(value, wrap, 3);
                let mut position = 0usize;
                let mut guard = 0;
                let mut visited = vec![position];
                while position < value.len() {
                    position = layout.next_boundary(position);
                    visited.push(position);
                    guard += 1;
                    assert!(guard <= value.len() + 2, "the walk must terminate");
                }
                assert_eq!(position, value.len());
                // Every byte of the value is either owned by a row or is a
                // newline that a row terminates.
                let mut cursor = 0usize;
                for index in 0..layout.row_count() {
                    let end = layout.row_source_end(index);
                    assert!(end >= cursor, "rows must not overlap");
                    cursor = end;
                }
                assert_eq!(cursor, value.len(), "rows must cover {value:?}");
                assert!(
                    visited.len() >= value.len(),
                    "each byte advances the boundary walk"
                );
            }
        }
    }
}
