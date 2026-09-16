// Native raster protocol tests require the `native-raster` feature; without
// it the renderer rejects those protocols before any worker starts.
#![cfg(feature = "native-raster")]

use std::{sync::Arc, thread, time::Duration};

use icmd::advanced::{Commit, Lower, Renderer, RendererConfig, Runtime};
use icmd::widgets::raster_image;
use icmd::{
    Attr, Cell, Component, Dimension, DomProps, Frame, Image, ImageMode, ImageProtocol,
    ImageSource, ImageUpdatePolicy, Layout, Node, Operation, RasterImage, RasterImageError,
    RasterPlacement, Size, Style, canvas, ui,
};

fn render(node: Node) -> String {
    let (commit, _) = icmd::advanced::Commit::new(Size::new(8, 4));
    let (input, output) = Runtime::new(icmd::advanced::Lower::default())
        .then(commit)
        .then(
            Renderer::with_config(
                Size::new(8, 4),
                RendererConfig {
                    image_protocol: ImageProtocol::Symbols,
                    ..RendererConfig::default()
                },
            )
            .unwrap(),
        )
        .start();
    input.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap()
}

// A generated on-disk image fixture. Tests must not depend on demo media
// shipped in the repository, which the published package deliberately excludes.
fn asset_path() -> std::path::PathBuf {
    use std::sync::OnceLock;
    static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("icmd-raster-asset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fixture.jpg");
        if !path.exists() {
            let pixels: Vec<u8> = (0..(8 * 8 * 3))
                .map(|index| ((index * 7) % 256) as u8)
                .collect();
            let file = std::fs::File::create(&path).unwrap();
            let encoder = image::codecs::jpeg::JpegEncoder::new(file);
            image::ImageEncoder::write_image(
                encoder,
                &pixels,
                8,
                8,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        }
        path
    })
    .clone()
}

fn asset() -> RasterImage {
    RasterImage::from_rgba8(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    )
    .unwrap()
}

#[test]
fn raster_assets_validate_and_clone_without_copying_pixels() {
    assert!(matches!(
        RasterImage::from_rgba8(2, 2, vec![0; 3]),
        Err(RasterImageError::InvalidLength { .. })
    ));
    let source = asset();
    assert_eq!(source, source.clone());
    assert_eq!(source.rgba8().len(), 16);
}

#[test]
fn image_component_has_a_symbol_fallback() {
    let source = asset();
    let node = ui! { <raster_image src={source} width=4 height=2 mode={ImageMode::Symbols} /> };
    assert!(!render(node).is_empty());
}

#[test]
fn canvas_images_keep_later_cell_draws_on_top() {
    let source = asset();
    let node = canvas
        .extra(move |props| {
            props.width /= 4;
            props.height /= 2;
            let source = source.clone();
            props.draw /= Arc::new(move |drawing| {
                drawing.draw_image(&source, 0, 0, 4, 2).unwrap();
                drawing.set(1, 0, "X").unwrap();
            });
        })
        .node();
    assert!(render(node).contains('X'));
}

#[test]
fn native_raster_operations_are_retained() {
    for (protocol, marker) in [
        (ImageProtocol::Kitty, "\x1b_G"),
        (ImageProtocol::Sixel, "\x1bP"),
        (ImageProtocol::Iterm2, "\x1b]1337"),
    ] {
        let mut renderer = Renderer::with_config(
            Size::new(4, 2),
            RendererConfig {
                image_protocol: protocol,
                ..RendererConfig::default()
            },
        )
        .unwrap();
        renderer
            .apply_frame(Frame::new(vec![Operation::CreateRaster {
                id: icmd::ImageId(1),
                raster: RasterPlacement::new(asset(), 2, 1, Default::default()),
                position: Default::default(),
                level: 0,
            }]))
            .unwrap();
        let frame = renderer.render_diff().unwrap().unwrap();
        assert!(frame.contains(marker), "{protocol:?}: {frame:?}");
    }
}

