use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use icmd::{
    AxisPosition, Cell, Commit, Component, Dimension, Fill, Image, Justify, Layout, Lower, Node,
    Operation, Overflow, Renderer, Runtime, Size, canvas, fragment, text, view,
};

fn render(node: Node, viewport: Size) -> String {
    let (commit, _) = Commit::new(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
    input.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap()
}

fn positioned_parent(overflow: Overflow, child: Node) -> Node {
    view.style(|style| {
        style.layout /= Layout::Absolute;
        style.width /= Dimension::Cells(4);
        style.height /= Dimension::Cells(1);
        style.column /= AxisPosition::Cells(1);
        style.overflow /= overflow;
    })
    .child(child)
}

fn viewport_root(child: Node) -> Node {
    view.style(|style| {
        style.layout /= Layout::Absolute;
        style.width /= Dimension::Cells(5);
        style.height /= Dimension::Cells(1);
    })
    .child(child)
}

#[test]
fn clipped_descendants_do_not_paint_or_receive_pointer_events() {
    let clipped = viewport_root(positioned_parent(
        Overflow::Clip,
        view.style(|style| style.column /= AxisPosition::Cells(-1))
            .child("X"),
    ));
    let visible = viewport_root(positioned_parent(
        Overflow::Visible,
        view.style(|style| style.column /= AxisPosition::Cells(-1))
            .child("X"),
    ));
    let clipped_frame = render(clipped, Size::new(5, 1));
    let visible_frame = render(visible, Size::new(5, 1));
    assert!(!clipped_frame.contains('X'));
    assert!(visible_frame.contains('X'));

    let hits = Arc::new(AtomicUsize::new(0));
    let clipped = viewport_root(positioned_parent(
        Overflow::Clip,
        view.style(|style| style.column /= AxisPosition::Cells(-1))
            .events({
                let hits = hits.clone();
                move |events| {
                    let hits = hits.clone();
                    events.pointer_down /= icmd::EventListener::new(move |_| {
                        hits.fetch_add(1, Ordering::SeqCst);
                    });
                }
            })
            .child("X"),
    ));
    let (commit, _, dispatcher) = Commit::new_with_events(Size::new(5, 1));
    let (input, output) = Runtime::new(Lower::default()).then(commit).start();
    input.send(clipped).unwrap();
    output.recv_timeout(Duration::from_secs(1)).unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn stretch_subtracts_cross_axis_margins() {
    let node = view
        .style(|style| {
            style.layout /= Layout::Horizontal;
            style.width /= Dimension::Cells(3);
            style.height /= Dimension::Cells(3);
            style.align /= icmd::Align::Stretch;
        })
        .child(
            view.style(|style| {
                style.width /= Dimension::Cells(1);
                style.margin /= icmd::Edges::symmetric(1, 0);
                style.fill /= "X";
            })
            .child(""),
        );
    assert_eq!(render(node, Size::new(3, 3)).matches('X').count(), 1);
}

#[test]
fn space_between_distributes_odd_remainders_to_the_leading_gap() {
    let child = |symbol: &'static str| {
        view.style(|style| {
            style.width /= Dimension::Cells(1);
            style.height /= Dimension::Cells(1);
            style.fill /= symbol;
        })
        .child("")
    };
    let node = view
        .style(|style| {
            style.layout /= Layout::Horizontal;
            style.width /= Dimension::Cells(10);
            style.height /= Dimension::Cells(1);
            style.justify /= Justify::SpaceBetween;
        })
        .children([child("a"), child("b"), child("c")]);
    let frame = render(node, Size::new(10, 1));
    assert!(frame.contains("\x1b[1;1Ha"));
    assert!(frame.contains("\x1b[1;6Hb"));
    assert!(frame.contains("\x1b[1;10Hc"));
}

#[test]
fn limits_are_fallible_and_graphemes_are_bounded() {
    assert!(Renderer::try_new(Size::new(1025, 1025)).is_err());
    assert!(Image::blank(1024, 1024, Cell::blank()).is_ok());
    assert!(Image::blank(1024, 1025, Cell::blank()).is_err());
    assert!(Cell::plain("a".repeat(257)).is_err());
    assert!(Fill::new("a".repeat(257)).is_err());
}

#[test]
fn root_fragments_render_through_the_internal_wrapper() {
    let frame = render(fragment([text("one"), text("two")]), Size::new(8, 2));
    assert!(frame.contains('o'));
    assert!(frame.contains('t'));
}

#[test]
fn extreme_canvas_lines_are_clipped_before_iteration() {
    let node = canvas
        .extra(|props| {
            props.width /= 4;
            props.height /= 2;
            props.draw /= Arc::new(|drawing: &mut icmd::CanvasContext| {
                drawing
                    .line(i32::MIN, i32::MIN, i32::MAX, i32::MAX, "x")
                    .unwrap();
            });
        })
        .node();
    let frame = render(node, Size::new(4, 2));
    assert!(frame.contains('x'));
}

#[test]
fn oversized_declarative_canvas_uses_error_placeholder() {
    let node = canvas
        .extra(|props| {
            props.width /= 1025;
            props.height /= 1025;
        })
        .node();
    assert!(render(node, Size::new(8, 1)).contains('c'));
}

#[test]
fn renderer_tie_order_is_deterministic() {
    let mut renderer = Renderer::try_new(Size::new(2, 1)).unwrap();
    let a = Image::from_rows(vec![vec![Cell::plain("A").unwrap()]]).unwrap();
    let b = Image::from_rows(vec![vec![Cell::plain("B").unwrap()]]).unwrap();
    renderer
        .apply_frame(icmd::Frame {
            operations: vec![
                Operation::Create {
                    id: icmd::ImageId(1),
                    image: a,
                    position: icmd::ScreenPosition::new(0, 0),
                    level: 0,
                },
                Operation::Create {
                    id: icmd::ImageId(2),
                    image: b,
                    position: icmd::ScreenPosition::new(0, 0),
                    level: 0,
                },
                Operation::SetOrder {
                    id: icmd::ImageId(1),
                    order: 0,
                },
                Operation::SetOrder {
                    id: icmd::ImageId(2),
                    order: 0,
                },
            ],
            viewport: None,
            force_redraw: false,
        })
        .unwrap();
    let diff = renderer.try_render_diff().unwrap().unwrap();
    assert!(diff.contains('B'));
    assert!(!diff.contains('A'));
}

#[test]
fn renderer_rejects_frames_atomically_and_restores_overlap() {
    let mut renderer = Renderer::new(Size::new(4, 1));
    renderer.render_diff();
    let image = |symbol: &str| Image::from_rows(vec![vec![Cell::plain(symbol).unwrap()]]).unwrap();
    renderer
        .apply_frame(icmd::Frame::new(vec![
            Operation::Create {
                id: icmd::ImageId(1),
                image: image("A"),
                position: icmd::ScreenPosition::default(),
                level: 0,
            },
            Operation::Create {
                id: icmd::ImageId(2),
                image: image("B"),
                position: icmd::ScreenPosition::default(),
                level: 0,
            },
        ]))
        .unwrap();
    assert!(renderer.render_diff().unwrap().contains('B'));
    renderer
        .apply_frame(icmd::Frame::new(vec![Operation::Remove {
            id: icmd::ImageId(2),
        }]))
        .unwrap();
    assert!(renderer.render_diff().unwrap().contains('A'));

    let result = renderer.apply_frame(icmd::Frame::new(vec![
        Operation::Move {
            id: icmd::ImageId(99),
            position: icmd::ScreenPosition::default(),
        },
        Operation::Create {
            id: icmd::ImageId(3),
            image: image("C"),
            position: icmd::ScreenPosition::default(),
            level: 0,
        },
    ]));
    assert!(result.is_err());
    assert!(renderer.render_diff().is_none());
}

#[test]
fn renderer_handles_wide_patch_boundaries_and_resize() {
    let wide = Cell::plain("界").unwrap();
    let image = Image::from_rows(vec![vec![wide, Cell::plain("A").unwrap()]]).unwrap();
    let mut renderer = Renderer::new(Size::new(3, 1));
    renderer.render_diff();
    renderer
        .apply_frame(icmd::Frame::new(vec![Operation::Create {
            id: icmd::ImageId(1),
            image,
            position: icmd::ScreenPosition::default(),
            level: 0,
        }]))
        .unwrap();
    renderer.render_diff();
    renderer
        .apply_frame(icmd::Frame::new(vec![Operation::PatchRect {
            id: icmd::ImageId(1),
            rect: icmd::Rect::new(0, 1, 1, 1),
            rows: vec![vec![Cell::plain("B").unwrap()]],
        }]))
        .unwrap();
    assert!(renderer.render_diff().unwrap().contains('B'));
    renderer
        .apply_frame(icmd::Frame::empty().resize(Size::new(2, 1)))
        .unwrap();
    assert!(renderer.render_diff().unwrap().contains("\x1b[2J"));
}
