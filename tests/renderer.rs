// Renderer composition and encoding tests. The damage encoder and its cell
// slot type are crate-internal, so they are reached through the hidden module.
#![allow(clippy::single_range_in_vec_init)] // Damage rows are spans by design.
use icmd::__private::{CellSlot, Surface, encode_diff};
use icmd::advanced::{Frame, ImageId, Operation, Renderer, RendererConfig};
use icmd::{Cell, EmojiMerging, Image, ImageProtocol, ScreenPosition, Size};

// Renderer composition and encoding tests. They live beside the renderer
// module so the production file stays focused on the implementation.

fn slot(symbol: &str) -> CellSlot {
    CellSlot::Lead(Cell::plain(symbol).unwrap())
}

#[test]
fn contiguous_cells_share_one_cursor_move() {
    let old = vec![CellSlot::Lead(Cell::blank()); 4];
    let desired = vec![
        slot("a"),
        slot("b"),
        slot("c"),
        CellSlot::Lead(Cell::blank()),
    ];
    let mut changed = Vec::new();
    let output = encode_diff(
        &old,
        &desired,
        Size::new(4, 1),
        &[vec![0..4]],
        false,
        EmojiMerging::Merge,
        &mut changed,
        &mut 0u64,
    )
    .unwrap()
    .unwrap();
    assert!(output.contains("[1;1Habc"));
    assert!(!output.contains("[1;2H"));
    assert!(!output.contains("[1;3H"));
}

#[test]
fn emoji_sequences_resynchronize_the_next_ansi_run() {
    use unicode_width::UnicodeWidthStr;
    for (name, symbol) in [
        ("zwj", "👩\u{200D}💻"),
        ("family", "👨\u{200D}👩\u{200D}👧\u{200D}👦"),
        ("skin", "👍🏽"),
        ("flag", "🇺🇸"),
        ("keycap", "1\u{FE0F}\u{20E3}"),
        ("vs16", "❤\u{FE0F}"),
    ] {
        let mut desired = vec![CellSlot::Lead(Cell::blank()); 6];
        desired[0] = slot(symbol);
        if symbol.width() == 2 {
            desired[1] = CellSlot::Continuation(Cell::blank());
        }
        desired[2] = slot("x");
        let old = vec![CellSlot::Lead(Cell::blank()); 6];
        let mut changed = Vec::new();
        let output = encode_diff(
            &old,
            &desired,
            Size::new(6, 1),
            &[vec![0..6]],
            false,
            EmojiMerging::Merge,
            &mut changed,
            &mut 0u64,
        )
        .unwrap()
        .unwrap();
        assert!(
            output.contains("[1;3Hx"),
            "{name} must address the following cell explicitly: {output:?}"
        );
    }
}

#[test]
fn plain_wide_glyphs_stay_batched() {
    for symbol in ["👍", "界"] {
        let mut desired = vec![CellSlot::Lead(Cell::blank()); 6];
        desired[0] = slot(symbol);
        desired[1] = CellSlot::Continuation(Cell::blank());
        desired[2] = slot("x");
        desired[3] = slot("y");
        let old = vec![CellSlot::Lead(Cell::blank()); 6];
        let mut changed = Vec::new();
        let output = encode_diff(
            &old,
            &desired,
            Size::new(6, 1),
            &[vec![0..6]],
            false,
            EmojiMerging::Merge,
            &mut changed,
            &mut 0u64,
        )
        .unwrap()
        .unwrap();
        assert!(
            output.contains(&format!("{symbol}xy")),
            "{symbol} must stay in one run: {output:?}"
        );
        assert!(
            !output.contains("[1;3H"),
            "{symbol} must not resynchronize mid-row: {output:?}"
        );
    }
}

#[test]
fn separate_merging_trusts_the_measured_width() {
    let old = vec![CellSlot::Lead(Cell::blank()); 6];
    let mut desired = vec![CellSlot::Lead(Cell::blank()); 6];
    // The shape the commit pass produces under `Separate`: the base keeps
    // the trailing joiner, then the second emoji, then ordinary text.
    desired[0] = slot("👩\u{200D}");
    desired[1] = CellSlot::Continuation(Cell::blank());
    desired[2] = slot("💻");
    desired[3] = CellSlot::Continuation(Cell::blank());
    desired[4] = slot("x");
    let mut changed = Vec::new();
    let output = encode_diff(
        &old,
        &desired,
        Size::new(6, 1),
        &[vec![0..6]],
        false,
        EmojiMerging::Separate,
        &mut changed,
        &mut 0u64,
    )
    .unwrap()
    .unwrap();
    assert!(
        output.contains("👩\u{200D}💻x"),
        "Separate must keep the measured cells in one run: {output:?}"
    );
    assert!(
        !output.contains("[1;3H"),
        "Separate must not resynchronize a sequence: {output:?}"
    );
}

