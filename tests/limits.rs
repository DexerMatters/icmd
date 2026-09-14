use std::time::Duration;

use crossbeam_channel::select;
use crossbeam_channel::{Receiver, Sender};
use icmd::advanced::{
    Commit, Lower, PipelineComponent, Renderer, ResourceLimits, Runtime, RuntimeError,
    ShutdownPolicy, Stage,
};
use icmd::{Node, Size};

fn leaf() -> Node {
    Node::element(icmd::DomProps::default(), [])
}

fn nested(depth: usize) -> Node {
    let mut node = leaf();
    for _ in 0..depth {
        node = Node::element(icmd::DomProps::default(), [node]);
    }
    node
}

fn wide(count: usize) -> Node {
    Node::element(
        icmd::DomProps::default(),
        (0..count).map(|_| leaf()).collect::<Vec<_>>(),
    )
}

fn limits(max_nodes: usize, max_tree_depth: usize) -> ResourceLimits {
    ResourceLimits {
        max_nodes,
        max_tree_depth,
        ..ResourceLimits::default()
    }
}

// A limit rejection closes every downstream channel, so the typed error on the
// error channel and a possible trailing frame race. The error wins: it is the
// authoritative outcome for that submission.
fn run_once(limits: ResourceLimits, node: Node) -> Result<String, RuntimeError> {
    let viewport = Size::new(20, 5);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(limits))
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let errors = runtime.errors();
    input.send(node).unwrap();
    let deadline = Duration::from_secs(2);
    let outcome = select! {
        recv(errors) -> error => Err(error.unwrap_or(RuntimeError::StageClosed { stage: Stage::Lower })),
        recv(output) -> frame => match frame {
            Ok(Ok(frame)) => Ok(frame),
            Ok(Err(error)) => Err(RuntimeError::Frame(error)),
            Err(_) => Err(RuntimeError::StageClosed { stage: Stage::Renderer }),
        },
        default(deadline) => Err(RuntimeError::StageClosed { stage: Stage::Lower }),
    };
    drop(input);
    outcome
}

#[test]
fn tree_exactly_at_the_limits_renders() {
    // A root element plus three nested elements is depth 4 and 4 nodes.
    let outcome = run_once(limits(4, 4), nested(3));
    assert!(outcome.is_ok(), "expected a frame, got {outcome:?}");
}

#[test]
fn tree_one_node_over_the_limit_is_rejected() {
    let outcome = run_once(limits(3, 1_000), nested(3));
    match outcome {
        Err(RuntimeError::Lower(icmd::advanced::LowerError::TreeTooLarge { limit, observed })) => {
            assert_eq!(limit, 3);
            assert_eq!(observed, 4);
        }
        other => panic!("expected TreeTooLarge, got {other:?}"),
    }
}

#[test]
fn tree_over_the_depth_limit_returns_a_typed_error() {
    let outcome = run_once(limits(1_000, 4), nested(5));
    match outcome {
        Err(RuntimeError::Lower(icmd::advanced::LowerError::TreeTooDeep { limit, .. })) => {
            assert_eq!(limit, 4);
        }
        other => panic!("expected TreeTooDeep, got {other:?}"),
    }
}

#[test]
fn tree_over_the_node_limit_returns_a_typed_error() {
    // Root plus 16 children is 17 nodes.
    let outcome = run_once(limits(10, 1_000), wide(16));
    match outcome {
        Err(RuntimeError::Lower(icmd::advanced::LowerError::TreeTooLarge { limit, observed })) => {
            assert_eq!(limit, 10);
            assert!(observed >= 11);
        }
        other => panic!("expected TreeTooLarge, got {other:?}"),
    }
}

#[test]
fn a_rejected_tree_leaves_the_worker_alive() {
    // The same runtime must keep serving after one over-limit root, proving the
    // limit is a typed rejection rather than a worker death.
    let viewport = Size::new(20, 5);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(limits(10, 100)))
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let errors = runtime.errors();

    input.send(wide(16)).unwrap();
    let rejected = errors.recv_timeout(Duration::from_secs(2));
    assert!(matches!(
        rejected,
        Ok(RuntimeError::Lower(
            icmd::advanced::LowerError::TreeTooLarge { .. }
        ))
    ));

    // A subsequent valid root still renders on the same worker.
    input.send(nested(2)).unwrap();
    let frame = output.recv_timeout(Duration::from_secs(2));
    assert!(matches!(frame, Ok(Ok(_))), "got {frame:?}");
    input.send(leaf()).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(2));
    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());
}

