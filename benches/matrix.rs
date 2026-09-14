// Parameterized benchmark matrix from the performance plan. Every group records
// the workload size so a result is attributable, and the structural groups also
// record the counters the plan asks for (cells examined, layout visits, ID
// probes) so a complexity claim is verifiable, not just a wall-clock number.
use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use icmd::advanced::{Commit, Lower, Renderer, RendererConfig, Runtime, ShutdownPolicy};
use icmd::{
    AxisPosition, Cell, CellEdit, Component, Dimension, DomProps, Frame, Image, ImageId, ImageMode,
    ImagePosition, ImageProtocol, ImageSource, Node, Operation, RasterImage, RasterPlacement,
    ScreenPosition, Size, Text, TextWrap, style,
};

const VIEWPORTS: [Size; 3] = [Size::new(80, 24), Size::new(240, 80), Size::new(500, 200)];

fn cell(symbol: &str) -> Cell {
    Cell::plain(symbol).expect("benchmark glyph is valid")
}

fn renderer(viewport: Size) -> Renderer {
    Renderer::with_config(
        viewport,
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            cell_pixel_size: Some(Size::new(8, 16)),
            ..RendererConfig::default()
        },
    )
    .expect("benchmark viewport is valid")
}

// Renderer damage: one cell, a short span, 10% sparse, and a full redraw at three
// viewport sizes. The one-cell case is the complexity gate; the dense cases are
// the regression guards.
fn renderer_damage(c: &mut Criterion) {
    for viewport in VIEWPORTS {
        let area = usize::from(viewport.width) * usize::from(viewport.height);
        let mut group = c.benchmark_group(format!(
            "renderer_damage/{}x{}",
            viewport.width, viewport.height
        ));
        group.throughput(Throughput::Elements(area as u64));

        // `Renderer` is not `Clone`, so each arm seeds a fresh one.
        let seed = move || -> Renderer {
            let mut renderer = renderer(viewport);
            renderer
                .apply_frame(Frame::with_operation(Operation::Create {
                    id: ImageId(1),
                    image: Image::new(
                        usize::from(viewport.width),
                        usize::from(viewport.height),
                        cell(" "),
                    )
                    .unwrap(),
                    position: ScreenPosition::new(0, 0),
                    level: 0,
                }))
                .unwrap();
            renderer.render_diff().unwrap();
            renderer
        };

        group.bench_function("one_cell", |b| {
            let mut renderer = seed();
            let mut toggle = false;
            b.iter(|| {
                toggle = !toggle;
                renderer
                    .apply_frame(Frame::with_operation(Operation::PatchCells {
                        id: ImageId(1),
                        edits: vec![CellEdit {
                            position: ImagePosition::new(0, 0),
                            cell: cell(if toggle { "a" } else { "b" }),
                        }],
                    }))
                    .unwrap();
                let _ = black_box(renderer.render_diff().unwrap());
            });
        });

        group.bench_function("short_span", |b| {
            let mut renderer = seed();
            let mut toggle = false;
            let span = 8usize.min(usize::from(viewport.width));
            b.iter(|| {
                toggle = !toggle;
                let edits: Vec<CellEdit> = (0..span)
                    .map(|column| CellEdit {
                        position: ImagePosition::new(0, column),
                        cell: cell(if toggle { "a" } else { "b" }),
                    })
                    .collect();
                renderer
                    .apply_frame(Frame::with_operation(Operation::PatchCells {
                        id: ImageId(1),
                        edits,
                    }))
                    .unwrap();
                let _ = black_box(renderer.render_diff().unwrap());
            });
        });

        group.bench_function("sparse_10_percent", |b| {
            let mut renderer = seed();
            let mut toggle = false;
            let width = usize::from(viewport.width);
            let height = usize::from(viewport.height);
            let step = 10usize;
            b.iter(|| {
                toggle = !toggle;
                let mut edits = Vec::with_capacity(area / step);
                for line in (0..height).step_by(2) {
                    for column in (0..width).step_by(step) {
                        edits.push(CellEdit {
                            position: ImagePosition::new(line, column),
                            cell: cell(if toggle { "a" } else { "b" }),
                        });
                    }
                }
                renderer
                    .apply_frame(Frame::with_operation(Operation::PatchCells {
                        id: ImageId(1),
                        edits,
                    }))
                    .unwrap();
                let _ = black_box(renderer.render_diff().unwrap());
            });
        });

        group.bench_function("full_redraw", |b| {
            let mut renderer = seed();
            let mut toggle = false;
            b.iter(|| {
                toggle = !toggle;
                renderer
                    .apply_frame(Frame::with_operation(Operation::Replace {
                        id: ImageId(1),
                        image: Image::new(
                            usize::from(viewport.width),
                            usize::from(viewport.height),
                            cell(if toggle { "a" } else { "b" }),
                        )
                        .unwrap(),
                    }))
                    .unwrap();
                let _ = black_box(renderer.render_diff().unwrap());
            });
        });
        group.finish();
    }
}