#[test]
fn wide_graphemes_resynchronize_the_next_ansi_run() {
    let old = vec![CellSlot::Lead(Cell::blank()); 4];
    let desired = vec![
        slot("👩‍💻"),
        CellSlot::Continuation(Cell::blank()),
        slot("x"),
        slot("y"),
    ];
    let mut changed = Vec::new();
    let output = encode_diff(
        &old,
        &desired,
        Size::new(4, 1),
        &[vec![0..4]],
        false,
        EmojiMerging::Merge,
        &mut changed,
        &mut 0u64,
    )
    .unwrap()
    .unwrap();
    assert!(output.contains("[1;1H👩‍💻"));
    assert!(output.contains("[1;3Hx"));
    assert!(output.contains("[1;4Hy"));
}

#[test]
fn ordered_composition_preserves_wide_glyph_footprints() {
    let mut renderer = Renderer::with_config(
        Size::new(4, 1),
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            cell_pixel_size: Some(Size::new(8, 16)),
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![
            Operation::Create {
                id: ImageId(1),
                image: Image::from_rows(vec![vec![
                    Cell::plain("界").unwrap(),
                    Cell::plain("x").unwrap(),
                ]])
                .unwrap(),
                position: ScreenPosition::default(),
                level: 0,
            },
            Operation::Create {
                id: ImageId(2),
                image: Image::new(1, 1, Cell::plain("y").unwrap()).unwrap(),
                position: ScreenPosition::new(0, 1),
                level: 1,
            },
        ]))
        .unwrap();
    renderer.render_diff().unwrap();
    assert!(matches!(renderer.last[0], CellSlot::Lead(ref value) if value.symbol() == " "));
    assert!(matches!(renderer.last[1], CellSlot::Lead(ref value) if value.symbol() == "y"));
}

#[test]
fn ordered_composition_matches_scan_reference_for_overlaps_and_clipping() {
    let mut renderer = Renderer::with_config(
        Size::new(12, 6),
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            cell_pixel_size: Some(Size::new(8, 16)),
            ..RendererConfig::default()
        },
    )
    .unwrap();
    let cell = |value: &str| Cell::plain(value).unwrap();
    let rows =
        |width: usize, height: usize, value: &str| Image::new(width, height, cell(value)).unwrap();
    renderer
        .apply_frame(Frame::new(vec![
            Operation::Create {
                id: ImageId(1),
                image: rows(8, 4, "a"),
                position: ScreenPosition::new(0, 1),
                level: 0,
            },
            Operation::Create {
                id: ImageId(2),
                image: rows(7, 3, "b"),
                position: ScreenPosition::new(1, -2),
                level: 2,
            },
            Operation::Create {
                id: ImageId(3),
                image: Image::from_rows(vec![vec![cell("界"), cell("c"), cell("d")]]).unwrap(),
                position: ScreenPosition::new(4, 8),
                level: 1,
            },
        ]))
        .unwrap();
    renderer.render_diff().unwrap();

    let mut expected = vec![CellSlot::Lead(Cell::blank()); 12 * 6];
    let layers = renderer.layers.clone();
    for line in 0..6 {
        for column in 0..12 {
            let mut winner = None;
            for id in &layers {
                let node = &renderer.images[id];
                let local_line = line as i32 - node.position.line;
                let local_column = column as i32 - node.position.column;
                let Surface::Cells(image) = &node.surface else {
                    continue;
                };
                if local_line < 0
                    || local_column < 0
                    || local_line >= image.height() as i32
                    || local_column >= image.width() as i32
                {
                    continue;
                }
                winner = Some((
                    node.level,
                    node.order,
                    node.mutation,
                    id.0,
                    image
                        .cell_at(local_line as usize, local_column as usize)
                        .clone(),
                ));
            }
            expected[line * 12 + column] = winner
                .map(|(_, _, _, _, cell)| cell)
                .unwrap_or_else(|| CellSlot::Lead(Cell::blank()));
        }
    }
    for line in 0..6 {
        for column in 0..12 {
            let index = line * 12 + column;
            let valid = match &expected[index] {
                CellSlot::Lead(cell) if cell.width() == 2 => {
                    column + 1 < 12 && matches!(expected[index + 1], CellSlot::Continuation(_))
                }
                CellSlot::Continuation(_) => {
                    column > 0
                        && matches!(expected[index - 1], CellSlot::Lead(ref cell) if cell.width() == 2)
                }
                _ => true,
            };
            if !valid {
                expected[index] = match &expected[index] {
                    CellSlot::Lead(cell) => CellSlot::Lead(cell.as_blank()),
                    CellSlot::Continuation(cell) => CellSlot::Lead(cell.clone()),
                };
            }
        }
    }
    assert_eq!(renderer.last, expected);
}
