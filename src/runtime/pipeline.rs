use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicUsize, Ordering},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, bounded};

use super::limits::RendererConfigError;
use super::lower::LowerError;
use super::renderer::FrameError;

// Must cover `ResourceLimits::max_tree_depth` recursive frames with margin.
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Lower,
    Commit,
    Renderer,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lower => "lower",
            Self::Commit => "commit",
            Self::Renderer => "renderer",
        }
    }
}

// Every stage failure is a typed, named condition. A closed channel is never
// collapsed into a generic error: shutdown paths distinguish an expected close
// from a stage that panicked or timed out.
#[derive(Debug)]
pub enum RuntimeError {
    Lower(LowerError),
    Frame(FrameError),
    StagePanicked { stage: Stage },
    StageClosed { stage: Stage },
    ShutdownTimeout { pending: Vec<Stage> },
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lower(error) => write!(f, "lowering failed: {error}"),
            Self::Frame(error) => write!(f, "rendering frame failed: {error}"),
            Self::StagePanicked { stage } => write!(f, "{} stage panicked", stage.as_str()),
            Self::StageClosed { stage } => {
                write!(f, "{} stage stopped unexpectedly", stage.as_str())
            }
            Self::ShutdownTimeout { pending } => {
                let names: Vec<_> = pending.iter().map(|stage| stage.as_str()).collect();
                write!(f, "shutdown timed out waiting for: {}", names.join(", "))
            }
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lower(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::StagePanicked { .. }
            | Self::StageClosed { .. }
            | Self::ShutdownTimeout { .. } => None,
        }
    }
}

impl From<FrameError> for RuntimeError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

impl From<LowerError> for RuntimeError {
    fn from(error: LowerError) -> Self {
        Self::Lower(error)
    }
}

impl From<RendererConfigError> for RuntimeError {
    fn from(_error: RendererConfigError) -> Self {
        // A configuration rejection can only be produced before the runtime
        // exists; reaching a worker with one is an internal invariant break.
        Self::StageClosed {
            stage: Stage::Renderer,
        }
    }
}

// A stage consumes its upstream channel and produces its downstream channel.
// The upstream receiver is handed in by the runtime so adjacent stages are
// wired point-to-point, with no forwarding (bridge) thread between them.
pub trait PipelineComponent: Send + 'static {
    type Input: Send + 'static;
    type Output: Send + 'static;

    const STAGE: Stage;

    // `errors` lets a stage report a recoverable rejection and keep serving.
    // Returning `Err` is terminal: the worker stops and the error is reported.
    fn run(
        self,
        input: Receiver<Self::Input>,
        output: Sender<Self::Output>,
        errors: Sender<RuntimeError>,
    ) -> Result<(), RuntimeError>;
}

pub struct End;

pub struct Chain<Head, Tail>(Head, Tail);

pub trait Append<Next> {
    type Output;
    fn append(self, next: Next) -> Self::Output;
}

impl<Next> Append<Next> for End {
    type Output = Chain<Next, End>;

    fn append(self, next: Next) -> Self::Output {
        Chain(next, End)
    }
}

impl<Head, Tail, Next> Append<Next> for Chain<Head, Tail>
where
    Tail: Append<Next>,
{
    type Output = Chain<Head, <Tail as Append<Next>>::Output>;

    fn append(self, next: Next) -> Self::Output {
        let Chain(head, tail) = self;
        Chain(head, tail.append(next))
    }
}

type WorkerJoin = (Stage, JoinHandle<()>);

// Builds the chain by allocating each intermediate channel once and handing the
// receiver directly to the next stage. There are exactly as many workers as
// stages, each named, and every join handle is retained by the caller.
// Test/diagnostic counter so a leaked worker is a direct test failure rather
// than an invisible thread.
static LIVE_WORKERS: AtomicUsize = AtomicUsize::new(0);

#[doc(hidden)]
pub fn live_worker_count() -> usize {
    LIVE_WORKERS.load(Ordering::SeqCst)
}

struct WorkerGuard;

