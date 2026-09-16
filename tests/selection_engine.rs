// The selection engine's own rules, driven directly. These are the tests that
// pin the behavior every consumer inherits, so a change here is a change to the
// editor and to every selectable region at once.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use icmd::__private::{
    ClipboardIntent, DocPoint, DocumentBuilder, EditAction, EditModel, EditPolicy, HitBias, Run,
    Selection, SelectionDocument, SelectionMotion, block_separator, clipboard_intent, motion_for,
    value_layout,
};
use icmd::{EmojiMerging, KeyboardEvent};

const MERGE: EmojiMerging = EmojiMerging::Merge;

fn document(value: &str) -> SelectionDocument {
    SelectionDocument::from_value(value.to_string(), MERGE)
}

fn event(code: KeyCode, modifiers: KeyModifiers) -> KeyboardEvent {
    KeyboardEvent {
        key: KeyEvent::new(code, modifiers),
    }
}

// Two leaves, one per painted line, built the way the paint pass builds them.
fn two_runs(first: &str, second: &str) -> SelectionDocument {
    let mut builder = DocumentBuilder::new(MERGE);
    for (value, line) in [(first, 0), (second, 1)] {
        let layout = Arc::new(value_layout(value, MERGE));
        builder.push(Run::new(
            Arc::from(value),
            layout,
            icmd::ScreenPosition::new(line, 0),
            value.len(),
            1,
        ));
    }
    builder.finish()
}

#[test]
fn boundaries_follow_the_grapheme_policy() {
    let doc = document("a界b");
    assert_eq!(doc.boundaries(), &[0, 1, 4, 5]);
    assert_eq!(doc.clamp(2), 1);
    assert_eq!(doc.clamp(3), 4);
    assert_eq!(doc.next_boundary(1), 4);
    assert_eq!(doc.previous_boundary(4), 1);

    // A separate-mode document splits an emoji sequence into its parts, exactly
    // as the editor's own boundaries do.
    let separate =
        SelectionDocument::from_value("👩\u{200D}💻".to_string(), EmojiMerging::Separate);
    assert_eq!(separate.boundaries(), &[0, 7, 11]);
    let merged = SelectionDocument::from_value("👩\u{200D}💻".to_string(), MERGE);
    assert_eq!(merged.boundaries(), &[0, 11]);
}

#[test]
fn clamp_snaps_to_the_nearest_boundary_and_ties_resolve_forward() {
    // "é" is two bytes and one boundary pair, so byte 1 is an exact midpoint.
    let doc = document("é");
    assert_eq!(doc.clamp(0), 0);
    assert_eq!(doc.clamp(1), 2, "a midpoint resolves forward");
    assert_eq!(doc.clamp(2), 2);
    assert_eq!(doc.clamp(99), 2, "past the end clamps to the document end");
}

#[test]
fn char_and_word_motion_walk_the_document() {
    let doc = document("one two");
    let mut selection = Selection::at(0);
    selection.apply(&doc, SelectionMotion::CharRight, false);
    assert_eq!(selection.cursor, 1);
    selection.apply(&doc, SelectionMotion::WordRight, false);
    assert_eq!(
        selection.cursor, 3,
        "a word step lands on the whitespace that follows the word"
    );
    selection.apply(&doc, SelectionMotion::WordRight, false);
    assert_eq!(selection.cursor, 7);
    selection.apply(&doc, SelectionMotion::WordLeft, false);
    assert_eq!(selection.cursor, 4);
    selection.apply(&doc, SelectionMotion::DocStart, false);
    assert_eq!(selection.cursor, 0);
    selection.apply(&doc, SelectionMotion::DocEnd, false);
    assert_eq!(selection.cursor, 7);
    selection.apply(&doc, SelectionMotion::RowStart, false);
    assert_eq!(selection.cursor, 0);
}

#[test]
fn directional_motion_collapses_an_existing_selection_first() {
    let doc = document("hello");
    let mut selection = Selection::between(1, 4);
    // Right collapses to the end of the range, left to its start.
    selection.apply(&doc, SelectionMotion::CharRight, false);
    assert_eq!(selection.range(), (4, 4));
    selection.anchor = 1;
    selection.cursor = 4;
    selection.apply(&doc, SelectionMotion::CharLeft, false);
    assert_eq!(selection.range(), (1, 1));
    // Extending keeps the anchor and moves the active end.
    selection.anchor = 1;
    selection.cursor = 1;
    selection.apply(&doc, SelectionMotion::CharRight, true);
    assert_eq!(selection.range(), (1, 2));
}

