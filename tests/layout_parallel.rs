// Parallel panels on one row: what keeps every border inside the viewport?
//
// A percent width resolves against the container's content width, and the gaps
// between children are *extra* (the same rule as CSS `width: 50%` next to a
// `gap`). Two `Percent::available(50)` children plus a one-cell gap therefore
// overflow by exactly one cell, and the trailing card's right border - the
// last painted cell - is the cell that gets clipped. `Dimension::Max` shares
// the space left after the fixed children and the gaps, which is the pattern
// that always fits.

use std::time::Duration;

use icmd::advanced::{Commit, Lower, Renderer, Runtime};
use icmd::{Dimension, Node, Percent, ScrollAxes, Size, Text, card, row, scroll_area, ui, view};

fn frame_of(node: Node, width: u16, height: u16) -> String {
    let viewport = Size::new(width, height);
    let (commit, _, _) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start();
    input.send(node).unwrap();
    // Settle so the frame inspected is the one the layout converged on.
    let mut raw = String::new();
    loop {
        match output.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(frame)) => raw = frame,
            Ok(Err(error)) => panic!("frame error: {error}"),
            Err(_) => break,
        }
    }
    assert!(!raw.is_empty(), "no frame arrived");
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            out.push(ch);
            continue;
        }
        if let Some('[') = chars.next() {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

// Every painted line must open and close as many corners as it draws; an
// unbalanced line means a card's border ran past the clip.
fn borders_are_balanced(frame: &str) -> bool {
    frame.lines().all(|line| {
        let corners =
            |open: char, close: char| line.matches(open).count() == line.matches(close).count();
        corners('┌', '┐') && corners('└', '┘') && corners('╭', '╮') && corners('╰', '╯')
    })
}

fn columns(left: Dimension, right: Dimension) -> Node {
    let column = |width: Dimension, label: &str| {
        let label = label.to_string();
        ui! {
            <view style={move |s| s.width /= width}>
                <card style={|s| s.width /= Dimension::Max}>
                    {Text::new(label)}
                </card>
            </view>
        }
    };
    let body = ui! {
        <row style={|s| { s.width /= Dimension::Max; s.gap /= 1; }}>
            {column(left, "LEFT")}
            {column(right, "RIGHT")}
        </row>
    };
    ui! {
        <scroll_area axes={ScrollAxes::Vertical} style={|s| {
            s.width /= Dimension::Max;
            s.height /= Dimension::Max;
        }}>{body}</scroll_area>
    }
}

#[test]
fn max_columns_keep_every_border_inside_the_row() {
    for width in [39u16, 40, 41, 78, 79, 80] {
        for (name, left, right) in [
            ("max+max", Dimension::Max, Dimension::Max),
            (
                "percent+max",
                Dimension::Percent(Percent::available(50)),
                Dimension::Max,
            ),
        ] {
            let painted = frame_of(columns(left, right), width, 6);
            assert!(
                borders_are_balanced(&painted),
                "width {width} with {name} clipped a border:\n{painted}"
            );
        }
    }
}

// The pitfall, pinned so the guidance in `examples/demo.rs` cannot silently
// become wrong: two percent columns and a gap overflow by the gap.
#[test]
fn two_percent_columns_plus_a_gap_overflow_by_the_gap() {
    for width in [40u16, 41, 78, 79] {
        let painted = frame_of(
            columns(
                Dimension::Percent(Percent::available(50)),
                Dimension::Percent(Percent::available(50)),
            ),
            width,
            6,
        );
        assert!(
            !borders_are_balanced(&painted),
            "width {width}: two 50% columns were expected to overrun by the gap:\n{painted}"
        );
    }
}