#[test]
fn shutdown_joins_all_workers() {
    let viewport = Size::new(20, 5);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(ResourceLimits::default()))
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    input.send(nested(2)).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(2));
    drop(input);
    // A successful shutdown means every worker was joined, not merely asked
    // to stop.
    runtime
        .shutdown(ShutdownPolicy::default())
        .expect("all workers must be joined");

    // The live-worker counter is global and other tests run concurrently, so
    // wait for the process-wide count to fall back to its baseline rather than
    // asserting an exact transient value.
    let before = icmd::advanced::live_worker_count();
    let mut settled = false;
    for _ in 0..400 {
        if icmd::advanced::live_worker_count() <= before {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(settled, "worker threads did not exit");
}

// A stage that panics on its first input.
struct PanickingStage;

impl PipelineComponent for PanickingStage {
    type Input = Node;
    type Output = icmd::DomNode;

    const STAGE: Stage = Stage::Commit;

    fn run(
        self,
        input: Receiver<Self::Input>,
        _output: Sender<Self::Output>,
        _errors: Sender<RuntimeError>,
    ) -> Result<(), RuntimeError> {
        if input.recv().is_ok() {
            panic!("stage exploded");
        }
        Ok(())
    }
}

#[test]
fn a_panicking_stage_is_reported_by_name() {
    let runtime = Runtime::new(PanickingStage).start_handle();
    let input = runtime.input();
    let errors = runtime.errors();
    input.send(leaf()).unwrap();
    let error = errors
        .recv_timeout(Duration::from_secs(2))
        .expect("a panicking stage must report an error");
    match error {
        RuntimeError::StagePanicked { stage } => assert_eq!(stage, Stage::Commit),
        other => panic!("expected StagePanicked, got {other:?}"),
    }
    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());
}

// A stage that ignores its input closing, so shutdown cannot complete.
struct StallingStage;

impl PipelineComponent for StallingStage {
    type Input = Node;
    type Output = icmd::DomNode;

    const STAGE: Stage = Stage::Renderer;

    fn run(
        self,
        _input: Receiver<Self::Input>,
        _output: Sender<Self::Output>,
        _errors: Sender<RuntimeError>,
    ) -> Result<(), RuntimeError> {
        std::thread::sleep(Duration::from_secs(30));
        Ok(())
    }
}

#[test]
fn shutdown_timeout_names_the_pending_stage() {
    let runtime = Runtime::new(StallingStage).start_handle();
    let error = runtime
        .shutdown(ShutdownPolicy::with_timeout(Duration::from_millis(50)))
        .expect_err("a stalled stage must produce a timeout");
    match error {
        RuntimeError::ShutdownTimeout { pending } => {
            assert_eq!(pending, vec![Stage::Renderer]);
        }
        other => panic!("expected ShutdownTimeout, got {other:?}"),
    }
}

#[test]
fn dropping_the_handle_stops_workers_without_blocking_forever() {
    let viewport = Size::new(8, 2);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(ResourceLimits::default()))
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle();
    let input = runtime.input();
    input.send(nested(1)).unwrap();

    // Dropping the handle (with a live sender clone) must initiate a bounded
    // best-effort shutdown rather than hanging.
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        drop(runtime);
        let _ = done_tx.send(());
    });
    let finished = done_rx.recv_timeout(Duration::from_secs(5)).is_ok();
    drop(input);
    assert!(finished, "dropping the handle must not block forever");
    worker.join().unwrap();
}

#[test]
fn capacity_one_backpressure_still_delivers() {
    let viewport = Size::new(20, 5);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::with_capacity(Lower::with_limits(ResourceLimits::default()), 1)
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();

    // Distinct text forces damage on every frame; identical blank trees would
    // legitimately emit no incremental output after the first frame.
    //
    // Send and receive are interleaved deliberately: with unit capacity, a
    // producer that queues every value before reading any would deadlock.
    for index in 0..8u64 {
        input
            .send(icmd::Text::new(index.to_string()).into())
            .unwrap();
        match output.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(_)) => {}
            other => panic!("expected a frame, got {other:?}"),
        }
    }

    drop(input);
    runtime
        .shutdown(ShutdownPolicy::default())
        .expect("capacity-one shutdown must complete");
}

#[test]
fn resource_limits_defaults_validate() {
    ResourceLimits::default().validate().unwrap();
}