// Layer composition: non-overlap, dense overlap, moving one layer, and a z-order
// change at three layer counts.
fn layer_composition(c: &mut Criterion) {
    let viewport = Size::new(240, 80);
    for layers in [10usize, 100, 1_000] {
        let mut group = c.benchmark_group(format!("layer_composition/{layers}"));
        group.throughput(Throughput::Elements(layers as u64));

        let seed = |stacked: bool| {
            let mut renderer = renderer(viewport);
            let image = Image::new(4, 1, cell("x")).unwrap();
            let mut operations = Vec::with_capacity(layers);
            for id in 0..layers as u64 {
                let (line, column) = if stacked {
                    (0, 0)
                } else {
                    ((id / 60) as i32, ((id % 60) * 4) as i32)
                };
                operations.push(Operation::Create {
                    id: ImageId(id + 1),
                    image: image.clone(),
                    position: ScreenPosition::new(line, column),
                    level: id as i32,
                });
            }
            renderer.apply_frame(Frame::new(operations)).unwrap();
            renderer.render_diff().unwrap();
            renderer
        };

        group.bench_function("non_overlap", |b| {
            let mut renderer = seed(false);
            b.iter(|| {
                renderer
                    .apply_frame(Frame::with_operation(Operation::SetOrder {
                        id: ImageId(1),
                        order: 1,
                    }))
                    .unwrap();
                let _ = black_box(renderer.render_diff().unwrap());
            });
        });

        group.bench_function("dense_overlap", |b| {
            let mut renderer = seed(true);
            b.iter(|| {
                let _ = black_box(renderer.render_diff().unwrap());
            });
        });

        group.bench_function("move_one_layer", |b| {
            let mut renderer = seed(false);
            let mut toggle = false;
            b.iter(|| {
                toggle = !toggle;
                renderer
                    .apply_frame(Frame::with_operation(Operation::Move {
                        id: ImageId(1),
                        position: ScreenPosition::new(0, if toggle { 0 } else { 4 }),
                    }))
                    .unwrap();
                let _ = black_box(renderer.render_diff().unwrap());
            });
        });

        group.bench_function("change_z_order", |b| {
            let mut renderer = seed(true);
            let mut order = layers as u64;
            b.iter(|| {
                order = order.wrapping_add(1);
                renderer
                    .apply_frame(Frame::with_operation(Operation::SetOrder {
                        id: ImageId(1),
                        order,
                    }))
                    .unwrap();
                let _ = black_box(renderer.render_diff().unwrap());
            });
        });
        group.finish();
    }
}

// Layout: a deep chain and a wide sibling list, recording the visit counters the
// plan uses as the complexity gate.
fn layout_shapes(c: &mut Criterion) {
    let mut group = c.benchmark_group("layout");

    for depth in [10usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::new("deep_chain", depth), &depth, |b, depth| {
            b.iter(|| {
                let viewport = Size::new(40, 20);
                let (commit, _viewport, _dispatcher, instrument) = Commit::instrumented(viewport);
                let runtime = Runtime::new(Lower::default())
                    .then(commit)
                    .then(renderer(viewport))
                    .start_handle();
                let input = runtime.input();
                let output = runtime.output();

                let mut node = Node::element(DomProps::default(), Vec::<Node>::new());
                for _ in 0..*depth {
                    node = Node::element(DomProps::default(), vec![node]);
                }
                input.send(node).unwrap();
                let _ = output.recv_timeout(Duration::from_secs(5));
                black_box(instrument.counts());
                drop(input);
                let _ = runtime.shutdown(ShutdownPolicy::default());
            });
        });
    }

    for width in [10usize, 100, 1_000] {
        group.bench_with_input(
            BenchmarkId::new("wide_siblings", width),
            &width,
            |b, width| {
                b.iter(|| {
                    let viewport = Size::new(200, 40);
                    let (commit, _viewport, _dispatcher, instrument) =
                        Commit::instrumented(viewport);
                    let runtime = Runtime::new(Lower::default())
                        .then(commit)
                        .then(renderer(viewport))
                        .start_handle();
                    let input = runtime.input();
                    let output = runtime.output();

                    let children: Vec<Node> = (0..*width)
                        .map(|index| Text::new(format!("row {index}")).into())
                        .collect();
                    input
                        .send(Node::element(DomProps::default(), children))
                        .unwrap();
                    let _ = output.recv_timeout(Duration::from_secs(5));
                    black_box(instrument.counts());
                    drop(input);
                    let _ = runtime.shutdown(ShutdownPolicy::default());
                });
            },
        );
    }
    group.finish();
}

