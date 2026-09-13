// Deterministic property tests. They use a small xorshift generator instead of
// an external fuzzing crate so the corpus is reproducible and replayable in CI.
//
// Coverage required by the hardening plan:
// - raw frame operation sequences: validation is transactional and never panics;
// - tree shapes: within-limit trees render and over-limit trees are typed errors;
// - glyph validation over arbitrary UTF-8 never panics a public path.

fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut state = seed | 1;
    move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    }
}

mod frames {
    use super::rng;
    use icmd::{
        Cell, CellEdit, Frame, FrameError, Image, ImageId, ImagePosition, Operation, RasterImage,
        RasterPlacement, Rect, Renderer, ScreenPosition, Size,
    };

    fn cell(symbol: &str) -> Cell {
        Cell::plain(symbol).unwrap_or_else(|_| Cell::blank())
    }

    fn cells(width: usize, height: usize) -> Image {
        Image::new(width, height, Cell::blank()).expect("small surface")
    }

    fn raster() -> RasterPlacement {
        let image = RasterImage::from_rgba8(2, 2, vec![128u8; 16]).expect("rgba");
        RasterPlacement::new(icmd::ImageSource::loaded(image), 1, 1, Default::default())
    }

    fn random_operation(rng: &mut impl FnMut() -> u64, id: u64) -> Operation {
        let id = ImageId(id);
        match rng() % 11 {
            0 => Operation::Create {
                id,
                image: cells(1 + (rng() % 3) as usize, 1 + (rng() % 3) as usize),
                position: ScreenPosition::new(0, 0),
                level: 0,
            },
            1 => Operation::CreateRaster {
                id,
                raster: raster(),
                position: ScreenPosition::new(0, 0),
                level: 0,
            },
            2 => Operation::Remove { id },
            3 => Operation::Move {
                id,
                position: ScreenPosition::new((rng() % 4) as i32, (rng() % 4) as i32),
            },
            4 => Operation::SetLevel {
                id,
                level: (rng() % 3) as i32,
            },
            5 => Operation::SetOrder {
                id,
                order: rng() % 4,
            },
            6 => Operation::Replace {
                id,
                image: cells(1, 1),
            },
            7 => Operation::ReplaceRaster {
                id,
                raster: raster(),
            },
            8 => Operation::SetRasterClip {
                id,
                clip: Some(Rect::new(0, 0, 1, 1)),
            },
            9 => Operation::PatchRect {
                id,
                rect: Rect::new(0, 0, 1, 1),
                rows: vec![vec![cell("x")]],
            },
            _ => Operation::PatchCells {
                id,
                edits: vec![CellEdit {
                    position: ImagePosition::new(0, 0),
                    cell: cell("y"),
                }],
            },
        }
    }

    #[test]
    fn random_frame_sequences_never_panic_and_stay_transactional() {
        for seed in 1..200u64 {
            let mut rng = rng(seed);
            let mut renderer = Renderer::with_config(
                Size::new(6, 3),
                icmd::RendererConfig {
                    image_protocol: icmd::ImageProtocol::Symbols,
                    ..icmd::RendererConfig::default()
                },
            )
            .expect("renderer");

            for _ in 0..12 {
                let count = 1 + (rng() % 5) as usize;
                let mut operations = Vec::with_capacity(count);
                for _ in 0..count {
                    let id = 1 + rng() % 4;
                    operations.push(random_operation(&mut rng, id));
                }
                // The outcome is either success or a typed error. The point of
                // the test is that no input reaches a panic.
                match renderer.apply_frame(Frame::new(operations)) {
                    Ok(()) => {}
                    Err(error) => {
                        assert!(
                            matches!(
                                error,
                                FrameError::DuplicateImage(_)
                                    | FrameError::UnknownImage(_)
                                    | FrameError::InvalidPatch { .. }
                                    | FrameError::WrongSurface { .. }
                                    | FrameError::SurfaceTooLarge { .. }
                                    | FrameError::AllocationFailed { .. }
                                    | FrameError::UnsupportedProtocol { .. }
                                    | FrameError::Config { .. }
                                    | FrameError::OutputTooLarge { .. }
                            ),
                            "unexpected error kind: {error:?}"
                        );
                    }
                }
            }

            // The renderer must still be usable after any sequence.
            let _ = renderer.render_diff();
        }
    }

    #[test]
    fn a_valid_frame_after_a_rejected_one_still_renders() {
        let mut renderer = Renderer::with_config(
            Size::new(6, 3),
            icmd::RendererConfig {
                image_protocol: icmd::ImageProtocol::Symbols,
                ..icmd::RendererConfig::default()
            },
        )
        .expect("renderer");
        // Unknown id: rejected.
        assert!(
            renderer
                .apply_frame(Frame::new(vec![Operation::Remove { id: ImageId(9) }]))
                .is_err()
        );
        // A valid create + patch still works and produces output.
        renderer
            .apply_frame(Frame::new(vec![
                Operation::Create {
                    id: ImageId(1),
                    image: cells(2, 1),
                    position: ScreenPosition::new(0, 0),
                    level: 0,
                },
                Operation::PatchCells {
                    id: ImageId(1),
                    edits: vec![CellEdit {
                        position: ImagePosition::new(0, 0),
                        cell: cell("Z"),
                    }],
                },
            ]))
            .expect("valid frame");
        let frame = renderer.render_diff().expect("render").expect("output");
        assert!(frame.contains('Z'), "frame was {frame:?}");
    }
}