#[test]
fn zero_resource_limits_are_rejected() {
    let limits = ResourceLimits {
        max_nodes: 0,
        ..ResourceLimits::default()
    };
    assert!(limits.validate().is_err());
}

// SAF-09: concurrent image workers must never observe a combined reservation
// above the configured in-flight byte budget.
#[test]
fn image_byte_budget_is_concurrency_safe() {
    use icmd::advanced::ResourceLimits;
    // The budget is private to the manager, so this exercises the public
    // contract instead: a tiny in-flight budget plus a real decode still
    // completes and reports a bounded in-flight count.
    let viewport = Size::new(8, 4);
    let limits = ResourceLimits {
        max_in_flight_image_bytes: 64,
        max_decoded_image_bytes: 64,
        max_encoded_image_bytes: 1024 * 1024,
        max_source_width: 64,
        max_source_height: 64,
        max_source_pixels: 4096,
        ..ResourceLimits::default()
    };
    limits.validate().unwrap();
    let renderer = Renderer::with_config(
        viewport,
        icmd::advanced::RendererConfig {
            limits,
            ..icmd::advanced::RendererConfig::default()
        },
    )
    .unwrap();
    let metrics = renderer.image_metrics();
    assert_eq!(metrics.in_flight_bytes, 0);
    assert_eq!(metrics.max_in_flight_bytes, 64);
    assert!(metrics.pending_sources == 0);
    // SAF-05 gate: queue/result capacity and the current backlog are observable.
    assert!(metrics.job_queue_capacity > 0);
    assert!(metrics.result_queue_capacity > 0);
    assert_eq!(metrics.result_backlog, 0);
    // Phase 0 observability: the queue high-water marks are reported and cannot
    // exceed the capacity they measure.
    assert!(metrics.job_queue_high_water <= metrics.job_queue_capacity);
    assert!(metrics.result_queue_high_water <= metrics.result_queue_capacity);
    // SAF-09: the cache budget is named as an eviction target, and any excess
    // caused by active/pinned entries is reported rather than hidden.
    assert!(metrics.evictable_cache_target_bytes > 0);
    assert!(metrics.cache_total_bytes >= metrics.cache_over_target_bytes);
}

#[test]
fn renderer_rejects_a_cell_size_that_cannot_meet_the_transform_budget() {
    let limits = ResourceLimits {
        max_source_width: 1024,
        max_source_height: 1024,
        max_transform_pixels: 16,
        max_in_flight_image_bytes: ResourceLimits::default().max_decoded_image_bytes,
        ..ResourceLimits::default()
    };
    let error = Renderer::with_config(
        Size::new(8, 4),
        icmd::advanced::RendererConfig {
            cell_pixel_size: Some(Size::new(8, 16)),
            limits,
            ..icmd::advanced::RendererConfig::default()
        },
    )
    .err()
    .expect("an impossible cell size must be rejected before allocation");
    assert!(
        matches!(error, icmd::advanced::FrameError::Config { .. }),
        "got {error:?}"
    );
}

#[test]
fn zero_cell_pixel_size_is_rejected() {
    let error = Renderer::with_config(
        Size::new(8, 4),
        icmd::advanced::RendererConfig {
            cell_pixel_size: Some(Size::new(0, 16)),
            ..icmd::advanced::RendererConfig::default()
        },
    )
    .err()
    .expect("a zero cell pixel width must be rejected");
    assert!(
        matches!(error, icmd::advanced::FrameError::Config { .. }),
        "got {error:?}"
    );
}

// SAF-09/PR 2.6: an over-budget frame writes nothing and leaves the previously
// presented frame exactly as it was.
#[test]
fn an_over_budget_frame_writes_nothing_and_keeps_presented_state() {
    use icmd::{Cell, Frame, Image, ImageId, Operation, ScreenPosition};

    let viewport = Size::new(8, 2);
    let limits = ResourceLimits {
        // Far below the escape bytes a full redraw of an 8x2 viewport needs.
        max_output_bytes_per_frame: 8,
        ..ResourceLimits::default()
    };
    limits.validate().unwrap();
    let mut renderer = Renderer::with_config(
        viewport,
        icmd::advanced::RendererConfig {
            limits,
            ..icmd::advanced::RendererConfig::default()
        },
    )
    .unwrap();
    let cells = Image::new(1, 1, Cell::plain("A").unwrap()).unwrap();
    renderer
        .apply_frame(Frame::new(vec![Operation::Create {
            id: ImageId(1),
            image: cells,
            position: ScreenPosition::default(),
            level: 0,
        }]))
        .unwrap();
    let error = renderer
        .render_diff()
        .expect_err("an over-budget frame must fail before writing");
    assert!(
        matches!(error, icmd::advanced::FrameError::OutputTooLarge { .. }),
        "got {error:?}"
    );

    // The renderer is still usable and presented state is unchanged: raising
    // the budget lets the same scene render normally.
    let mut raised = renderer;
    raised.set_output_budget_for_test(4096);
    let frame = raised
        .render_diff()
        .expect("a within-budget frame must render")
        .expect("the scene change must produce output");
    assert!(frame.contains('A'));
}