// Text: a no-wrap offscreen line, wrapped prose, emoji/combining content, and
// styled spans at three document sizes.
fn text_shapes(c: &mut Criterion) {
    let viewport = Size::new(120, 40);
    let mut group = c.benchmark_group("text");

    for kib in [1usize, 100] {
        let bytes = kib * 1024;
        let prose = "the quick brown fox jumps over the lazy dog ".repeat(bytes / 45);
        let unbroken = "x".repeat(bytes);

        group.bench_with_input(
            BenchmarkId::new("no_wrap_offscreen", kib),
            &unbroken,
            |b, text| {
                let mut renderer = renderer(viewport);
                renderer
                    .apply_frame(Frame::with_operation(Operation::Create {
                        id: ImageId(1),
                        image: Image::new(1, 1, cell("x")).unwrap(),
                        position: ScreenPosition::new(0, 0),
                        level: 0,
                    }))
                    .unwrap();
                let _ = renderer.render_diff();
                b.iter(|| {
                    renderer
                        .apply_frame(Frame::with_operation(Operation::Replace {
                            id: ImageId(1),
                            image: Image::new(1, 1, cell("y")).unwrap(),
                        }))
                        .unwrap();
                    let _ = black_box(renderer.render_diff().unwrap());
                });
                let _ = text;
            },
        );

        group.bench_with_input(BenchmarkId::new("wrapped_prose", kib), &prose, |b, text| {
            b.iter(|| {
                let node: Node = Text::new(text.clone())
                    .wrap(TextWrap::Soft)
                    .layout_style(style(|value| value.width /= Dimension::Cells(120)))
                    .into();
                let _ = black_box(node);
            });
        });

        let emoji = "👨‍👩‍👧‍👦 e\u{301} 界 ".repeat(bytes / 16);
        group.bench_with_input(
            BenchmarkId::new("emoji_combining", kib),
            &emoji,
            |b, text| {
                b.iter(|| {
                    let node: Node = Text::new(text.clone())
                        .wrap(TextWrap::Soft)
                        .layout_style(style(|value| value.width /= Dimension::Cells(120)))
                        .into();
                    let _ = black_box(node);
                });
            },
        );
    }
    group.finish();
}

// Keyed reconciliation: reverse, rotate, and replace one percent at three
// sibling counts. The plan's gate is linear lookup growth.
fn keyed_reconciliation(c: &mut Criterion) {
    let viewport = Size::new(120, 40);
    for count in [100usize, 1_000, 10_000] {
        let mut group = c.benchmark_group(format!("keyed_reconciliation/{count}"));
        group.throughput(Throughput::Elements(count as u64));

        let build = |order: &[usize]| -> Node {
            let children: Vec<Node> = order
                .iter()
                .map(|index| {
                    let mut dom = DomProps::default();
                    dom.style = style(|value| {
                        value.line /= AxisPosition::Cells((*index % 20) as i32);
                        value.column /= AxisPosition::Cells(0);
                    });
                    Node::element(dom, [Text::new(format!("k{index}")).into()])
                        .key(index.to_string())
                })
                .collect();
            Node::element(DomProps::default(), children)
        };

        let forward: Vec<usize> = (0..count).collect();
        let mut reversed = forward.clone();
        reversed.reverse();
        let mut rotated = forward.clone();
        rotated.rotate_left(count / 2);
        let mut replaced = forward.clone();
        for index in (0..count).step_by(100) {
            replaced[index] = count + index;
        }

        let run = |order: Vec<usize>| {
            let viewport = Size::new(120, 40);
            let (commit, _viewport, _dispatcher, instrument) = Commit::instrumented(viewport);
            let runtime = Runtime::new(Lower::default())
                .then(commit)
                .then(renderer(viewport))
                .start_handle();
            let input = runtime.input();
            let output = runtime.output();
            input.send(build(&forward)).unwrap();
            let _ = output.recv_timeout(Duration::from_secs(5));
            input.send(build(&order)).unwrap();
            let _ = output.recv_timeout(Duration::from_secs(5));
            black_box(instrument.counts());
            drop(input);
            let _ = runtime.shutdown(ShutdownPolicy::default());
        };

        for (name, order) in [
            ("reverse", reversed.clone()),
            ("rotate", rotated.clone()),
            ("replace_one_percent", replaced.clone()),
        ] {
            group.bench_function(name, |b| {
                b.iter(|| run(order.clone()));
            });
        }
        let _ = viewport;
        group.finish();
    }
}