mod trees {
    use super::rng;
    use icmd::{DomProps, Node, ResourceLimits, RuntimeError};

    fn leaf() -> Node {
        Node::element(DomProps::default(), Vec::<Node>::new())
    }

    // The generator reports the shape it built, because node internals are
    // deliberately opaque to external code.
    struct Shape {
        node: Node,
        nodes: usize,
        depth: usize,
    }

    fn random_tree(rng: &mut impl FnMut() -> u64, depth: usize, budget: &mut usize) -> Shape {
        if depth == 0 || *budget == 0 {
            return Shape {
                node: leaf(),
                nodes: 1,
                depth: 1,
            };
        }
        *budget -= 1;
        let count = 1 + (rng() % 3) as usize;
        let mut children = Vec::new();
        let mut nodes = 1usize;
        let mut deepest = 0usize;
        for _ in 0..count {
            if *budget == 0 {
                break;
            }
            let child = random_tree(rng, depth - 1, budget);
            nodes += child.nodes;
            deepest = deepest.max(child.depth);
            children.push(child.node);
        }
        Shape {
            node: Node::element(DomProps::default(), children),
            nodes,
            depth: 1 + deepest,
        }
    }

    #[test]
    fn generated_trees_either_render_or_return_a_typed_error() {
        // One runtime per generated shape: starting a pipeline is the expensive
        // part of this test, so the corpus stays small but the shapes varied.
        for seed in 1..10u64 {
            let mut rng = rng(seed);
            let mut budget = 12;
            let shape = random_tree(&mut rng, 4, &mut budget);

            // Configure limits exactly at the generated shape; it must render.
            let exact = ResourceLimits {
                max_nodes: shape.nodes,
                max_tree_depth: shape.depth,
                ..ResourceLimits::default()
            };
            assert!(
                render_with(exact, shape.node.clone()).is_ok(),
                "seed {seed}: a tree exactly at its limits must render"
            );

            // One node below its size must be a typed rejection, never a panic.
            // Only meaningful when the tree has more than one node.
            if shape.nodes > 1 {
                let tight = ResourceLimits {
                    max_nodes: shape.nodes - 1,
                    max_tree_depth: shape.depth,
                    ..ResourceLimits::default()
                };
                match render_with(tight, shape.node) {
                    Err(RuntimeError::Lower(icmd::LowerError::TreeTooLarge { .. })) => {}
                    other => panic!("seed {seed}: expected TreeTooLarge, got {other:?}"),
                }
            }
        }
    }

    fn render_with(limits: ResourceLimits, node: Node) -> Result<String, RuntimeError> {
        use icmd::{Commit, Lower, Renderer, Runtime, ShutdownPolicy, Size};
        use std::time::Duration;

        let viewport = Size::new(8, 2);
        let (commit, _) = Commit::new(viewport);
        let runtime = Runtime::new(Lower::with_limits(limits))
            .then(commit)
            .then(Renderer::new(viewport).unwrap())
            .start_handle();
        let input = runtime.input();
        let output = runtime.output();
        let errors = runtime.errors();
        input.send(node).unwrap();
        // Wait on both outcomes at once: a rejection arrives on the error
        // channel, a success on the output channel.
        let result = crossbeam_channel::select! {
            recv(errors) -> error => Err(error.unwrap_or(RuntimeError::StageClosed {
                stage: icmd::Stage::Lower,
            })),
            recv(output) -> frame => match frame {
                Ok(Ok(frame)) => Ok(frame),
                Ok(Err(error)) => Err(RuntimeError::Frame(error)),
                Err(_) => Err(RuntimeError::StageClosed { stage: icmd::Stage::Renderer }),
            },
            default(Duration::from_secs(5)) => Err(RuntimeError::StageClosed {
                stage: icmd::Stage::Renderer,
            }),
        };
        drop(input);
        let _ = runtime.shutdown(ShutdownPolicy::default());
        result
    }
}

// Glyph validation over arbitrary UTF-8 must never panic a public path.
#[test]
fn arbitrary_utf8_strings_never_panic_glyph_validation() {
    let mut rng = rng(0x5eed);
    for _ in 0..2_000 {
        let len = (rng() % 8) as usize;
        let candidate: String = (0..len)
            .map(|_| char::from_u32((rng() % 0x2FFF) as u32 + 1).unwrap_or('x'))
            .collect();
        let _ = icmd::Cell::plain(candidate.as_str());
        let _ = icmd::Fill::new(candidate.as_str());
        let _ = icmd::ScrollbarGlyph::new(candidate.as_str());
    }
}