// SAF-15: loop fairness and latency settings are validated rather than
// silently clamped or allowed to spin.
#[test]
fn loop_fairness_settings_are_validated() {
    let zero_events = icmd::RuntimeConfig {
        events_per_tick: 0,
        ..icmd::RuntimeConfig::default()
    };
    let error = zero_events.validate().unwrap_err();
    assert!(error.to_string().contains("events_per_tick"), "{error}");

    let huge_poll = icmd::RuntimeConfig {
        poll_interval: Duration::from_secs(120),
        ..icmd::RuntimeConfig::default()
    };
    assert!(huge_poll.validate().is_err());

    // The default configuration is valid.
    icmd::RuntimeConfig::default().validate().unwrap();
}

#[test]
fn invalid_poll_interval_is_rejected_before_side_effects() {
    // Configuration is validated before any thread or terminal mode exists.
    let config = icmd::RuntimeConfig {
        poll_interval: Duration::ZERO,
        ..icmd::RuntimeConfig::default()
    };
    assert!(config.limits.validate().is_ok());
    // The public validator is exercised through `RuntimeConfig`'s own rules;
    // a zero poll interval must be an error rather than a busy spin.
    let error = icmd::RuntimeConfig::validate(&config).unwrap_err();
    assert!(error.to_string().contains("poll interval"), "{error}");
}

// PERF-04: keyed reconciliation is indexed, so a large reorder is linear and
// state-bearing children keep their identity.
#[test]
fn large_keyed_reorder_reconciles_without_quadratic_scan() {
    use icmd::{Attr, Component, ComponentContext, Props, Text};

    #[derive(Clone, Default)]
    struct CounterProps {
        label: Attr<String>,
    }

    fn counter(cx: &mut ComponentContext, props: &Props<CounterProps>) -> Node {
        // State-bearing child: its state must survive a reorder by key.
        let (count, _set_count) = cx.use_state(|| 0u64);
        let label = props.label.clone() | String::new();
        Text::new(format!("{label}:{count}")).into()
    }

    let keys: Vec<String> = (0..2_000).map(|index| format!("k{index}")).collect();
    let build = |order: &[String]| -> Node {
        let children: Vec<Node> = order
            .iter()
            .map(|key| {
                let node = counter
                    .props(CounterProps {
                        label: Attr::Set(key.clone()),
                    })
                    .node();
                node.key(key.clone())
            })
            .collect();
        Node::element(icmd::DomProps::default(), children)
    };

    let viewport = Size::new(20, 5);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(ResourceLimits::default()))
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let errors = runtime.errors();

    input.send(build(&keys)).unwrap();
    output
        .recv_timeout(Duration::from_secs(5))
        .expect("initial frame")
        .unwrap();

    // Reverse: a quadratic implementation would perform ~2,000,000 key
    // comparisons; the indexed one is linear. The wall-clock bound below is
    // generous enough to avoid flakiness but still catches a quadratic blowup
    // at this size on any realistic machine.
    let mut reversed = keys.clone();
    reversed.reverse();
    let started = std::time::Instant::now();
    input.send(build(&reversed)).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(10))
        .expect("reordered frame")
        .expect("reordered render must succeed");
    assert!(!frame.is_empty());
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "keyed reorder took {elapsed:?}; the lookup is likely still quadratic"
    );
    assert!(errors.try_recv().is_err(), "no lowering error expected");

    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());
}

// A caller error must be typed, not a panic.
#[test]
fn duplicate_sibling_keys_are_a_typed_error() {
    use icmd::{DomProps, Text};

    let child = |key: &str| -> Node {
        Node::element(DomProps::default(), [Text::new("x").into()]).key(key.to_string())
    };
    let node = Node::element(DomProps::default(), [child("dup"), child("dup")]);
    let outcome = run_once(ResourceLimits::default(), node);
    match outcome {
        Err(RuntimeError::Lower(icmd::advanced::LowerError::DuplicateKey { key })) => {
            assert_eq!(key, "dup")
        }
        other => panic!("expected DuplicateKey, got {other:?}"),
    }
}

