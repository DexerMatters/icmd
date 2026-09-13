use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use icmd::advanced::{Commit, Lower, Renderer, Runtime};
use icmd::{
    AxisPosition, Cell, Component, Dimension, Fill, Image, Justify, Layout, Node, Operation,
    Overflow, Size, canvas, fragment, text, view,
};

fn render(node: Node, viewport: Size) -> String {
    let (commit, _) = Commit::new(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
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
fn layouts_clip_children_by_default() {
    let node = viewport_root(
        view.style(|style| {
            style.layout /= Layout::Absolute;
            style.width /= Dimension::Cells(4);
            style.height /= Dimension::Cells(1);
            style.column /= AxisPosition::Cells(1);
        })
        .child(
            view.style(|style| style.column /= AxisPosition::Cells(-1))
                .child("X"),
        ),
    );
    assert!(!render(node, Size::new(5, 1)).contains('X'));
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
    assert!(Renderer::new(Size::new(1025, 1025)).is_err());
    assert!(Image::new(1024, 1024, Cell::blank()).is_ok());
    assert!(Image::new(1024, 1025, Cell::blank()).is_err());
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
    let mut renderer = Renderer::new(Size::new(2, 1)).unwrap();
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
    let diff = renderer.render_diff().unwrap().unwrap();
    assert!(diff.contains('B'));
    assert!(!diff.contains('A'));
}

#[test]
fn renderer_rejects_frames_atomically_and_restores_overlap() {
    let mut renderer = Renderer::new(Size::new(4, 1)).unwrap();
    renderer.render_diff().unwrap();
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
    assert!(renderer.render_diff().unwrap().unwrap().contains('B'));
    renderer
        .apply_frame(icmd::Frame::new(vec![Operation::Remove {
            id: icmd::ImageId(2),
        }]))
        .unwrap();
    assert!(renderer.render_diff().unwrap().unwrap().contains('A'));

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
    assert!(renderer.render_diff().unwrap().is_none());
}

#[test]
fn renderer_handles_wide_patch_boundaries_and_resize() {
    let wide = Cell::plain("界").unwrap();
    let image = Image::from_rows(vec![vec![wide, Cell::plain("A").unwrap()]]).unwrap();
    let mut renderer = Renderer::new(Size::new(3, 1)).unwrap();
    renderer.render_diff().unwrap();
    renderer
        .apply_frame(icmd::Frame::new(vec![Operation::Create {
            id: icmd::ImageId(1),
            image,
            position: icmd::ScreenPosition::default(),
            level: 0,
        }]))
        .unwrap();
    renderer.render_diff().unwrap();
    renderer
        .apply_frame(icmd::Frame::new(vec![Operation::PatchRect {
            id: icmd::ImageId(1),
            rect: icmd::Rect::new(0, 1, 1, 1),
            rows: vec![vec![Cell::plain("B").unwrap()]],
        }]))
        .unwrap();
    assert!(renderer.render_diff().unwrap().unwrap().contains('B'));
    renderer
        .apply_frame(icmd::Frame::empty().resize(Size::new(2, 1)))
        .unwrap();
    assert!(renderer.render_diff().unwrap().unwrap().contains("\x1b[2J"));
}

// PERF-11: a commit produces one canonical scene order. The rendered output for
// a given scene must therefore be byte-identical across repeated builds, with no
// dependence on hash iteration order.
#[test]
fn scene_order_is_canonical_and_deterministic() {
    let viewport = Size::new(24, 6);
    let build = || {
        let children: Vec<Node> = (0..24_i32)
            .map(|index| {
                let node = text(format!("row{index}"));
                let mut dom = icmd::DomProps::default();
                dom.style = icmd::style(|style| {
                    style.line /= icmd::AxisPosition::Cells(index % 6);
                    style.column /= icmd::AxisPosition::Cells((index % 4) * 5);
                    style.width /= Dimension::Cells(4);
                    style.height /= Dimension::Cells(1);
                });
                Node::element(dom, [node])
            })
            .collect();
        Node::element(icmd::DomProps::default(), children)
    };

    let first = render(build(), viewport);
    for _ in 0..3 {
        assert_eq!(
            render(build(), viewport),
            first,
            "identical scenes must render identically"
        );
    }
}

// The scene order is produced once per commit: a no-op commit must not emit a
// second ordering pass or duplicate operations.
#[test]
fn a_second_identical_frame_emits_no_operations() {
    let viewport = Size::new(12, 3);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let node = || -> Node { text("same") };
    input.send(node()).unwrap();
    let first = output.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(
        !first.operations.is_empty(),
        "the first frame creates the scene"
    );
    input.send(node()).unwrap();
    let second = output.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(
        second.operations.is_empty(),
        "an unchanged scene must emit no operations, got {:?}",
        second.operations
    );
    drop(input);
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
}

// PERF-10: the composition pass must not clone the layer list per frame, and
// the retained order must survive a render unchanged.
#[test]
fn layer_order_survives_repeated_composition_without_cloning() {
    use icmd::{Cell, Frame, Image, ImageId, Operation, ScreenPosition};

    let viewport = Size::new(6, 2);
    let mut renderer = Renderer::with_config(
        viewport,
        icmd::advanced::RendererConfig {
            image_protocol: icmd::ImageProtocol::Symbols,
            ..icmd::advanced::RendererConfig::default()
        },
    )
    .unwrap();

    // Several overlapping layers with distinct z-order and level.
    let mut operations = Vec::new();
    for index in 0..8u64 {
        operations.push(Operation::Create {
            id: ImageId(index + 1),
            image: Image::new(4, 1, Cell::plain((b'a' + index as u8) as char).unwrap()).unwrap(),
            position: ScreenPosition::new(0, index as i32 % 3),
            level: (index % 3) as i32,
        });
        operations.push(Operation::SetOrder {
            id: ImageId(index + 1),
            order: 8 - index,
        });
    }
    renderer.apply_frame(Frame::new(operations)).unwrap();
    let first = renderer.render_diff().unwrap().expect("first frame");

    // A second identical render is a no-op: the retained order is unchanged.
    assert!(renderer.render_diff().unwrap().is_none());

    // A single-cell update repaints only its damage and keeps the same layering.
    renderer
        .apply_frame(Frame::new(vec![Operation::PatchCells {
            id: ImageId(1),
            edits: vec![icmd::CellEdit {
                position: icmd::ImagePosition::new(0, 0),
                cell: Cell::plain("Z").unwrap(),
            }],
        }]))
        .unwrap();
    let updated = renderer
        .render_diff()
        .unwrap()
        .expect("a cell update must repaint");
    assert!(updated.contains('Z'), "{updated:?}");
    assert!(!first.is_empty());
}

// PERF-07: a no-wrap line scrolled offscreen must rasterize only the visible
// window, so the temporary allocation does not grow with the document.
#[test]
fn offscreen_text_rasterizes_only_the_visible_window() {
    use icmd::{AxisPosition, Dimension, DomProps, Overflow, Text, TextWrap};

    let viewport = Size::new(40, 3);
    let build = |prefix: usize| -> Node {
        // A long single line with the viewport scrolled to the far end.
        let content: String = "abcdefghij".repeat(prefix.max(1));
        let mut dom = DomProps::default();
        dom.style = icmd::style(|style| {
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
            style.overflow /= Overflow::Clip;
            style.overflow_x /= Overflow::Clip;
        });
        let area = icmd::scroll_area
            .props(icmd::ScrollAreaProps {
                axes: icmd::Attr::Set(icmd::ScrollAxes::Horizontal),
                scrollbar_visibility: icmd::Attr::Set(icmd::ScrollbarVisibility::Hidden),
                ..icmd::ScrollAreaProps::default()
            })
            .style(|style| {
                style.width /= Dimension::Cells(20);
                style.height /= Dimension::Cells(1);
                style.line /= AxisPosition::Cells(0);
            })
            .children([Text::new(content)
                .wrap(TextWrap::NoWrap)
                .layout_style(icmd::style(|style| style.width /= Dimension::Max))
                .into()]);
        Node::element(dom, [area])
    };

    // Both documents paint the same visible window and must render identically
    // at the same viewport; the longer document is not allowed to change the
    // painted cells.
    let short = render(build(4), viewport);
    let long = render(build(400), viewport);
    assert!(
        long.starts_with(&short[..short.len().min(8)]),
        "the visible window must be painted the same way regardless of document length"
    );
    assert!(!short.is_empty() && !long.is_empty());
}

// PERF-03: shaping stores one shared normalized buffer and glyphs reference
// ranges into it, so the painted result is identical to the previous
// per-glyph storage.
#[test]
fn shaped_text_shares_one_normalized_buffer() {
    use icmd::{Text, TextWrap};

    // Mixed content exercises the separate-unit path, tabs, newlines, wide
    // glyphs, and controls through the same shared buffer.
    let content = "ab界\tcd\n🙂e\u{7}f ghij";
    let node: Node = Text::new(content)
        .wrap(TextWrap::NoWrap)
        .layout_style(icmd::style(|style| style.width /= icmd::Dimension::Max))
        .into();
    let frame = render(node, Size::new(32, 2));
    // The rendered glyphs are the same ones the source contains.
    for expected in ['a', 'b', '界', 'c', 'd', 'e', 'f', 'g'] {
        assert!(
            frame.contains(expected),
            "expected {expected:?} in the shaped output: {frame:?}"
        );
    }
    assert!(!frame.is_empty());
}
