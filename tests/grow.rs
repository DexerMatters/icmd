//! Weighted layout: `Dimension::Grow` splits the remaining main-axis space by
//! weight and keeps `Max` as `Grow(1)`.

use std::time::Duration;

use icmd::advanced::{Commit, Lower, Renderer, Runtime};
use icmd::{Attr, Dimension, DomProps, ElementRef, Layout, Node, Size};

fn render(viewport: Size, node: Node) {
    let (commit, _, _) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).expect("renderer"))
        .start();
    input.send(node).expect("root accepted");
    let _ = output
        .recv_timeout(Duration::from_secs(2))
        .expect("frame")
        .expect("render");
}

fn sized(width: Dimension, height: Dimension, refs: &mut Vec<ElementRef>) -> Node {
    let handle = ElementRef::new();
    refs.push(handle.clone());
    let mut dom = DomProps::default();
    dom.style.width = Attr::Set(width);
    dom.style.height = Attr::Set(height);
    dom.element_ref = Attr::Set(handle);
    Node::element(dom, [icmd::text("x")])
}

fn widths(refs: &[ElementRef]) -> Vec<u32> {
    refs.iter()
        .map(|handle| handle.current().expect("committed").bounding_rect().width)
        .collect()
}

#[test]
fn grow_splits_remaining_space_by_weight_with_largest_remainder() {
    let mut refs = Vec::new();
    let children = vec![
        sized(Dimension::Cells(4), Dimension::Cells(1), &mut refs),
        sized(Dimension::Grow(1), Dimension::Cells(1), &mut refs),
        sized(Dimension::Grow(2), Dimension::Cells(1), &mut refs),
    ];
    let mut row = DomProps::default();
    row.style.layout = Attr::Set(Layout::Horizontal);
    row.style.width = Attr::Set(Dimension::Max);
    row.style.height = Attr::Set(Dimension::Cells(3));
    let root = Node::element(row, children);
    render(Size::new(20, 4), root);
    assert_eq!(
        widths(&refs),
        vec![4, 5, 11],
        "remaining 16 cells split 1:2 with the largest remainder on the second grow"
    );
}

#[test]
fn max_is_grow_one_and_mixes_with_weighted_siblings() {
    let mut refs = Vec::new();
    let children = vec![
        sized(Dimension::Max, Dimension::Cells(1), &mut refs),
        sized(Dimension::Grow(1), Dimension::Cells(1), &mut refs),
    ];
    let mut row = DomProps::default();
    row.style.layout = Attr::Set(Layout::Horizontal);
    row.style.width = Attr::Set(Dimension::Max);
    row.style.height = Attr::Set(Dimension::Cells(3));
    render(Size::new(20, 4), Node::element(row, children));
    assert_eq!(widths(&refs), vec![10, 10]);
}

#[test]
fn a_zero_weight_receives_nothing() {
    let mut refs = Vec::new();
    let children = vec![
        sized(Dimension::Grow(0), Dimension::Cells(1), &mut refs),
        sized(Dimension::Grow(1), Dimension::Cells(1), &mut refs),
    ];
    let mut row = DomProps::default();
    row.style.layout = Attr::Set(Layout::Horizontal);
    row.style.width = Attr::Set(Dimension::Max);
    row.style.height = Attr::Set(Dimension::Cells(3));
    render(Size::new(20, 4), Node::element(row, children));
    assert_eq!(widths(&refs), vec![0, 20]);
}

#[test]
fn grow_works_on_the_vertical_main_axis() {
    let mut refs = Vec::new();
    let children = vec![
        sized(Dimension::Max, Dimension::Grow(1), &mut refs),
        sized(Dimension::Max, Dimension::Grow(2), &mut refs),
    ];
    let mut column = DomProps::default();
    column.style.layout = Attr::Set(Layout::Vertical);
    column.style.width = Attr::Set(Dimension::Max);
    column.style.height = Attr::Set(Dimension::Max);
    render(Size::new(12, 10), Node::element(column, children));
    let heights: Vec<u32> = refs
        .iter()
        .map(|handle| handle.current().expect("committed").bounding_rect().height)
        .collect();
    assert_eq!(heights, vec![3, 7]);
}

#[test]
fn grow_on_the_cross_axis_fills_the_available_space() {
    let mut refs = Vec::new();
    let children = vec![sized(Dimension::Grow(3), Dimension::Cells(1), &mut refs)];
    let mut column = DomProps::default();
    column.style.layout = Attr::Set(Layout::Vertical);
    column.style.width = Attr::Set(Dimension::Max);
    column.style.height = Attr::Set(Dimension::Max);
    render(Size::new(15, 4), Node::element(column, children));
    assert_eq!(widths(&refs), vec![15]);
}