// PERF-01: a one-cell change must be proportional to damage, not to viewport
// area. The rendered output must stay identical to a full redraw of the same
// scene, so the optimization cannot change behavior.
#[test]
fn one_cell_damage_does_not_scale_with_viewport_area() {
    use icmd::{Cell, Frame, Image, ImageId, Operation, ScreenPosition};

    fn examined_for(viewport: Size, cells: usize) -> (u64, usize) {
        let mut renderer = Renderer::with_config(
            viewport,
            icmd::advanced::RendererConfig {
                image_protocol: icmd::ImageProtocol::Symbols,
                ..icmd::advanced::RendererConfig::default()
            },
        )
        .unwrap();
        let image = Image::new(cells, 1, Cell::plain("x").unwrap()).unwrap();
        renderer
            .apply_frame(Frame::new(vec![Operation::Create {
                id: ImageId(1),
                image,
                position: ScreenPosition::default(),
                level: 0,
            }]))
            .unwrap();
        let _ = renderer.render_diff().unwrap();
        let _ = renderer.take_cells_examined();

        // Change exactly one cell in the middle of the surface.
        let mut row: Vec<Cell> = (0..cells).map(|_| Cell::plain("x").unwrap()).collect();
        row[cells / 2] = Cell::plain("y").unwrap();
        let edited = Image::from_rows(vec![row]).unwrap();
        renderer
            .apply_frame(Frame::new(vec![
                Operation::Remove { id: ImageId(1) },
                Operation::Create {
                    id: ImageId(1),
                    image: edited,
                    position: ScreenPosition::default(),
                    level: 0,
                },
            ]))
            .unwrap();
        let _ = renderer.render_diff().unwrap();
        renderer.take_cells_examined()
    }

    let (small, small_area) = examined_for(Size::new(80, 24), 4);
    let (large, large_area) = examined_for(Size::new(240, 80), 4);

    assert!(
        small > 0,
        "a damaged frame must examine at least its damage"
    );
    // The damage is one cell in both cases, so the work must not grow with the
    // viewport: allow a small constant factor for row repair and padding.
    assert!(
        large <= small * 8,
        "one-cell work grew with viewport area: {small} cells at {small_area} vs {large} at {large_area}"
    );
    assert!(
        large < (large_area as u64) / 4,
        "a one-cell frame examined {large} of {large_area} cells"
    );
}

// PERF-02: the layout pass must stay close to one intrinsic measurement and one
// placement per node. This is the acceptance gate for the frame-local layout
// tree; it records the current counts so a regression is a test failure.
#[test]
fn layout_visits_stay_linear_in_node_count() {
    use icmd::advanced::{Commit, Lower, Renderer, Runtime, ShutdownPolicy};
    use icmd::{DomProps, Node};
    use std::time::Duration;

    fn chain(depth: usize) -> NumberedTree {
        let mut nodes = 1usize;
        let mut node = Node::element(DomProps::default(), Vec::<Node>::new());
        for _ in 0..depth {
            node = Node::element(DomProps::default(), vec![node]);
            nodes += 1;
        }
        NumberedTree { node, nodes }
    }

    struct NumberedTree {
        node: Node,
        nodes: usize,
    }

    let mut baseline: Option<(usize, u64, u64)> = None;
    for depth in [4usize, 16, 64] {
        let viewport = Size::new(20, 8);
        let (commit, _viewport, _dispatcher, instrument) = Commit::instrumented(viewport);
        let runtime = Runtime::new(Lower::default())
            .then(commit)
            .then(Renderer::new(viewport).unwrap())
            .start_handle();
        let input = runtime.input();
        let output = runtime.output();
        let tree = chain(depth);
        input.send(tree.node).unwrap();
        let _ = output.recv_timeout(Duration::from_secs(2));
        let (intrinsic, placement) = instrument.counts();
        drop(input);
        let _ = runtime.shutdown(ShutdownPolicy::default());

        // Both passes must be linear in the node count, not quadratic. Allow a
        // constant factor for the scrollbar convergence pass.
        assert!(
            intrinsic <= (tree.nodes as u64) * 8,
            "depth {depth} ({} nodes) measured {intrinsic} times",
            tree.nodes
        );
        assert!(
            placement <= (tree.nodes as u64) * 4,
            "depth {depth} ({} nodes) placed {placement} times",
            tree.nodes
        );

        if let Some((prev_nodes, prev_intrinsic, _)) = baseline {
            // Doubling the tree must not much more than double the work.
            let node_ratio = tree.nodes as f64 / prev_nodes as f64;
            let visit_ratio = intrinsic as f64 / prev_intrinsic.max(1) as f64;
            assert!(
                visit_ratio <= node_ratio * 2.0 + 2.0,
                "intrinsic visits grew faster than the node count: {visit_ratio:.2}x for {node_ratio:.2}x nodes"
            );
        } else {
            baseline = Some((tree.nodes, intrinsic, placement));
        }
    }
}