#[test]
fn a_document_of_two_runs_is_one_text() {
    let doc = two_runs("hello", "world");
    assert_eq!(doc.len(), 11);
    assert_eq!(doc.boundaries(), &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    assert_eq!(doc.slice(0..11), "hello\nworld");
    assert_eq!(doc.slice(3..8), "lo\nwo");
    assert_eq!(doc.slice(5..6), "\n");
    // A separator is a caret stop in its own right.
    assert_eq!(doc.clamp(6), 6);
    assert_eq!(doc.previous_boundary(6), 5);
    assert_eq!(doc.next_boundary(5), 6);
}

#[test]
fn the_separator_rule_only_joins_blocks() {
    assert_eq!(block_separator(None, 0), 0);
    assert_eq!(
        block_separator(Some(0), 0),
        0,
        "inline siblings share a line"
    );
    assert_eq!(
        block_separator(Some(0), 1),
        1,
        "a later line is a new block"
    );
    assert_eq!(
        block_separator(Some(3), 1),
        0,
        "a nested leaf above is still inline"
    );
}

#[test]
fn a_point_resolves_to_the_cell_it_landed_on() {
    let doc = two_runs("hello", "world");
    let leading = |line, column| doc.hit(DocPoint::Point { line, column }, HitBias::Leading);
    let trailing = |line, column| doc.hit(DocPoint::Point { line, column }, HitBias::Trailing);
    assert_eq!(leading(0, 0), 0);
    assert_eq!(trailing(0, 0), 1);
    assert_eq!(leading(0, 2), 2);
    assert_eq!(leading(1, 4), 10);
    assert_eq!(
        leading(1, 99),
        11,
        "past the row end is the row's last boundary"
    );
    assert_eq!(leading(9, 0), 11, "below every leaf is the document end");
    assert_eq!(leading(-1, 0), 0, "above every leaf is the document start");
}

#[test]
fn drag_keeps_the_pointer_cell_inside_the_selection() {
    let doc = two_runs("hello", "world");
    let point = |line, column| DocPoint::Point { line, column };
    // Dragging right from the cell holding "h" includes every cell up to and
    // including the one under the pointer.
    let mut selection = Selection::at(0);
    selection.drag_to(&doc, point(0, 3), 0);
    assert_eq!(selection.range(), (0, 4), "cells 0..=3");
    // Dragging left keeps the pressed glyph selected too.
    let mut selection = Selection::at(4);
    selection.drag_to(&doc, point(0, 0), 4);
    assert_eq!(selection.range(), (0, 5), "cells 0..=4");
    // A pointer that never leaves its cell selects nothing.
    let mut selection = Selection::at(2);
    selection.drag_to(&doc, point(0, 2), 2);
    assert!(selection.is_collapsed());
    assert_eq!(selection.cursor, 2);
    // A drag into the next run is expressed in document offsets.
    let mut selection = Selection::at(0);
    selection.drag_to(&doc, point(1, 0), 0);
    assert_eq!(selection.range(), (0, 7));
}

#[test]
fn local_range_projects_onto_each_run() {
    let doc = two_runs("hello", "world");
    let mut selection = Selection::at(0);
    selection.drag_to(&doc, DocPoint::Point { line: 1, column: 4 }, 0);
    // The projection is per run, so a painter never has to do the arithmetic.
    assert_eq!(selection.local_range(0, 5), Some(0..5));
    assert_eq!(selection.local_range(6, 5), Some(0..5));
    assert_eq!(selection.local_range(6, 2), Some(0..2));
    let collapsed = Selection::at(2);
    assert_eq!(collapsed.local_range(0, 5), None);
}

#[test]
fn copy_returns_none_when_collapsed() {
    let doc = document("abc");
    assert_eq!(Selection::at(0).text(&doc), None);
    let mut selection = Selection::at(0);
    selection.select_all(&doc);
    assert_eq!(selection.text(&doc).as_deref(), Some("abc"));
}

#[test]
fn keyboard_maps_are_shared() {
    let plain = |code| event(code, KeyModifiers::empty());
    let control = |code| event(code, KeyModifiers::CONTROL);
    let control_shift = |code| event(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    assert_eq!(
        motion_for(&plain(KeyCode::Left)),
        Some(SelectionMotion::CharLeft)
    );
    assert_eq!(
        motion_for(&control(KeyCode::Left)),
        Some(SelectionMotion::WordLeft)
    );
    assert_eq!(
        motion_for(&plain(KeyCode::Home)),
        Some(SelectionMotion::RowStart)
    );
    assert_eq!(
        motion_for(&control(KeyCode::Home)),
        Some(SelectionMotion::DocStart)
    );
    assert_eq!(
        motion_for(&plain(KeyCode::PageDown)),
        Some(SelectionMotion::PageDown)
    );
    assert_eq!(motion_for(&plain(KeyCode::Char('x'))), None);
    // Every clipboard chord is Shift-qualified so the plain chords stay with
    // the terminal and the runtime.
    assert_eq!(
        clipboard_intent(&control_shift(KeyCode::Char('c'))),
        Some(ClipboardIntent::Copy)
    );
    assert_eq!(
        clipboard_intent(&control_shift(KeyCode::Char('X'))),
        Some(ClipboardIntent::Cut)
    );
    assert_eq!(
        clipboard_intent(&control_shift(KeyCode::Char('v'))),
        Some(ClipboardIntent::Paste)
    );
    assert_eq!(
        clipboard_intent(&control(KeyCode::Char('a'))),
        Some(ClipboardIntent::SelectAll)
    );
    // Plain Ctrl+C is the runtime's exit key: no widget may claim it.
    assert_eq!(clipboard_intent(&control(KeyCode::Char('c'))), None);
    assert_eq!(clipboard_intent(&control(KeyCode::Char('v'))), None);
    assert_eq!(clipboard_intent(&control(KeyCode::Char('x'))), None);
    assert_eq!(clipboard_intent(&plain(KeyCode::Char('c'))), None);
}

// The anti-duplication gate: the same script, driven through the editor's model
// and through a region document over the same text, must agree at every step.
// The editor sees one value with a newline; the region sees two leaves. If the
// two consumers ever drift, this test fails.
#[test]
fn editor_and_region_agree_on_one_script() {
    let value = "hello\nworld";
    let mut model = EditModel::default();
    model.render(None, Some(value), true, MERGE);
    let doc = two_runs("hello", "world");
    let mut selection = Selection::at(doc.len());

    // Boundaries and clamping agree over the whole document.
    assert_eq!(model.boundaries(), doc.boundaries());
    for offset in 0..=value.len() + 2 {
        assert_eq!(model.clamp(offset), doc.clamp(offset), "clamp({offset})");
        assert_eq!(
            model.next_boundary(offset),
            doc.next_boundary(offset),
            "next({offset})"
        );
        assert_eq!(
            model.previous_boundary(offset),
            doc.previous_boundary(offset),
            "previous({offset})"
        );
    }

    let policy = EditPolicy {
        multiline: true,
        emoji_merging: MERGE,
        ..EditPolicy::default()
    };
    let script = [
        (
            EditAction::MoveLeft {
                extend: false,
                word: false,
            },
            SelectionMotion::CharLeft,
        ),
        (
            EditAction::MoveRight {
                extend: false,
                word: false,
            },
            SelectionMotion::CharRight,
        ),
        (
            EditAction::MoveLeft {
                extend: false,
                word: true,
            },
            SelectionMotion::WordLeft,
        ),
        (
            EditAction::MoveRight {
                extend: false,
                word: true,
            },
            SelectionMotion::WordRight,
        ),
        (
            EditAction::Home {
                extend: false,
                document: true,
            },
            SelectionMotion::DocStart,
        ),
        (
            EditAction::End {
                extend: false,
                document: true,
            },
            SelectionMotion::DocEnd,
        ),
        (
            EditAction::MoveRight {
                extend: true,
                word: false,
            },
            SelectionMotion::CharRight,
        ),
    ];
    for (action, motion) in script {
        let extend = matches!(
            action,
            EditAction::MoveRight { extend: true, .. } | EditAction::MoveLeft { extend: true, .. }
        );
        model.reduce(action, policy);
        selection.apply(&doc, motion, extend);
        assert_eq!(
            model.caret().anchor,
            selection.anchor,
            "anchor after {motion:?}"
        );
        assert_eq!(
            model.caret().cursor,
            selection.cursor,
            "cursor after {motion:?}"
        );
    }

    // Vertical motion is resolved from the committed row table by the
    // component, not by `reduce`, so both consumers drive the engine's row
    // motion directly with the documents they were given.
    let model_document = SelectionDocument::from_value(value.to_string(), MERGE);
    for direction in [-1, 1, 1, -1] {
        model.vertical_move(&model_document, direction, 1, true, policy);
        selection.move_vertical(&doc, direction, 1, true);
        assert_eq!(
            model.caret().cursor,
            selection.cursor,
            "row motion {direction} agrees"
        );
        assert_eq!(model.caret().anchor, selection.anchor);
    }

    // And the clipboard text they produce is the same string.
    model.reduce(EditAction::SelectAll, policy);
    selection.select_all(&doc);
    assert_eq!(model.selected_text(), selection.text(&doc));
    assert_eq!(model.selected_text().as_deref(), Some("hello\nworld"));
}