impl WorkerGuard {
    fn enter() -> Self {
        LIVE_WORKERS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        LIVE_WORKERS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[doc(hidden)]
pub trait Driver {
    type Input: Send + 'static;
    type Output: Send + 'static;

    fn drive(
        self,
        capacity: usize,
        input: Receiver<Self::Input>,
        output: Sender<Self::Output>,
        errors: &Sender<RuntimeError>,
        joins: &mut Vec<WorkerJoin>,
    );
}

fn spawn_worker<C>(
    component: C,
    input: Receiver<C::Input>,
    output: Sender<C::Output>,
    errors: &Sender<RuntimeError>,
    joins: &mut Vec<WorkerJoin>,
) where
    C: PipelineComponent,
{
    let stage = C::STAGE;
    let error_tx = errors.clone();
    let stage_error_tx = errors.clone();
    let name = format!("icmd-{}", stage.as_str());
    // Lowering, layout, and paint are recursive over the logical tree, so the
    // worker stack must comfortably cover the configured depth limit. The
    // default thread stack is not enough for a tree at the default depth, whose
    // overflow would abort the process rather than return a typed error.
    let handle = thread::Builder::new()
        .name(name)
        .stack_size(WORKER_STACK_BYTES)
        .spawn(move || {
            let _live = WorkerGuard::enter();
            // A panicking stage is reported by name instead of looking like an
            // unexplained closed channel. The output sender is dropped on every
            // path, so downstream stages still terminate.
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                component.run(input, output, stage_error_tx)
            }));
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    let _ = error_tx.send(error);
                }
                Err(_) => {
                    let _ = error_tx.send(RuntimeError::StagePanicked { stage });
                }
            }
        })
        .expect("failed to spawn pipeline worker");
    joins.push((stage, handle));
}

impl<C> Driver for Chain<C, End>
where
    C: PipelineComponent,
{
    type Input = C::Input;
    type Output = C::Output;

    fn drive(
        self,
        _capacity: usize,
        input: Receiver<Self::Input>,
        output: Sender<Self::Output>,
        errors: &Sender<RuntimeError>,
        joins: &mut Vec<WorkerJoin>,
    ) {
        let Chain(component, End) = self;
        spawn_worker(component, input, output, errors, joins);
    }
}

impl<Head, Tail> Driver for Chain<Head, Tail>
where
    Head: PipelineComponent,
    Tail: Driver<Input = Head::Output>,
{
    type Input = Head::Input;
    type Output = Tail::Output;

    fn drive(
        self,
        capacity: usize,
        input: Receiver<Self::Input>,
        output: Sender<Self::Output>,
        errors: &Sender<RuntimeError>,
        joins: &mut Vec<WorkerJoin>,
    ) {
        let (middle_tx, middle_rx) = bounded(capacity.max(1));
        let Chain(head, tail) = self;
        spawn_worker(head, input, middle_tx, errors, joins);
        tail.drive(capacity, middle_rx, output, errors, joins);
    }
}

pub struct ShutdownPolicy {
    pub timeout: Duration,
}

impl Default for ShutdownPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
        }
    }
}

impl ShutdownPolicy {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

pub struct Runtime<C> {
    chain: C,
    capacity: usize,
}

impl<C> Runtime<Chain<C, End>>
where
    C: PipelineComponent,
{
    pub fn new(component: C) -> Self {
        Self {
            chain: Chain(component, End),
            capacity: 64,
        }
    }

    pub fn with_capacity(component: C, capacity: usize) -> Self {
        Self {
            chain: Chain(component, End),
            capacity: capacity.max(1),
        }
    }
}

impl<C> Runtime<C> {
    pub fn then<Next>(self, next: Next) -> Runtime<<C as Append<Next>>::Output>
    where
        C: Append<Next>,
        Next: PipelineComponent,
    {
        Runtime {
            chain: self.chain.append(next),
            capacity: self.capacity,
        }
    }