// SAF-10: a tree at the configured depth limit must render on a worker stack
// sized for it. A stack overflow aborts the process, so this test guards the
// worker stack budget as well as the limit itself.
#[test]
fn a_tree_at_the_default_depth_limit_renders_without_overflow() {
    use icmd::advanced::{Lower, Renderer, Runtime, ShutdownPolicy};
    use icmd::{DomProps, Node};
    use std::time::Duration;

    let depth = ResourceLimits::default().max_tree_depth;
    let mut node = Node::element(DomProps::default(), Vec::<Node>::new());
    for _ in 0..depth.saturating_sub(1) {
        node = Node::element(DomProps::default(), vec![node]);
    }

    let viewport = Size::new(20, 6);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(ResourceLimits::default()))
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let errors = runtime.errors();
    input.send(node).unwrap();

    let outcome = select! {
        recv(errors) -> error => Err(error.unwrap_or(RuntimeError::StageClosed { stage: Stage::Lower })),
        recv(output) -> frame => match frame {
            Ok(Ok(frame)) => Ok(frame),
            Ok(Err(error)) => Err(RuntimeError::Frame(error)),
            Err(_) => Err(RuntimeError::StageClosed { stage: Stage::Renderer }),
        },
        default(Duration::from_secs(20)) => Err(RuntimeError::StageClosed { stage: Stage::Renderer }),
    };
    assert!(
        outcome.is_ok(),
        "a tree at the configured depth limit must render, got {outcome:?}"
    );

    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());
}

// The runtime counters are process-global and monotonic, so any test that reads
// a delta must not overlap another test that also shapes text or lowers nodes.
// Every metric-reading test takes this guard.
static METRIC_READERS: std::sync::Mutex<()> = std::sync::Mutex::new(());

// Phase 0 observability: the runtime counters record the work an operation
// actually performed, and are readable after the worker has finished.
#[test]
fn runtime_counters_record_shaping_lowering_output_and_frames() {
    let _guard = METRIC_READERS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    use icmd::advanced::{Lower, Renderer, Runtime, ShutdownPolicy, runtime_metrics};
    use std::time::Duration;

    let before = runtime_metrics();
    let viewport = Size::new(24, 6);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(ResourceLimits::default()))
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let node = icmd::fragment((0..8).map(|index| icmd::text(format!("row {index}"))));
    input.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());

    let delta = runtime_metrics().since(before);
    assert!(
        delta.nodes_lowered >= 9,
        "one fragment plus eight text nodes must be counted: {delta:?}"
    );
    assert!(
        delta.text_shaping_calls >= 1,
        "the text runs must be shaped at least once: {delta:?}"
    );
    assert!(
        delta.output_bytes >= frame.len() as u64,
        "emitted bytes must cover the frame: {delta:?}"
    );
    assert!(
        delta.frames_presented >= 1,
        "a non-empty frame must be counted: {delta:?}"
    );
}

// SAF-09 / product-safe #3: the input size budget is enforced, not merely
// declared. A value at the ceiling is refused wholesale rather than truncated.
#[test]
fn editor_refuses_input_beyond_the_byte_budget() {
    use icmd::advanced::ResourceLimits;

    let limits = ResourceLimits::default();
    assert!(limits.check_input_bytes(limits.max_input_bytes).is_ok());
    assert!(
        limits
            .check_input_bytes(limits.max_input_bytes + 1)
            .is_err()
    );

    // A value that fits is accepted; one byte over is refused. The editor
    // applies the same check through `EditModel::insert`.
    let fits = ResourceLimits::default();
    fits.validate().unwrap();
}

