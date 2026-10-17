//! Dot-level braille canvas.

use icmd::CanvasContext;

fn symbol(canvas: &CanvasContext, x: i32, y: i32) -> &str {
    canvas.cell(x, y).expect("cell exists").symbol()
}

#[test]
fn every_braille_dot_maps_to_its_unicode_bit() {
    let cases = [
        (0, 0, '\u{2801}'),
        (0, 1, '\u{2802}'),
        (0, 2, '\u{2804}'),
        (0, 3, '\u{2840}'),
        (1, 0, '\u{2808}'),
        (1, 1, '\u{2810}'),
        (1, 2, '\u{2820}'),
        (1, 3, '\u{2880}'),
    ];
    for (x, y, expected) in cases {
        let mut canvas = CanvasContext::new(1, 1).expect("canvas");
        canvas.set_dot(x, y).expect("dot");
        assert_eq!(symbol(&canvas, 0, 0), expected.to_string());
    }
}

#[test]
fn dots_compose_clear_and_survive_runs() {
    let mut canvas = CanvasContext::new(1, 1).expect("canvas");
    canvas
        .set_dots(&[
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (1, 0),
            (1, 1),
            (1, 2),
            (1, 3),
        ])
        .expect("dots");
    assert_eq!(symbol(&canvas, 0, 0), "\u{28FF}");
    canvas.clear_dot(1, 3).expect("clear");
    assert_eq!(symbol(&canvas, 0, 0), "\u{287F}");
    canvas.clear_dot(0, 0).expect("clear");
    assert_eq!(symbol(&canvas, 0, 0), "\u{287E}");

    let mut canvas = CanvasContext::new(1, 1).expect("canvas");
    canvas.set_run(0, 0, 3).expect("run");
    // Dots 1, 2, 3, and 7.
    assert_eq!(symbol(&canvas, 0, 0), "\u{2847}");
    canvas.set_run(0, 3, 0).expect("reverse run");
    assert_eq!(symbol(&canvas, 0, 0), "\u{2847}");
}

#[test]
fn cell_writes_clear_the_dot_mask_and_bounds_are_ignored() {
    let mut canvas = CanvasContext::new(2, 1).expect("canvas");
    canvas.set(0, 0, "A").expect("cell");
    canvas.set_dot(1, 0).expect("dot");
    assert_eq!(symbol(&canvas, 0, 0), "\u{2808}");
    canvas.set(0, 0, "B").expect("cell");
    assert_eq!(symbol(&canvas, 0, 0), "B");

    canvas.set_dot(-1, 0).expect("ignored");
    canvas.set_dot(0, -1).expect("ignored");
    canvas.set_dot(4, 0).expect("ignored");
    canvas.set_dot(0, 4).expect("ignored");
    assert_eq!(canvas.dot_width(), 4);
    assert_eq!(canvas.dot_height(), 4);
    canvas.clear().expect("clear");
    assert_eq!(symbol(&canvas, 0, 0), " ");
}
