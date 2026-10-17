//! Frame-clock scheduling: callbacks run after presented frames, force the
//! owning component to render, pause/resume, and report panics.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use icmd::advanced::{Commit, FrameError, Lower, Renderer, Runtime, RuntimeHandle};
use icmd::{AppHandle, Component, ComponentContext, FrameClock, Node, Props, Size, text};

type StageOutput = Result<String, FrameError>;

struct Pipeline {
    runtime: RuntimeHandle<Node, StageOutput>,
    output: crossbeam_channel::Receiver<StageOutput>,
    handle: AppHandle,
}

impl Pipeline {
    fn new(viewport: Size) -> Self {
        let handle = AppHandle::default();
        let (commit, _) = Commit::new(viewport);
        let runtime = Runtime::new(Lower::default().with_handle(handle.clone()))
            .then(commit)
            .then(Renderer::new(viewport).expect("renderer"))
            .start_handle();
        Self {
            output: runtime.output(),
            runtime,
            handle,
        }
    }

    fn send(&self, node: Node) {
        self.runtime.input().send(node).expect("root accepted");
    }

    fn frame(&self) {
        self.output
            .recv_timeout(Duration::from_secs(2))
            .expect("a frame arrives")
            .expect("renderer succeeds");
    }

    fn pump(&self) {
        while self.output.recv_timeout(Duration::from_millis(200)).is_ok() {}
    }
}

#[derive(Clone, Default)]
struct Ctx {
    renders: Arc<AtomicU64>,
    ticks: Arc<AtomicU64>,
    clock: Arc<Mutex<Option<FrameClock>>>,
}

fn clocked(cx: &mut ComponentContext, props: &Props<Ctx>) -> Node {
    props.renders.fetch_add(1, Ordering::SeqCst);
    let ticks = props.ticks.clone();
    let clock = cx.use_frame_clock(move |_dt| {
        ticks.fetch_add(1, Ordering::SeqCst);
    });
    *props.data().clock.lock().unwrap() = Some(clock);
    text("clocked")
}

#[test]
fn a_due_clock_runs_and_rerenders_its_component() {
    let ctx = Ctx::default();
    let node = clocked.apply(ctx.clone());
    let pipeline = Pipeline::new(Size::new(20, 4));
    pipeline.send(node);
    pipeline.frame();
    let renders = ctx.renders.load(Ordering::SeqCst);
    assert_eq!(renders, 1);

    let future = Instant::now() + Duration::from_millis(80);
    assert!(pipeline.handle.tick_frame_clocks(future));
    pipeline.pump();
    assert_eq!(ctx.ticks.load(Ordering::SeqCst), 1);
    assert!(
        ctx.renders.load(Ordering::SeqCst) > renders,
        "a clock forces its owning component to render"
    );

    // Not due yet: the next deadline is one interval after the tick.
    assert!(
        !pipeline
            .handle
            .tick_frame_clocks(future + Duration::from_millis(1))
    );
    assert_eq!(ctx.ticks.load(Ordering::SeqCst), 1);
    drop(pipeline.runtime);
}

#[test]
fn pause_and_resume_control_ticks() {
    let ctx = Ctx::default();
    let node = clocked.apply(ctx.clone());
    let pipeline = Pipeline::new(Size::new(20, 4));
    pipeline.send(node);
    pipeline.frame();
    let clock = ctx.clock.lock().unwrap().clone().expect("clock");
    clock.pause();
    assert!(!clock.is_active());
    let future = Instant::now() + Duration::from_secs(1);
    assert!(!pipeline.handle.tick_frame_clocks(future));
    assert_eq!(ctx.ticks.load(Ordering::SeqCst), 0);

    clock.resume();
    assert!(clock.is_active());
    assert!(
        pipeline
            .handle
            .tick_frame_clocks(Instant::now() + Duration::from_secs(2))
    );
    assert_eq!(ctx.ticks.load(Ordering::SeqCst), 1);
    drop(pipeline.runtime);
}

#[test]
fn a_panicking_clock_is_reported_and_deactivated() {
    let clock_slot = Arc::new(Mutex::new(None));
    let clock_for_component = clock_slot.clone();
    fn panicking(cx: &mut ComponentContext, props: &Props<Arc<Mutex<Option<FrameClock>>>>) -> Node {
        let slot = props.data().clone();
        let clock = cx.use_frame_clock(move |_dt| panic!("clock exploded"));
        *slot.lock().unwrap() = Some(clock);
        text("clocked")
    }
    let node = panicking.apply(clock_for_component);
    let pipeline = Pipeline::new(Size::new(20, 4));
    pipeline.send(node);
    pipeline.frame();
    assert!(
        pipeline
            .handle
            .tick_frame_clocks(Instant::now() + Duration::from_millis(80))
    );
    assert!(pipeline.handle.take_frame_clock_fault());
    let clock = clock_slot.lock().unwrap().clone().expect("clock");
    assert!(!clock.is_active(), "a panicking clock stops itself");
    drop(pipeline.runtime);
}

#[test]
fn interval_is_clamped_and_reported() {
    let ctx = Ctx::default();
    let node = clocked.apply(ctx.clone());
    let pipeline = Pipeline::new(Size::new(20, 4));
    pipeline.send(node);
    pipeline.frame();
    let clock = ctx.clock.lock().unwrap().clone().expect("clock");
    clock.interval(Duration::from_nanos(1));
    assert_eq!(clock.current_interval(), Duration::from_millis(1));
    clock.interval(Duration::from_secs(10));
    assert_eq!(clock.current_interval(), Duration::from_secs(1));
    drop(pipeline.runtime);
}