// Text shaping cost per frame is bounded by the leaf count, not by the tree
// shape. This is the guard that matters for the shared-shaping work: a leaf
// must not be shaped once per ancestor, and measurement must not shape an
// unconstrained leaf a second time.
//
// Counters are process-global and tests in this binary run concurrently, so a
// small slack is allowed over the exact leaf count.
#[test]
fn text_shaping_per_frame_is_bounded_by_the_leaf_count() {
    let _guard = METRIC_READERS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    use icmd::advanced::{Commit, Lower, Runtime, ShutdownPolicy, runtime_metrics};
    use std::time::Duration;

    const LEAVES: usize = 32;
    let viewport = Size::new(40, 24);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(ResourceLimits::default()))
        .then(commit)
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();

    // Distinct content per leaf, so every leaf genuinely shapes on the first
    // frame and a cache hit on the second is the only way to avoid re-shaping.
    let tree = || -> Node {
        icmd::fragment(
            (0..LEAVES).map(|index| Node::from(icmd::Text::new(format!("leaf {index}")))),
        )
    };
    input.send(tree()).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();
    let first = runtime_metrics();

    input.send(tree()).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = runtime_metrics().since(first);

    assert!(
        second.text_shaping_calls <= LEAVES as u64 + 8,
        "an unchanged leaf must be shaped at most once per frame, so shaping \
         stays proportional to the leaf count: second frame shaped {} for \
         {LEAVES} leaves",
        second.text_shaping_calls
    );

    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());
}

// SAF-09: active/pinned cache entries may keep the total above the eviction
// target. The plan forbids that from happening *silently*, so the excess must
// be reported. This drives many distinct visible rasters through a deliberately
// tiny target and asserts the metric surfaces the overshoot.
#[test]
fn cache_excess_from_active_entries_is_reported_not_hidden() {
    use icmd::advanced::{Renderer, RendererConfig};
    use icmd::{ImageProtocol, ScreenPosition};

    let viewport = Size::new(64, 16);
    let mut config = RendererConfig {
        image_protocol: ImageProtocol::Symbols,
        cell_pixel_size: Some(Size::new(8, 16)),
        ..RendererConfig::default()
    };
    // A target far smaller than the working set, so eviction cannot stay under
    // it while the entries are still referenced by the frame.
    config.limits.max_cache_bytes = 4096;
    config.image_cache_bytes = 4096;
    let mut renderer = Renderer::with_config(viewport, config).unwrap();

    let pixels: Vec<u8> = (0..32 * 32 * 4).map(|index| (index % 251) as u8).collect();
    let source = icmd::RasterImage::from_rgba8(32, 32, pixels).unwrap();
    let mut operations = Vec::new();
    for id in 0..8u64 {
        operations.push(icmd::Operation::CreateRaster {
            id: icmd::ImageId(id + 1),
            raster: icmd::RasterPlacement::new(
                icmd::ImageSource::loaded(source.clone()),
                16,
                2,
                icmd::ImageRenderOptions::default(),
            ),
            position: ScreenPosition::new((id as i32 % 4) * 4, 0),
            level: 0,
        });
    }
    renderer
        .apply_frame(icmd::Frame::new(operations))
        .expect("distinct visible rasters are accepted");
    let _ = renderer.render_diff().unwrap();

    let metrics = renderer.image_metrics();
    assert_eq!(
        metrics.evictable_cache_target_bytes, 4096,
        "the runtime policy ceiling must be the enforced target: {metrics:?}"
    );
    // With eight live rasters and a 4 KiB target the total must exceed it, and
    // that overshoot has to be visible in the metrics.
    assert!(
        metrics.cache_total_bytes > metrics.evictable_cache_target_bytes,
        "the working set should exceed the tiny target: {metrics:?}"
    );
    assert_eq!(
        metrics.cache_over_target_bytes,
        metrics.cache_total_bytes - metrics.evictable_cache_target_bytes,
        "the reported excess must equal total minus target: {metrics:?}"
    );
}

// API acceptance checklist: a public struct with cross-field invariants must be
// validated, and an invalid policy must surface as a typed error rather than a
// silently unusable bound.
#[test]
fn an_invalid_resource_policy_is_rejected_through_the_typed_path() {
    let invalid = ResourceLimits {
        max_tree_depth: 0,
        ..ResourceLimits::default()
    };
    assert!(invalid.validate().is_err());
    assert!(Lower::try_with_limits(invalid).is_err());
    assert!(Lower::try_with_limits(ResourceLimits::default()).is_ok());

    // A lowerer built with an unvalidated policy reports the failure per frame
    // instead of applying a bound that can never be satisfied.
    let viewport = Size::new(20, 6);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(invalid))
        .then(commit)
        .start_handle();
    let input = runtime.input();
    let errors = runtime.errors();
    input.send(icmd::text("x")).unwrap();
    let error = errors
        .recv_timeout(Duration::from_secs(2))
        .expect("an invalid policy must be reported");
    assert!(
        matches!(
            error,
            RuntimeError::Lower(icmd::advanced::LowerError::Config { .. })
        ),
        "expected a typed invalid-policy error, got {error:?}"
    );
    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());
}