#[test]
fn transparent_cells_do_not_emit_native_tiles() {
    let transparent = RasterImage::from_rgba8(1, 1, vec![255, 0, 0, 0]).unwrap();
    let mut renderer = Renderer::with_config(
        Size::new(2, 1),
        RendererConfig {
            image_protocol: ImageProtocol::Kitty,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(transparent, 1, 1, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let frame = renderer.render_diff().unwrap().unwrap();
    assert!(!frame.contains("\x1b_G"), "{frame:?}");
}

#[test]
fn kitty_repartitions_an_occluded_corner_without_uploading_again() {
    let mut renderer = Renderer::with_config(
        Size::new(4, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Kitty,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(asset(), 2, 2, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let first = renderer.render_diff().unwrap().unwrap();
    assert!(first.contains("a=t,"), "{first:?}");

    let overlay = Image::from_rows(vec![vec![Cell::plain("X").unwrap()]]).unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::Create {
            id: icmd::ImageId(2),
            image: overlay,
            position: Default::default(),
            level: 1,
        }]))
        .unwrap();
    let second = renderer.render_diff().unwrap().unwrap();
    assert!(second.contains("a=d,d=i"), "{second:?}");
    assert!(second.contains(",p="), "{second:?}");
    assert!(!second.contains("d=p"), "{second:?}");
    assert!(second.contains("a=p,"), "{second:?}");
    assert!(!second.contains("a=t,"), "{second:?}");
}

#[test]
fn kitty_moves_a_tile_by_updating_its_placement() {
    let mut renderer = Renderer::with_config(
        Size::new(5, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Kitty,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(asset(), 2, 1, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    renderer.render_diff().unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::Move {
            id: icmd::ImageId(1),
            position: icmd::ScreenPosition::new(0, 1),
        }]))
        .unwrap();
    let moved = renderer.render_diff().unwrap().unwrap();
    assert!(moved.contains("a=p,"), "{moved:?}");
    assert!(!moved.contains("a=t,"), "{moved:?}");
    assert!(!moved.contains("a=d,d=p"), "{moved:?}");
}

#[test]
fn kitty_resizes_a_tile_by_reusing_its_placement() {
    let mut renderer = Renderer::with_config(
        Size::new(6, 3),
        RendererConfig {
            image_protocol: ImageProtocol::Kitty,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    let source = asset();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(source.clone(), 2, 1, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let first = renderer.render_diff().unwrap().unwrap();
    let image = first
        .split("a=t,")
        .nth(1)
        .and_then(|value| value.split(';').next())
        .and_then(|value| value.split(',').find_map(|field| field.strip_prefix("i=")))
        .and_then(|value| value.parse::<u32>().ok())
        .expect("first Kitty image id");
    let placement = first
        .split("a=p,")
        .nth(1)
        .and_then(|value| value.split(',').find_map(|field| field.strip_prefix("p=")))
        .and_then(|value| value.parse::<u32>().ok())
        .expect("first Kitty placement id");

    renderer
        .apply_frame(Frame::new(vec![Operation::ReplaceRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(source, 3, 1, Default::default()),
        }]))
        .unwrap();
    let resized = renderer.render_diff().unwrap().unwrap();
    assert!(resized.contains("a=p,"), "{resized:?}");
    assert!(resized.contains(&format!("p={placement}")), "{resized:?}");
    assert!(!resized.contains("a=d,d=i"), "{resized:?}");
    assert!(resized.contains("a=t,"), "{resized:?}");
    assert!(
        resized.contains("a=t,f=32") && resized.contains(&format!("i={image},m=")),
        "{resized:?}"
    );
}

#[test]
fn kitty_reasserts_existing_placements_after_a_full_clear() {
    let mut renderer = Renderer::with_config(
        Size::new(5, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Kitty,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(asset(), 2, 1, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let first = renderer.render_diff().unwrap().unwrap();
    let image = first
        .split("a=t,")
        .nth(1)
        .and_then(|value| value.split(';').next())
        .and_then(|value| value.split(',').find_map(|field| field.strip_prefix("i=")))
        .and_then(|value| value.parse::<u32>().ok())
        .expect("first Kitty image id");
    let placement = first
        .split("a=p,")
        .nth(1)
        .and_then(|value| value.split(',').find_map(|field| field.strip_prefix("p=")))
        .and_then(|value| value.parse::<u32>().ok())
        .expect("first Kitty placement id");
    renderer.apply_frame(Frame::empty().invalidate()).unwrap();
    let second = renderer.render_diff().unwrap().unwrap();
    assert!(second.contains("a=p,"), "{second:?}");
    assert!(second.contains(&format!("p={placement}")), "{second:?}");
    assert!(second.contains("a=t,"), "{second:?}");
    assert!(second.contains(&format!("i={image},m=")), "{second:?}");
    assert!(!second.contains("a=d,d=i"), "{second:?}");
}

#[test]
fn kitty_resize_retransmits_and_restores_existing_ids() {
    let mut renderer = Renderer::with_config(
        Size::new(5, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Kitty,
            cell_pixel_size: Some(Size::new(1, 1)),
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(asset(), 2, 1, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let first = renderer.render_diff().unwrap().unwrap();
    let image = first
        .split("a=t,")
        .nth(1)
        .and_then(|value| value.split(';').next())
        .and_then(|value| value.split(',').find_map(|field| field.strip_prefix("i=")))
        .and_then(|value| value.parse::<u32>().ok())
        .expect("first Kitty image id");
    let placement = first
        .split("a=p,")
        .nth(1)
        .and_then(|value| value.split(',').find_map(|field| field.strip_prefix("p=")))
        .and_then(|value| value.parse::<u32>().ok())
        .expect("first Kitty placement id");

    renderer
        .apply_frame(Frame::empty().resize(Size::new(8, 3)))
        .unwrap();
    let resized = renderer.render_diff().unwrap().unwrap();
    assert!(resized.contains("\x1b[2J"), "{resized:?}");
    assert!(resized.contains("a=t,"), "{resized:?}");
    assert!(resized.contains(&format!("i={image},m=")), "{resized:?}");
    assert!(resized.contains(&format!("p={placement}")), "{resized:?}");
    assert!(!resized.contains("a=d,d=i"), "{resized:?}");
    assert!(!resized.contains("a=d,d=I"), "{resized:?}");
}

#[test]
fn kitty_multipart_uploads_have_one_metadata_header() {
    let mut pixels = Vec::with_capacity(40 * 40 * 4);
    let mut value = 0x1234_5678_u32;
    for _ in 0..(40 * 40) {
        value = value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        pixels.extend_from_slice(&value.to_le_bytes());
    }
    let source = RasterImage::from_rgba8(40, 40, pixels).unwrap();
    let mut renderer = Renderer::with_config(
        Size::new(40, 40),
        RendererConfig {
            image_protocol: ImageProtocol::Kitty,
            cell_pixel_size: Some(Size::new(1, 1)),
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(source, 40, 40, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let frame = renderer.render_diff().unwrap().unwrap();
    assert!(frame.matches("a=t,").count() == 1, "{frame:?}");
    assert!(frame.contains("m=1"), "{frame:?}");
}

#[test]
fn lazy_file_sources_load_only_when_reaching_the_prefetch_region() {
    let path = asset_path();
    let source = ImageSource::file(path);
    let mut renderer = Renderer::with_config(
        Size::new(4, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(source.clone(), 2, 1, Default::default()),
            position: icmd::ScreenPosition::new(20, 0),
            level: 0,
        }]))
        .unwrap();
    let first = renderer.render_diff().unwrap().unwrap();
    assert!(!first.contains("×"), "{first:?}");
    renderer
        .apply_frame(Frame::with_operation(Operation::Move {
            id: icmd::ImageId(1),
            position: icmd::ScreenPosition::new(0, 0),
        }))
        .unwrap();
    let pending = renderer.render_diff().unwrap().unwrap();
    assert!(pending.contains("…"), "{pending:?}");
    // Wait for the background decode to be reported. The completion signal is
    // `render_diff` returning a frame, not a fixed sleep budget: the loop polls
    // a channel-like condition with a generous bound so a loaded machine cannot
    // turn a correct load into a failure.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if let Some(frame) = renderer.render_diff().unwrap()
            && !frame.contains("…")
        {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("lazy source did not finish loading within 20s");
}

#[test]
fn commit_retains_lazy_rasters_in_the_prefetch_margin_without_painting_them() {
    let source = ImageSource::file(asset_path());
    let mut root_props = DomProps::default();
    root_props.style = Style {
        width: Attr::Set(Dimension::Cells(4)),
        height: Attr::Set(Dimension::Cells(2)),
        layout: Attr::Set(Layout::Vertical),
        ..Style::default()
    };
    let mut spacer_props = DomProps::default();
    spacer_props.style = Style {
        width: Attr::Set(Dimension::Cells(4)),
        height: Attr::Set(Dimension::Cells(3)),
        ..Style::default()
    };
    let root = Node::element(
        root_props,
        [
            Node::element(spacer_props, []),
            Node::raster(RasterPlacement::new(source, 2, 1, Default::default())),
        ],
    );
    let (commit, _) = Commit::new(Size::new(4, 2));
    let (input, output) = Runtime::new(Lower::default()).then(commit).start();
    input.send(root).unwrap();
    let frame = output.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(input);
    assert!(
        frame
            .operations
            .iter()
            .any(|operation| { matches!(operation, Operation::CreateRaster { .. }) })
    );
    assert!(frame.operations.iter().any(|operation| {
        matches!(operation, Operation::SetRasterClip { clip: Some(clip), .. } if clip.width == 0 && clip.height == 0)
    }));
}

#[test]
fn lazy_image_without_dimensions_renders_an_error_placeholder() {
    let path = asset_path();
    let node = ui! { <raster_image src={ImageSource::file(path)} width=2 /> };
    let frame = render(node);
    assert!(frame.contains("×"), "{frame:?}");
}

#[test]
fn direct_lazy_raster_without_dimensions_fails_without_loading() {
    let path = asset_path();
    let mut renderer = Renderer::with_config(
        Size::new(3, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(ImageSource::file(path), 0, 2, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let frame = renderer.render_diff().unwrap().unwrap();
    assert!(frame.contains("×"), "{frame:?}");
}

#[test]
fn missing_lazy_files_transition_from_loading_to_error() {
    let source = ImageSource::file(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("does-not-exist.jpg"),
    );
    let mut renderer = Renderer::with_config(
        Size::new(4, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(source, 2, 1, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    assert!(renderer.render_diff().unwrap().unwrap().contains("…"));
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(10));
        if let Some(frame) = renderer.render_diff().unwrap()
            && frame.contains("×")
        {
            return;
        }
    }
    panic!("missing lazy source did not report an error");
}

#[test]
fn kitty_cache_eviction_releases_terminal_image_data() {
    let mut renderer = Renderer::with_config(
        Size::new(4, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Kitty,
            image_cache_bytes: 1,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(asset(), 2, 1, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    renderer.render_diff().unwrap();
    renderer
        .apply_frame(Frame::new(vec![
            Operation::Remove {
                id: icmd::ImageId(1),
            },
            Operation::CreateRaster {
                id: icmd::ImageId(2),
                raster: RasterPlacement::new(asset(), 2, 1, Default::default()),
                position: Default::default(),
                level: 0,
            },
        ]))
        .unwrap();
    let frame = renderer.render_diff().unwrap().unwrap();
    assert!(frame.contains("a=d,d=I"), "{frame:?}");
}

#[test]
fn renderer_shutdown_emits_owned_kitty_cleanup() {
    let renderer = Renderer::with_config(
        Size::new(4, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Kitty,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    let (input, output) = Runtime::new(renderer).start();
    input
        .send(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(asset(), 2, 1, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let _ = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    drop(input);
    let mut cleanup = None;
    while let Ok(Ok(frame)) = output.recv_timeout(Duration::from_secs(1)) {
        if frame.contains("a=d,d=I") {
            cleanup = Some(frame);
            break;
        }
    }
    assert!(cleanup.is_some());
}

#[test]
fn adaptive_sixel_uses_symbols_until_the_scene_settles() {
    let mut renderer = Renderer::with_config(
        Size::new(5, 2),
        RendererConfig {
            image_protocol: ImageProtocol::Sixel,
            image_update_policy: ImageUpdatePolicy::Adaptive,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster: RasterPlacement::new(asset(), 2, 1, Default::default()),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    assert!(renderer.render_diff().unwrap().unwrap().contains("\x1bP"));
    renderer
        .apply_frame(Frame::new(vec![Operation::Move {
            id: icmd::ImageId(1),
            position: icmd::ScreenPosition::new(0, 1),
        }]))
        .unwrap();
    let preview = renderer.render_diff().unwrap().unwrap();
    assert!(!preview.contains("\x1bP"), "{preview:?}");
    thread::sleep(Duration::from_millis(90));
    assert!(renderer.render_diff().unwrap().unwrap().contains("\x1bP"));
}

// SAF-06: frame validation is transactional and surface-kind aware. Every
// invalid batch must be rejected with a precise error and leave the renderer
// exactly as it was.
// Drive the renderer directly: frame validation is its contract, so the test
// submits raw operation batches rather than lowering a widget tree.
fn validation_pipeline() -> (
    crossbeam_channel::Sender<Frame>,
    crossbeam_channel::Receiver<Result<String, icmd::advanced::FrameError>>,
) {
    let viewport = Size::new(8, 4);
    let runtime = Runtime::new(
        Renderer::with_config(
            viewport,
            RendererConfig {
                image_protocol: ImageProtocol::Symbols,
                ..RendererConfig::default()
            },
        )
        .unwrap(),
    )
    .start_handle();
    (runtime.input(), runtime.output())
}

fn blank_cells() -> Image {
    Image::new(2, 2, Cell::plain(" ").unwrap()).unwrap()
}

#[test]
fn cell_patches_targeting_a_raster_surface_are_rejected() {
    let (input, output) = validation_pipeline();
    let raster = RasterPlacement::new(ImageSource::loaded(asset()), 2, 2, Default::default());
    input
        .send(Frame::new(vec![Operation::CreateRaster {
            id: icmd::ImageId(1),
            raster,
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    let invalid = Frame::new(vec![
        Operation::PatchCells {
            id: icmd::ImageId(1),
            edits: vec![icmd::CellEdit {
                position: icmd::ImagePosition::new(0, 0),
                cell: Cell::plain("x").unwrap(),
            }],
        },
        Operation::PatchRect {
            id: icmd::ImageId(1),
            rect: icmd::Rect::new(0, 0, 1, 1),
            rows: vec![vec![Cell::plain("x").unwrap()]],
        },
    ]);
    input.send(invalid).unwrap();
    let error = output
        .recv_timeout(Duration::from_secs(1))
        .expect("expected a validation error")
        .expect_err("a wrong-kind patch must not render");
    assert!(
        matches!(
            error,
            icmd::advanced::FrameError::WrongSurface {
                operation: 0,
                expected: icmd::advanced::SurfaceKind::Cells,
                actual: icmd::advanced::SurfaceKind::Raster,
                ..
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn raster_clips_targeting_a_cell_surface_are_rejected() {
    let (input, output) = validation_pipeline();
    input
        .send(Frame::new(vec![Operation::Create {
            id: icmd::ImageId(1),
            image: blank_cells(),
            position: Default::default(),
            level: 0,
        }]))
        .unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    input
        .send(Frame::new(vec![Operation::SetRasterClip {
            id: icmd::ImageId(1),
            clip: Some(icmd::Rect::new(0, 0, 1, 1)),
        }]))
        .unwrap();
    let error = output
        .recv_timeout(Duration::from_secs(1))
        .expect("expected a validation error")
        .expect_err("a raster clip on cells must not render");
    assert!(
        matches!(
            error,
            icmd::advanced::FrameError::WrongSurface {
                operation: 0,
                expected: icmd::advanced::SurfaceKind::Raster,
                actual: icmd::advanced::SurfaceKind::Cells,
                ..
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn use_after_remove_and_duplicate_create_report_operation_indexes() {
    let (input, output) = validation_pipeline();
    input
        .send(Frame::new(vec![
            Operation::Create {
                id: icmd::ImageId(1),
                image: blank_cells(),
                position: Default::default(),
                level: 0,
            },
            Operation::Remove {
                id: icmd::ImageId(1),
            },
            Operation::Move {
                id: icmd::ImageId(1),
                position: Default::default(),
            },
        ]))
        .unwrap();
    let error = output
        .recv_timeout(Duration::from_secs(1))
        .expect("expected a validation error")
        .expect_err("use after remove must not render");
    assert!(
        matches!(error, icmd::advanced::FrameError::UnknownImage(id) if id == icmd::ImageId(1)),
        "got {error:?}"
    );

    let (input, output) = validation_pipeline();
    input
        .send(Frame::new(vec![
            Operation::Create {
                id: icmd::ImageId(2),
                image: blank_cells(),
                position: Default::default(),
                level: 0,
            },
            Operation::Create {
                id: icmd::ImageId(2),
                image: blank_cells(),
                position: Default::default(),
                level: 0,
            },
        ]))
        .unwrap();
    let error = output
        .recv_timeout(Duration::from_secs(1))
        .expect("expected a validation error")
        .expect_err("a duplicate create must not render");
    assert!(
        matches!(error, icmd::advanced::FrameError::DuplicateImage(id) if id == icmd::ImageId(2)),
        "got {error:?}"
    );
}

#[test]
fn an_invalid_batch_leaves_the_previous_frame_intact() {
    // Create a cell surface, paint it, then submit a batch that is valid until
    // its last (wrong-kind) operation. Nothing from that batch may be applied.
    let (input, output) = validation_pipeline();
    input
        .send(Frame::new(vec![
            Operation::Create {
                id: icmd::ImageId(3),
                image: blank_cells(),
                position: Default::default(),
                level: 0,
            },
            Operation::PatchCells {
                id: icmd::ImageId(3),
                edits: vec![icmd::CellEdit {
                    position: icmd::ImagePosition::new(0, 0),
                    cell: Cell::plain("A").unwrap(),
                }],
            },
        ]))
        .unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(first.contains('A'));

    // The valid create is followed by an operation for the wrong surface kind.
    let raster = RasterPlacement::new(ImageSource::loaded(asset()), 2, 2, Default::default());
    input
        .send(Frame::new(vec![
            Operation::Create {
                id: icmd::ImageId(4),
                image: blank_cells(),
                position: Default::default(),
                level: 0,
            },
            Operation::SetRasterClip {
                id: icmd::ImageId(4),
                clip: Some(icmd::Rect::new(0, 0, 1, 1)),
            },
        ]))
        .unwrap();
    let error = output
        .recv_timeout(Duration::from_secs(1))
        .expect("expected a validation error")
        .expect_err("the batch must be rejected as a whole");
    assert!(matches!(
        error,
        icmd::advanced::FrameError::WrongSurface { .. }
    ));

    // A later valid patch on the original surface still works, proving the
    // rejected batch did not partially mutate retained state.
    input
        .send(Frame::new(vec![Operation::PatchCells {
            id: icmd::ImageId(3),
            edits: vec![icmd::CellEdit {
                position: icmd::ImagePosition::new(1, 1),
                cell: Cell::plain("B").unwrap(),
            }],
        }]))
        .unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(frame.contains('B'));

    // Re-creating id 4 must succeed. If the rejected batch had been applied
    // even partially, its id 4 would still exist and this would be a duplicate.
    input
        .send(Frame::new(vec![
            Operation::Create {
                id: icmd::ImageId(4),
                image: blank_cells(),
                position: Default::default(),
                level: 0,
            },
            Operation::PatchCells {
                id: icmd::ImageId(4),
                edits: vec![icmd::CellEdit {
                    position: icmd::ImagePosition::new(0, 0),
                    cell: Cell::plain("C").unwrap(),
                }],
            },
        ]))
        .unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(frame.contains('C'), "the re-created surface must render");
    let _ = raster;
}
