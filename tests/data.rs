// Cell and cell-surface tests. The surface geometry helpers are crate-internal,
// so they are reached through the hidden module rather than the public API.
use icmd::__private::CellSlot;
use icmd::{Cell, CellError, Image, ImageError, Rect};

mod tests {
    use super::*;

    fn cell(symbol: &str) -> Cell {
        Cell::plain(symbol).unwrap()
    }

    #[test]
    fn diff_patch_rebuilds_a_sparse_cell_change() {
        let old = Image::from_rows(vec![vec![cell("a"), cell("b"), cell("c")]]).unwrap();
        let next = Image::from_rows(vec![vec![cell("a"), cell("x"), cell("c")]]).unwrap();
        let (rect, rows) = old.diff_patch_rect(&next).expect("sparse change patches");
        assert_eq!(rect, Rect::new(0, 1, 1, 1));
        let mut patched = old.clone();
        patched.patch_rect(rect, &rows).unwrap();
        assert_eq!(patched, next);
    }

    #[test]
    fn diff_patch_expands_for_wide_glyph_boundaries() {
        let old = Image::from_rows(vec![vec![cell("a"), cell("界"), cell("c")]]).unwrap();
        let next =
            Image::from_rows(vec![vec![cell("a"), cell("x"), Cell::blank(), cell("c")]]).unwrap();
        let (rect, rows) = old.diff_patch_rect(&next).expect("sparse change patches");
        assert_eq!(rect, Rect::new(0, 1, 2, 1));
        let mut patched = old.clone();
        patched.patch_rect(rect, &rows).unwrap();
        assert_eq!(patched, next);
    }

    #[test]
    fn patch_rect_clears_wide_neighbors_per_row() {
        let old = Image::from_rows(vec![
            vec![
                cell("a"),
                cell("界"),
                cell("b"),
                cell("c"),
                cell("d"),
                cell("e"),
            ],
            vec![
                cell("a"),
                cell("b"),
                cell("c"),
                cell("d"),
                cell("界"),
                cell("e"),
            ],
        ])
        .unwrap();
        let rows = vec![
            vec![cell("x"), cell("x"), cell("x"), cell("x")],
            vec![cell("y"), cell("y"), cell("y"), cell("y")],
        ];
        let mut patched = old.clone();
        patched.patch_rect(Rect::new(0, 1, 4, 2), &rows).unwrap();
        assert_eq!(patched.cell_at(0, 5).cell().symbol(), "d");
        assert_eq!(patched.cell_at(1, 6).cell().symbol(), "e");
    }
}