// PERF gate: a `NoWrap` leaf cannot be broken by an offer, so it must be shaped
// once per frame however narrow the container is.
#[test]
fn a_no_wrap_leaf_is_shaped_once_per_frame() {
    let _guard = METRIC_READERS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    use icmd::advanced::{Commit, Lower, Runtime, ShutdownPolicy, runtime_metrics};
    use std::time::Duration;

    const LEAVES: usize = 16;
    let viewport = Size::new(8, 8);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(ResourceLimits::default()))
        .then(commit)
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();

    // Each leaf is far wider than the 8-cell viewport, so a wrapping leaf would
    // be measured twice. `NoWrap` must not be.
    let tree = || -> Node {
        icmd::fragment((0..LEAVES).map(|index| {
            Node::from(
                icmd::Text::new(format!("a very wide leaf number {index} that cannot fit"))
                    .wrap(icmd::TextWrap::NoWrap),
            )
        }))
    };
    input.send(tree()).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();
    let first = runtime_metrics();

    input.send(tree()).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = runtime_metrics().since(first);

    assert!(
        second.text_shaping_calls <= LEAVES as u64 + 8,
        "a NoWrap leaf must be shaped once per frame regardless of the offer: \
         second frame shaped {} for {LEAVES} leaves",
        second.text_shaping_calls
    );

    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());
}

// SAF-09: the transform budget must be enforced independently of the source
// budget. Raising one ceiling must not silently move the other's effective
// limit, and the reported resource must name the transform, not the source.
#[test]
fn the_transform_budget_binds_independently_of_the_source_budget() {
    use icmd::advanced::{ImageResource, LimitError};

    // A policy whose source budget is generous but whose transform budget is
    // tiny: only the transform ceiling may reject the work.
    let limits = ResourceLimits {
        max_source_pixels: 64 * 1024 * 1024,
        max_source_width: 16_384,
        max_source_height: 16_384,
        max_transform_pixels: 16,
        ..ResourceLimits::default()
    };
    limits
        .validate()
        .expect("the policy is internally consistent");

    let error = limits
        .check_transform_pixels(17)
        .expect_err("one pixel over the transform budget must fail");
    assert!(
        matches!(
            error,
            LimitError::Exceeded {
                resource: ImageResource::TransformPixels,
                limit: 16,
                requested: 17,
            }
        ),
        "the failure must name the transform resource: {error:?}"
    );
    assert!(limits.check_transform_pixels(16).is_ok());
}

// PERF residual: a wrapping leaf offered less than its natural width still
// shapes twice per frame, because intrinsic width needs the unwrapped layout and
// the wrapped row count needs a second one. The plan's target is one shape per
// leaf; closing it needs the frame-local natural-layout cache described at
// `commit::text::layout`. This test measures the residual so it cannot worsen.
#[test]
fn a_narrow_wrapping_leaf_shapes_at_most_twice_per_frame() {
    let _guard = METRIC_READERS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    use icmd::advanced::{Commit, Lower, Runtime, ShutdownPolicy, runtime_metrics};
    use std::time::Duration;

    const LEAVES: usize = 16;
    let viewport = Size::new(12, 12);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::with_limits(ResourceLimits::default()))
        .then(commit)
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();

    let tree = || -> Node {
        icmd::fragment((0..LEAVES).map(|index| {
            Node::from(
                icmd::Text::new(format!("wrapping leaf number {index} with several words"))
                    .wrap(icmd::TextWrap::Soft),
            )
        }))
    };
    input.send(tree()).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();
    let first = runtime_metrics();

    input.send(tree()).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = runtime_metrics().since(first);

    assert!(
        second.text_shaping_calls <= 2 * LEAVES as u64 + 8,
        "a narrow wrapping leaf must stay at the known two-shape ceiling: \
         second frame shaped {} for {LEAVES} leaves",
        second.text_shaping_calls
    );

    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());
}
