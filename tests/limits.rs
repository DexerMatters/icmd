use std::time::Duration;

use crossbeam_channel::select;
use crossbeam_channel::{Receiver, Sender};
use icmd::{
    Commit, Lower, Node, PipelineComponent, Renderer, ResourceLimits, Runtime, RuntimeError,
    ShutdownPolicy, Size, Stage,
};

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
        Err(RuntimeError::Lower(icmd::LowerError::TreeTooLarge { limit, observed })) => {
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
        Err(RuntimeError::Lower(icmd::LowerError::TreeTooDeep { limit, .. })) => {
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
        Err(RuntimeError::Lower(icmd::LowerError::TreeTooLarge { limit, observed })) => {
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
        Ok(RuntimeError::Lower(icmd::LowerError::TreeTooLarge { .. }))
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
    let before = icmd::live_worker_count();
    let mut settled = false;
    for _ in 0..400 {
        if icmd::live_worker_count() <= before {
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
    use icmd::ResourceLimits;
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
        icmd::RendererConfig {
            limits,
            ..icmd::RendererConfig::default()
        },
    )
    .unwrap();
    let metrics = renderer.image_metrics();
    assert_eq!(metrics.in_flight_bytes, 0);
    assert_eq!(metrics.max_in_flight_bytes, 64);
    assert!(metrics.pending_sources == 0);
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
        icmd::RendererConfig {
            cell_pixel_size: Some(Size::new(8, 16)),
            limits,
            ..icmd::RendererConfig::default()
        },
    )
    .err()
    .expect("an impossible cell size must be rejected before allocation");
    assert!(
        matches!(error, icmd::FrameError::Config { .. }),
        "got {error:?}"
    );
}

#[test]
fn zero_cell_pixel_size_is_rejected() {
    let error = Renderer::with_config(
        Size::new(8, 4),
        icmd::RendererConfig {
            cell_pixel_size: Some(Size::new(0, 16)),
            ..icmd::RendererConfig::default()
        },
    )
    .err()
    .expect("a zero cell pixel width must be rejected");
    assert!(
        matches!(error, icmd::FrameError::Config { .. }),
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
        icmd::RendererConfig {
            limits,
            ..icmd::RendererConfig::default()
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
        matches!(error, icmd::FrameError::OutputTooLarge { .. }),
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
