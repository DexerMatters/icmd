//! Pipeline runtime: stage identity, typed stage errors, the component and
//! driver traits, chain construction, and the handle that owns the worker
//! threads and performs bounded shutdown.

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

/// Worker thread stack size in bytes, sized to cover
/// `ResourceLimits::max_tree_depth` recursive frames with margin.
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

/// Identifies one pipeline stage for naming, diagnostics, and shutdown reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Lowers logical nodes into DOM nodes.
    Lower,
    /// Commits DOM nodes into a rendered frame through layout and paint.
    Commit,
    /// Encodes rendered frames into terminal output strings.
    Renderer,
}

impl Stage {
    /// Returns the lowercase stage name used in thread names and error text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lower => "lower",
            Self::Commit => "commit",
            Self::Renderer => "renderer",
        }
    }
}

/// One typed, named pipeline failure; a closed channel is never collapsed into a
/// generic error, so shutdown distinguishes an expected close from a stage that
/// panicked or timed out.
#[derive(Debug)]
pub enum RuntimeError {
    /// Logical-tree lowering failed.
    Lower(LowerError),
    /// Frame rendering failed.
    Frame(FrameError),
    /// A stage panicked; the panic is reported by stage name and the stage's
    /// output sender is dropped on every path so downstream stages terminate.
    StagePanicked {
        /// Stage whose worker panicked.
        stage: Stage,
    },
    /// A stage's channel closed unexpectedly.
    StageClosed {
        /// Stage that stopped before the runtime closed its input.
        stage: Stage,
    },
    /// An application callback failed while the commit stage published an
    /// element snapshot.
    ApplicationCallback(&'static str),
    /// Shutdown did not finish within the policy timeout.
    ShutdownTimeout {
        /// Stages still running when the timeout expired.
        pending: Vec<Stage>,
    },
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
            Self::ApplicationCallback(detail) => {
                write!(f, "application callback failed: {detail}")
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
            | Self::ApplicationCallback(_)
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
        Self::StageClosed {
            stage: Stage::Renderer,
        }
    }
}

/// A stage that consumes its upstream channel and produces its downstream
/// channel; adjacent stages are wired point-to-point with no forwarding thread.
pub trait PipelineComponent: Send + 'static {
    /// Item type received from the upstream stage.
    type Input: Send + 'static;
    /// Item type sent to the downstream stage.
    type Output: Send + 'static;

    /// Stage identity this component reports in errors and thread names.
    const STAGE: Stage;

    /// Runs until the input channel closes, forwarding recoverable rejections
    /// through `errors` and staying alive; returning `Err` is terminal and stops
    /// the worker.
    fn run(
        self,
        input: Receiver<Self::Input>,
        output: Sender<Self::Output>,
        errors: Sender<RuntimeError>,
    ) -> Result<(), RuntimeError>;
}

/// Terminates a [`Chain`] type list.
pub struct End;

/// Type-level list of pipeline stages; starting it allocates each intermediate
/// channel once and hands the receiver directly to the next stage, so there are
/// exactly as many named workers as stages and every join handle is retained.
pub struct Chain<Head, Tail>(Head, Tail);

/// Appends one stage type to a [`Chain`] type list.
pub trait Append<Next> {
    /// Chain type with `Next` added at the tail.
    type Output;
    /// Appends `next` at the tail of the chain.
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

/// Test/diagnostic counter of live workers, so a leaked worker is a direct test
/// failure rather than an invisible thread.
static LIVE_WORKERS: AtomicUsize = AtomicUsize::new(0);

/// Number of pipeline worker threads currently alive, for tests and diagnostics.
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

/// Spawns one worker per stage and wires their channels, recording each join
/// handle in `joins`; internal plumbing exposed for the chain types.
#[doc(hidden)]
pub trait Driver {
    /// Item type received by the first stage of the chain.
    type Input: Send + 'static;
    /// Item type produced by the last stage of the chain.
    type Output: Send + 'static;

    /// Spawns every chain worker over channels bounded by `capacity` items.
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
    let handle = thread::Builder::new()
        .name(name)
        .stack_size(WORKER_STACK_BYTES)
        .spawn(move || {
            let _live = WorkerGuard::enter();
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

/// Bounds how long shutdown waits for workers to join.
pub struct ShutdownPolicy {
    /// Maximum time to wait for workers after the input side closes.
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
    /// Builds a policy that waits up to `timeout` for workers to join.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

/// Builder that owns a pipeline chain and its per-channel item capacity.
pub struct Runtime<C> {
    chain: C,
    capacity: usize,
}

impl<C> Runtime<Chain<C, End>>
where
    C: PipelineComponent,
{
    /// Starts a runtime around a single component with a default capacity of 64
    /// items per channel.
    pub fn new(component: C) -> Self {
        Self {
            chain: Chain(component, End),
            capacity: 64,
        }
    }

    /// Starts a runtime around a single component with `capacity` items per
    /// channel, clamped to at least 1.
    pub fn with_capacity(component: C, capacity: usize) -> Self {
        Self {
            chain: Chain(component, End),
            capacity: capacity.max(1),
        }
    }
}

impl<C> Runtime<C> {
    /// Appends `next` as the final stage, preserving the configured capacity.
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

    /// Spawns the workers and returns a handle that owns their join handles, so
    /// shutdown can be observed and acknowledged.
    pub fn start_handle(self) -> RuntimeHandle<C::Input, C::Output>
    where
        C: Driver,
    {
        self.chain.start_handle(self.capacity)
    }

    /// Transitional tuple constructor: returns the same channels but keeps worker
    /// ownership on a supervisor thread that joins the stages once the caller
    /// drops its senders. New code should use [`Runtime::start_handle`] so it can
    /// observe typed stage errors and acknowledge shutdown itself.
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
    /// Returns a clone of the input sender for submitting logical roots.
    pub fn input(&self) -> Sender<I> {
        self.input
            .clone()
            .expect("runtime handle already shut down")
    }

    /// Stops accepting roots without joining the workers; dropping the handle's
    /// own input sender is what lets the stages observe the channel as closed.
    /// Frames emitted while the pipeline drains must still be consumed by the
    /// caller before [`RuntimeHandle::shutdown`] joins the workers.
    pub fn close_input(&mut self) {
        self.input.take();
    }

    /// Returns a clone of the frame receiver for downstream consumers.
    pub fn output(&self) -> Receiver<O> {
        self.output.clone()
    }

    /// Returns a clone of the typed stage-error receiver.
    pub fn errors(&self) -> Receiver<RuntimeError> {
        self.errors.clone()
    }

    /// Returns the next typed stage error if one is queued, without blocking.
    pub fn try_error(&self) -> Option<RuntimeError> {
        self.errors.try_recv().ok()
    }

    /// Splits the handle into its input sender and output receiver, moving worker
    /// ownership to a supervisor thread that joins the stages when they finish.
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

    /// Closes the input side and joins every worker within `policy.timeout`;
    /// exceeding it returns [`RuntimeError::ShutdownTimeout`] listing the stages
    /// still pending rather than reporting success.
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

/// Owns a started pipeline's channels and worker join handles, so callers can
/// observe typed errors and acknowledge bounded shutdown; dropping it stops
/// intake and waits up to 250 ms for workers without blocking forever.
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