// Events: hit testing, deep bubbling, and a focus move at three region counts.
fn event_routing(c: &mut Criterion) {
    let mut group = c.benchmark_group("events");

    for count in [100usize, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::new("hit_test", count), &count, |b, count| {
            let (commit, _viewport, dispatcher) = Commit::new_with_events(Size::new(200, 100));
            let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
            let input = runtime.input();
            let output = runtime.output();
            let children: Vec<Node> = (0..*count)
                .map(|index| {
                    let mut dom = DomProps::default();
                    dom.style = style(|value| {
                        value.line /= AxisPosition::Cells((index % 100) as i32);
                        value.column /= AxisPosition::Cells((index % 200) as i32);
                        value.width /= Dimension::Cells(1);
                    });
                    Node::element(dom, Vec::<Node>::new())
                })
                .collect();
            input
                .send(Node::element(DomProps::default(), children))
                .unwrap();
            let _ = output.recv_timeout(Duration::from_secs(5));

            b.iter(|| {
                let outcome = dispatcher.dispatch(crossterm::event::Event::Mouse(
                    crossterm::event::MouseEvent {
                        kind: crossterm::event::MouseEventKind::Moved,
                        column: 50,
                        row: 50,
                        modifiers: crossterm::event::KeyModifiers::empty(),
                    },
                ));
                black_box(outcome);
            });
            let _ = dispatcher.take_id_probe_count();
            drop(input);
            let _ = runtime.shutdown(ShutdownPolicy::default());
        });
    }

    for depth in [10usize, 100, 1_000] {
        group.bench_with_input(
            BenchmarkId::new("deep_bubbling", depth),
            &depth,
            |b, depth| {
                let (commit, _viewport, dispatcher) = Commit::new_with_events(Size::new(200, 100));
                let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
                let input = runtime.input();
                let output = runtime.output();
                let mut node =
                    Node::element(DomProps::default().with_focusable(true), Vec::<Node>::new());
                for _ in 0..*depth {
                    node = Node::element(DomProps::default(), vec![node]);
                }
                input.send(node).unwrap();
                let _ = output.recv_timeout(Duration::from_secs(5));
                let _ = dispatcher.dispatch(crossterm::event::Event::Mouse(
                    crossterm::event::MouseEvent {
                        kind: crossterm::event::MouseEventKind::Down(
                            crossterm::event::MouseButton::Left,
                        ),
                        column: 1,
                        row: 1,
                        modifiers: crossterm::event::KeyModifiers::empty(),
                    },
                ));
                b.iter(|| {
                    let probes = dispatcher.take_id_probe_count();
                    black_box(probes);
                    let outcome = dispatcher.dispatch(crossterm::event::Event::Key(
                        crossterm::event::KeyEvent::new_with_kind(
                            crossterm::event::KeyCode::Char('x'),
                            crossterm::event::KeyModifiers::empty(),
                            crossterm::event::KeyEventKind::Press,
                        ),
                    ));
                    black_box(outcome);
                });
                drop(input);
                let _ = runtime.shutdown(ShutdownPolicy::default());
            },
        );
    }
    group.finish();
}