    // Owns the workers and their join handles so shutdown can be acknowledged.
    pub fn start_handle(self) -> RuntimeHandle<C::Input, C::Output>
    where
        C: Driver,
    {
        self.chain.start_handle(self.capacity)
    }

    // Transitional tuple constructor. It hands back the same channels but keeps
    // worker ownership on a supervisor thread that joins the stages once the
    // caller drops its senders. New code should use `start_handle` so it can
    // observe typed stage errors and acknowledge shutdown itself.
    pub fn start(self) -> (Sender<C::Input>, Receiver<C::Output>)
    where
        C: Driver,
    {
        let handle = self.start_handle();
        let (input, output) = handle.into_parts();
        (input, output)
    }
}

impl<I, O> RuntimeHandle<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    pub fn input(&self) -> Sender<I> {
        self.input
            .clone()
            .expect("runtime handle already shut down")
    }

    pub fn output(&self) -> Receiver<O> {
        self.output.clone()
    }

    pub fn errors(&self) -> Receiver<RuntimeError> {
        self.errors.clone()
    }

    pub fn try_error(&self) -> Option<RuntimeError> {
        self.errors.try_recv().ok()
    }

    // Split the handle into its channels and hand worker ownership to a
    // supervisor thread that joins the stages when they finish.
    pub fn into_parts(mut self) -> (Sender<I>, Receiver<O>) {
        let input = self.input.take().expect("runtime handle already shut down");
        let output = self.output.clone();
        let errors = self.errors.clone();
        let joins = std::mem::take(&mut self.joins);
        thread::spawn(move || {
            for (_, handle) in joins {
                let _ = handle.join();
            }
            drop(errors);
        });
        (input, output)
    }

    // Close the input side, then join every worker within the policy timeout.
    // A timeout is reported with the stages still pending; it is never mistaken
    // for a successful shutdown.
    pub fn shutdown(mut self, policy: ShutdownPolicy) -> Result<(), RuntimeError> {
        self.input.take();
        let deadline = Instant::now() + policy.timeout;
        let joins = std::mem::take(&mut self.joins);
        while joins.iter().any(|(_, handle)| !handle.is_finished()) {
            if Instant::now() >= deadline {
                let pending = joins
                    .iter()
                    .filter(|(_, handle)| !handle.is_finished())
                    .map(|(stage, _)| *stage)
                    .collect();
                return Err(RuntimeError::ShutdownTimeout { pending });
            }
            thread::sleep(Duration::from_millis(1));
        }
        for (_, handle) in joins {
            let _ = handle.join();
        }
        Ok(())
    }
}

pub struct RuntimeHandle<I: Send + 'static, O: Send + 'static> {
    input: Option<Sender<I>>,
    output: Receiver<O>,
    errors: Receiver<RuntimeError>,
    joins: Vec<WorkerJoin>,
}

impl<I, O> Drop for RuntimeHandle<I, O>
where
    I: Send + 'static,
    O: Send + 'static,
{
    fn drop(&mut self) {
        // Dropping without an explicit shutdown still stops intake and gives
        // workers a bounded chance to finish; it never blocks forever.
        self.input.take();
        let deadline = Instant::now() + Duration::from_millis(250);
        while self.joins.iter().any(|(_, handle)| !handle.is_finished())
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(1));
        }
        for (_, handle) in std::mem::take(&mut self.joins) {
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

trait StartChain: Driver {
    fn start_handle(self, capacity: usize) -> RuntimeHandle<Self::Input, Self::Output>;
}

impl<C> StartChain for C
where
    C: Driver,
{
    fn start_handle(self, capacity: usize) -> RuntimeHandle<Self::Input, Self::Output> {
        let (input, input_rx) = bounded(capacity.max(1));
        let (output_tx, output) = bounded(capacity.max(1));
        let (error_tx, errors) = bounded(capacity.max(1));
        let mut joins = Vec::new();
        self.drive(capacity, input_rx, output_tx, &error_tx, &mut joins);
        RuntimeHandle {
            input: Some(input),
            output,
            errors,
            joins,
        }
    }
}
