use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use icmd::advanced::{Commit, Lower, Renderer, RendererConfig, Runtime};
use icmd::{
    Cell, CellEdit, Dimension, Frame, Image, ImageId, ImageMode, ImageProtocol, ImageRenderOptions,
    ImageSource, Node, Operation, RasterImage, RasterPlacement, ScreenPosition, Size, Text,
    TextWrap, style,
};

const VIEWPORT: Size = Size::new(240, 80);

fn cell(symbol: &str) -> Cell {
    Cell::plain(symbol).expect("benchmark glyph is valid")
}

fn renderer() -> Renderer {
    Renderer::with_config(
        VIEWPORT,
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            cell_pixel_size: Some(Size::new(8, 16)),
            ..RendererConfig::default()
        },
    )
    .expect("benchmark viewport is valid")
}

fn many_fragments() -> Renderer {
    let mut renderer = renderer();
    let image = Image::new(1, 1, cell("x")).unwrap();
    let mut operations = Vec::with_capacity(2_000);
    for id in 0..2_000u64 {
        operations.push(Operation::Create {
            id: ImageId(id + 1),
            image: image.clone(),
            position: ScreenPosition::new((id / 240) as i32, (id % 240) as i32),
            level: 0,
        });
    }
    renderer.apply_frame(Frame::new(operations)).unwrap();
    renderer.render_diff().unwrap();
    renderer
}

fn renderer_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("renderer");
    group.throughput(Throughput::Elements(1));

    group.bench_function("single_patch_2000_fragments", |b| {
        let mut renderer = many_fragments();
        let mut toggle = false;
        b.iter(|| {
            toggle = !toggle;
            renderer
                .apply_frame(Frame::with_operation(Operation::PatchCells {
                    id: ImageId(1),
                    edits: vec![CellEdit {
                        position: icmd::ImagePosition::new(0, 0),
                        cell: cell(if toggle { "a" } else { "b" }),
                    }],
                }))
                .unwrap();
            black_box(renderer.render_diff().unwrap());
        });
    });

    group.bench_function("move_fragment_2000_fragments", |b| {
        let mut renderer = many_fragments();
        let mut toggle = false;
        b.iter(|| {
            toggle = !toggle;
            renderer
                .apply_frame(Frame::with_operation(Operation::Move {
                    id: ImageId(1),
                    position: ScreenPosition::new(0, if toggle { 0 } else { 1 }),
                }))
                .unwrap();
            black_box(renderer.render_diff().unwrap());
        });
    });

    group.bench_function("dense_full_redraw", |b| {
        let mut renderer = renderer();
        renderer
            .apply_frame(Frame::with_operation(Operation::Create {
                id: ImageId(1),
                image: Image::new(240, 80, cell("x")).unwrap(),
                position: ScreenPosition::default(),
                level: 0,
            }))
            .unwrap();
        renderer.render_diff().unwrap();
        b.iter(|| {
            renderer.apply_frame(Frame::empty().invalidate()).unwrap();
            black_box(renderer.render_diff().unwrap());
        });
    });

    group.bench_function("overlap_256_layers", |b| {
        let mut renderer = renderer();
        let image = Image::new(80, 24, cell("x")).unwrap();
        let operations = (0..256u64)
            .map(|id| Operation::Create {
                id: ImageId(id + 1),
                image: image.clone(),
                position: ScreenPosition::new(16, 80),
                level: id as i32,
            })
            .collect();
        renderer.apply_frame(Frame::new(operations)).unwrap();
        renderer.render_diff().unwrap();
        b.iter(|| {
            renderer.apply_frame(Frame::empty().invalidate()).unwrap();
            black_box(renderer.render_diff().unwrap());
        });
    });

    group.bench_function("symbols_raster", |b| {
        let mut renderer = renderer();
        let pixels = (0..(64 * 64))
            .flat_map(|index| [index as u8, (index >> 2) as u8, 128, 255])
            .collect::<Vec<_>>();
        let source = RasterImage::from_rgba8(64, 64, pixels).unwrap();
        let raster = RasterPlacement::new(
            ImageSource::loaded(source),
            40,
            20,
            ImageRenderOptions {
                mode: ImageMode::Symbols,
                ..ImageRenderOptions::default()
            },
        );
        renderer
            .apply_frame(Frame::with_operation(Operation::CreateRaster {
                id: ImageId(1),
                raster,
                position: ScreenPosition::default(),
                level: 0,
            }))
            .unwrap();
        renderer.render_diff().unwrap();
        b.iter(|| {
            renderer.apply_frame(Frame::empty().invalidate()).unwrap();
            black_box(renderer.render_diff().unwrap());
        });
    });
    group.finish();
}

fn pipeline_benches(c: &mut Criterion) {
    c.bench_with_input(
        BenchmarkId::new("pipeline", "text_leaf_change"),
        &(),
        |b, _| {
            let (input, output) = Runtime::new(Lower::default())
                .then(Commit::new(VIEWPORT).0)
                .then(renderer())
                .start();
            let mut toggle = false;
            b.iter(|| {
                toggle = !toggle;
                input
                    .send(
                        Text::new(if toggle {
                            "interactive leaf a"
                        } else {
                            "interactive leaf b"
                        })
                        .into(),
                    )
                    .unwrap();
                black_box(output.recv().unwrap().unwrap());
            });
        },
    );

    c.bench_with_input(
        BenchmarkId::new("pipeline", "large_wrapped_text_change"),
        &(),
        |b, _| {
            let make_node = |suffix: &str| -> Node {
                Text::new(format!(
                    "{} {suffix}",
                    "wrapped terminal content ".repeat(100)
                ))
                .wrap(TextWrap::Soft)
                .layout_style(style(|value| value.width /= Dimension::Cells(80)))
                .into()
            };
            let first = make_node("a");
            let second = make_node("b");
            let (input, output) = Runtime::new(Lower::default())
                .then(Commit::new(VIEWPORT).0)
                .then(renderer())
                .start();
            let mut toggle = false;
            b.iter(|| {
                toggle = !toggle;
                input
                    .send(if toggle {
                        first.clone()
                    } else {
                        second.clone()
                    })
                    .unwrap();
                black_box(output.recv().unwrap().unwrap());
            });
        },
    );
}

criterion_group!(benches, renderer_benches, pipeline_benches);
criterion_main!(benches);