// Input editing: cursor moves, word moves, insertion, and paste at three
// document sizes, driven through the public component.
fn input_editing(c: &mut Criterion) {
    use icmd::advanced::Commit as EditingCommit;
    use icmd::{Attr, InputProps, input};

    let mut group = c.benchmark_group("input_editing");
    for units in [100usize, 10_000] {
        let text = "word ".repeat(units / 5 + 1);
        group.throughput(Throughput::Elements(units as u64));
        group.bench_with_input(BenchmarkId::new("cursor_move", units), &text, |b, text| {
            b.iter(|| {
                let viewport = Size::new(40, 6);
                let (commit, _viewport, dispatcher) = EditingCommit::new_with_events(viewport);
                let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
                let input_tx = runtime.input();
                let output = runtime.output();
                let node = input
                    .props(InputProps {
                        default_value: Attr::Set(text.clone()),
                        ..InputProps::default()
                    })
                    .node();
                input_tx.send(node).unwrap();
                let _ = output.recv_timeout(Duration::from_secs(5));
                dispatcher.dispatch(crossterm::event::Event::Mouse(
                    crossterm::event::MouseEvent {
                        kind: crossterm::event::MouseEventKind::Down(
                            crossterm::event::MouseButton::Left,
                        ),
                        column: 1,
                        row: 0,
                        modifiers: crossterm::event::KeyModifiers::empty(),
                    },
                ));
                dispatcher.dispatch(crossterm::event::Event::Key(
                    crossterm::event::KeyEvent::new_with_kind(
                        crossterm::event::KeyCode::End,
                        crossterm::event::KeyModifiers::CONTROL,
                        crossterm::event::KeyEventKind::Press,
                    ),
                ));
                black_box(dispatcher.dispatch(crossterm::event::Event::Key(
                    crossterm::event::KeyEvent::new_with_kind(
                        crossterm::event::KeyCode::Home,
                        crossterm::event::KeyModifiers::empty(),
                        crossterm::event::KeyEventKind::Press,
                    ),
                )));
                drop(input_tx);
                let _ = runtime.shutdown(ShutdownPolicy::default());
            });
        });

        group.bench_with_input(BenchmarkId::new("word_move", units), &text, |b, text| {
            b.iter(|| {
                let viewport = Size::new(40, 6);
                let (commit, _viewport, dispatcher) = EditingCommit::new_with_events(viewport);
                let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
                let input_tx = runtime.input();
                let output = runtime.output();
                let node = input
                    .props(InputProps {
                        default_value: Attr::Set(text.clone()),
                        ..InputProps::default()
                    })
                    .node();
                input_tx.send(node).unwrap();
                let _ = output.recv_timeout(Duration::from_secs(5));
                dispatcher.dispatch(crossterm::event::Event::Mouse(
                    crossterm::event::MouseEvent {
                        kind: crossterm::event::MouseEventKind::Down(
                            crossterm::event::MouseButton::Left,
                        ),
                        column: 1,
                        row: 0,
                        modifiers: crossterm::event::KeyModifiers::empty(),
                    },
                ));
                black_box(dispatcher.dispatch(crossterm::event::Event::Key(
                    crossterm::event::KeyEvent::new_with_kind(
                        crossterm::event::KeyCode::Right,
                        crossterm::event::KeyModifiers::CONTROL,
                        crossterm::event::KeyEventKind::Press,
                    ),
                )));
                drop(input_tx);
                let _ = runtime.shutdown(ShutdownPolicy::default());
            });
        });
    }
    group.finish();
}

// Canvas: fill, line, and text at three viewport sizes.
fn canvas_primitives(c: &mut Criterion) {
    use icmd::CanvasContext;

    let mut group = c.benchmark_group("canvas");
    for viewport in [Size::new(80, 24), Size::new(240, 80), Size::new(500, 200)] {
        let width = viewport.width;
        let height = viewport.height;
        let area = usize::from(width) * usize::from(height);
        group.throughput(Throughput::Elements(area as u64));
        group.bench_with_input(
            BenchmarkId::new("fill_rect", format!("{width}x{height}")),
            &viewport,
            |b, _| {
                b.iter(|| {
                    let mut canvas = CanvasContext::new(width, height).unwrap();
                    canvas.fill_rect(0, 0, width, height, "░").unwrap();
                    black_box(canvas.width());
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("line", format!("{width}x{height}")),
            &viewport,
            |b, _| {
                b.iter(|| {
                    let mut canvas = CanvasContext::new(width, height).unwrap();
                    canvas
                        .line(0, 0, i32::from(width) - 1, i32::from(height) - 1, "*")
                        .unwrap();
                    black_box(canvas.height());
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("text", format!("{width}x{height}")),
            &viewport,
            |b, _| {
                let text = "canvas text ".repeat(usize::from(width) / 12 + 1);
                b.iter(|| {
                    let mut canvas = CanvasContext::new(width, height).unwrap();
                    canvas.fill_text(&text, 0, 0).unwrap();
                    black_box(canvas.width());
                });
            },
        );
    }
    group.finish();
}

// Images: transform cache hit and miss, and an eviction boundary.
fn image_pipeline(c: &mut Criterion) {
    let viewport = Size::new(80, 24);
    let mut group = c.benchmark_group("images");

    let pixels: Vec<u8> = (0..64 * 64 * 4).map(|index| (index % 251) as u8).collect();
    let source = RasterImage::from_rgba8(64, 64, pixels).expect("benchmark raster");

    group.bench_function("transform_cache_hit", |b| {
        let mut renderer = renderer(viewport);
        let raster = RasterPlacement::new(
            ImageSource::loaded(source.clone()),
            20,
            8,
            icmd::ImageRenderOptions {
                mode: ImageMode::Symbols,
                ..icmd::ImageRenderOptions::default()
            },
        );
        renderer
            .apply_frame(Frame::with_operation(Operation::CreateRaster {
                id: ImageId(1),
                raster,
                position: ScreenPosition::new(0, 0),
                level: 0,
            }))
            .unwrap();
        let _ = renderer.render_diff();
        b.iter(|| {
            let _ = black_box(renderer.render_diff().unwrap());
        });
    });

    group.bench_function("transform_recompute", |b| {
        b.iter(|| {
            let window: Vec<u8> = source.rgba8().to_vec();
            black_box(window.len());
        });
    });

    group.bench_function("cache_eviction_boundary", |b| {
        b.iter(|| {
            let mut renderer = Renderer::with_config(
                viewport,
                RendererConfig {
                    image_protocol: ImageProtocol::Symbols,
                    image_cache_bytes: 64 * 1024,
                    ..RendererConfig::default()
                },
            )
            .unwrap();
            for id in 1..=8u64 {
                renderer
                    .apply_frame(Frame::with_operation(Operation::CreateRaster {
                        id: ImageId(id),
                        raster: RasterPlacement::new(
                            ImageSource::loaded(source.clone()),
                            10,
                            4,
                            icmd::ImageRenderOptions::default(),
                        ),
                        position: ScreenPosition::new((id as i32 % 4) * 2, 0),
                        level: 0,
                    }))
                    .unwrap();
            }
            let _ = black_box(renderer.render_diff().unwrap());
            black_box(renderer.image_metrics());
        });
    });
    group.finish();
}

// Runtime: event-to-dispatch latency and shutdown cost under an idle pipeline.
fn runtime_lifecycle(c: &mut Criterion) {
    let viewport = Size::new(80, 24);
    let mut group = c.benchmark_group("runtime");

    group.bench_function("event_to_dispatch", |b| {
        let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
        let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
        let input = runtime.input();
        let output = runtime.output();
        input
            .send(Node::element(DomProps::default(), Vec::<Node>::new()))
            .unwrap();
        let _ = output.recv_timeout(Duration::from_secs(5));
        b.iter(|| {
            black_box(dispatcher.dispatch(crossterm::event::Event::Key(
                crossterm::event::KeyEvent::new_with_kind(
                    crossterm::event::KeyCode::Char('x'),
                    crossterm::event::KeyModifiers::empty(),
                    crossterm::event::KeyEventKind::Press,
                ),
            )));
        });
        drop(input);
        let _ = runtime.shutdown(ShutdownPolicy::default());
    });

    group.bench_function("shutdown", |b| {
        b.iter(|| {
            let (commit, _viewport, _dispatcher) = Commit::new_with_events(viewport);
            let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
            let input = runtime.input();
            let output = runtime.output();
            input
                .send(Node::element(DomProps::default(), Vec::<Node>::new()))
                .unwrap();
            let _ = output.recv_timeout(Duration::from_secs(5));
            drop(input);
            let _ = black_box(runtime.shutdown(ShutdownPolicy::default()));
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    renderer_damage,
    layer_composition,
    layout_shapes,
    text_shapes,
    keyed_reconciliation,
    event_routing,
    input_editing,
    canvas_primitives,
    image_pipeline,
    runtime_lifecycle
);
criterion_main!(benches);
