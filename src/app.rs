//! Application entry points: terminal session setup and teardown, the runtime
//! configuration and error types, and the event loop that drives a component
//! tree until exit. Re-exported at the crate root.

use std::{
    error::Error,
    fmt,
    io::{self, IsTerminal, Write},
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    style::{Attribute, ResetColor, SetAttribute},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::basic::selection::set_system_writer;
use crate::lifecycle::{AppLifecycle, AppPhase, ExitReason, PhaseRunner};
use crate::runtime::{
    Commit, CommitConfig, ConfigError, FrameError, Lower, Renderer, RendererConfig, ResourceLimits,
    Runtime, RuntimeError, ShutdownPolicy,
};
use crate::{
    AppHandle, Component, ComponentContext, EmojiMerging, ImageProtocol, ImageUpdatePolicy, Node,
    Props, Size,
};

/// Runtime behavior for one rendered session: input, terminal modes, image
/// handling, and resource limits. `Default` is the conventional interactive
/// setup, and every field can be overridden before rendering.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Key press that ends the session; defaults to Ctrl+C.
    pub exit_key: KeyEvent,
    /// How long the event loop may block between polls; must be non-zero and at
    /// most 60 seconds.
    pub poll_interval: Duration,
    /// Whether to enter the terminal's alternate screen and restore the normal
    /// screen on exit.
    pub alternate_screen: bool,
    /// Whether to enable mouse reporting and release it on exit.
    pub mouse_capture: bool,
    /// Whether to enable bracketed paste reporting and disable it on exit.
    pub bracketed_paste: bool,
    /// Whether to report focus gained and lost events.
    pub focus_change: bool,
    /// Whether to ask a terminal that supports the kitty keyboard protocol to
    /// report modified keys unambiguously; without it a terminal collapses
    /// Ctrl+Shift+C into Ctrl+C, making the clipboard chords indistinguishable
    /// from the exit key.
    pub enhanced_keyboard: bool,
    /// Whether to mirror every copy and cut into the terminal's own clipboard
    /// with an OSC 52 write, so the terminal's paste shortcut sees text copied
    /// inside the application. The framework's own paste chord works without
    /// it, but a terminal paste reads the system clipboard, which nothing else
    /// fills.
    pub system_clipboard: bool,
    /// Protocol used to transmit images to the terminal.
    pub image_protocol: ImageProtocol,
    /// Upper bound on decoded image bytes cached, in bytes; defaults to 64 MiB.
    pub image_cache_bytes: usize,
    /// How image updates are scheduled across frames.
    pub image_update_policy: ImageUpdatePolicy,
    /// How emoji sequences are merged during layout and rendering.
    pub emoji_merging: EmojiMerging,
    /// Hard resource ceilings for the whole runtime; invalid combinations are
    /// rejected by `validate` before any thread or terminal mode exists.
    pub limits: ResourceLimits,
    /// Maximum terminal events processed before the loop returns to rendering;
    /// the fairness budget that keeps a continuous input stream from starving
    /// presentation.
    pub events_per_tick: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            exit_key: KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            poll_interval: Duration::from_millis(50),
            alternate_screen: true,
            mouse_capture: true,
            bracketed_paste: true,
            focus_change: true,
            enhanced_keyboard: true,
            system_clipboard: true,
            image_protocol: ImageProtocol::Auto,
            image_cache_bytes: 64 * 1024 * 1024,
            image_update_policy: ImageUpdatePolicy::Adaptive,
            emoji_merging: EmojiMerging::default(),
            limits: ResourceLimits::default(),
            events_per_tick: 64,
        }
    }
}

impl RuntimeConfig {
    /// Creates a configuration with `exit_key` and every other field from
    /// `Default`.
    pub fn new(exit_key: KeyEvent) -> Self {
        Self {
            exit_key,
            ..Self::default()
        }
    }
}

/// Error returned when a session cannot start or ends abnormally.
#[derive(Debug)]
pub enum RenderError {
    /// Terminal input or output failed.
    Io(io::Error),
    /// A frame could not be produced or written.
    Frame(FrameError),
    /// The rendering runtime stopped unexpectedly.
    RuntimeClosed,
    /// An application event listener panicked or re-entered itself; the runtime
    /// stops instead of continuing in an unknown partially-mutated state.
    ApplicationCallback(&'static str),
    /// A pipeline stage failed or was not joined; the typed cause is preserved.
    Stage(RuntimeError),
    /// The configuration could not produce a valid runtime.
    Config(ConfigError),
    /// An application lifecycle hook panicked. The remaining hooks in the phase
    /// still ran and terminal teardown still completed.
    Lifecycle {
        /// The phase whose hook failed.
        phase: AppPhase,
        /// The hook's registration position within that phase.
        index: usize,
    },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "terminal I/O failed: {error}"),
            Self::Frame(error) => write!(f, "rendering frame failed: {error}"),
            Self::RuntimeClosed => write!(f, "rendering runtime stopped unexpectedly"),
            Self::ApplicationCallback(detail) => write!(f, "application callback failed: {detail}"),
            Self::Stage(error) => write!(f, "runtime stage failed: {error}"),
            Self::Config(error) => write!(f, "invalid runtime configuration: {error}"),
            Self::Lifecycle { phase, index } => {
                write!(
                    f,
                    "application lifecycle hook {index} failed during {phase}"
                )
            }
        }
    }
}

impl Error for RenderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::RuntimeClosed | Self::ApplicationCallback(_) => None,
            Self::Stage(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::Lifecycle { .. } => None,
        }
    }
}

impl From<io::Error> for RenderError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<FrameError> for RenderError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

struct TerminalSession {
    config: RuntimeConfig,
    stdout: io::Stdout,
    /// Whether this session pushed the keyboard enhancement flags, so teardown
    /// pops exactly what it pushed.
    keyboard_enhanced: bool,
}

impl TerminalSession {
    /// Enables raw mode, probes keyboard enhancement only when stdout is a
    /// terminal, applies the configured modes, then hides the cursor and clears
    /// the screen. The probe writes a query and waits up to two seconds for an
    /// answer, so it is skipped for output that cannot reply.
    fn enter(config: RuntimeConfig) -> Result<Self, io::Error> {
        terminal::enable_raw_mode()?;
        let keyboard_enhanced = config.enhanced_keyboard
            && io::stdout().is_terminal()
            && terminal::supports_keyboard_enhancement().unwrap_or(false);
        let mut session = Self {
            config,
            stdout: io::stdout(),
            keyboard_enhanced,
        };
        if session.config.alternate_screen {
            execute!(session.stdout, EnterAlternateScreen)?;
        }
        if session.config.mouse_capture {
            execute!(session.stdout, EnableMouseCapture)?;
        }
        if session.config.bracketed_paste {
            execute!(session.stdout, EnableBracketedPaste)?;
        }
        if session.config.focus_change {
            execute!(session.stdout, EnableFocusChange)?;
        }
        if session.keyboard_enhanced {
            execute!(
                session.stdout,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                )
            )?;
        }
        execute!(session.stdout, Hide, Clear(ClearType::All))?;
        Ok(session)
    }

    fn write(&mut self, frame: &str) -> Result<(), io::Error> {
        self.stdout.write_all(frame.as_bytes())?;
        self.stdout.flush()
    }
}

/// Writes the ordered terminal teardown for the modes enabled in `config`:
/// show the cursor and reset attributes, release the mouse, bracketed-paste,
/// focus and keyboard-enhancement captures (one pop per push), leave the
/// alternate screen, then flush. Written against `impl Write` so the exact
/// order is testable without a real terminal, and raw mode is restored by the
/// caller last, after every escape above has reached the still-raw terminal.
pub fn teardown(
    config: &RuntimeConfig,
    keyboard_enhanced: bool,
    out: &mut impl io::Write,
) -> io::Result<()> {
    execute!(out, Show, ResetColor, SetAttribute(Attribute::Reset))?;
    if config.mouse_capture {
        execute!(out, DisableMouseCapture)?;
    }
    if config.bracketed_paste {
        execute!(out, DisableBracketedPaste)?;
    }
    if config.focus_change {
        execute!(out, DisableFocusChange)?;
    }
    if keyboard_enhanced {
        execute!(out, PopKeyboardEnhancementFlags)?;
    }
    if config.alternate_screen {
        execute!(out, LeaveAlternateScreen)?;
    }
    out.flush()
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = teardown(&self.config, self.keyboard_enhanced, &mut self.stdout);
        let _ = terminal::disable_raw_mode();
    }
}

fn root(_cx: &mut ComponentContext, props: &Props<Node>) -> Node {
    props.data().clone()
}

/// Upper bound on how long the loop blocks waiting for a frame, in time; 16
/// milliseconds, so input latency does not grow with a large configured
/// `poll_interval`.
pub const MAX_RENDER_WAIT: Duration = Duration::from_millis(16);

/// Buffers a replaceable event in `pending` and reports whether it was
/// coalesced. Return, move, and resize events collapse because only the newest
/// position or size is observable; every other event keeps its place in the
/// stream.
pub fn coalesce_event(pending: &mut Option<Event>, event: Event) -> bool {
    match &event {
        Event::Mouse(mouse) if matches!(mouse.kind, crossterm::event::MouseEventKind::Moved) => {
            *pending = Some(event);
            true
        }
        Event::Resize(_, _) => {
            *pending = Some(event);
            true
        }
        _ => {
            *pending = Some(event);
            false
        }
    }
}

fn is_exit_key(event: KeyEvent, expected: KeyEvent) -> bool {
    event.kind == KeyEventKind::Press
        && event.code == expected.code
        && event.modifiers == expected.modifiers
}

fn receive_frame(
    output: &Receiver<Result<String, FrameError>>,
    timeout: Duration,
) -> Result<Option<String>, RenderError> {
    match output.recv_timeout(timeout) {
        Ok(Ok(frame)) => Ok(Some(frame)),
        Ok(Err(error)) => Err(error.into()),
        Err(RecvTimeoutError::Timeout) => Ok(None),
        Err(RecvTimeoutError::Disconnected) => Err(RenderError::RuntimeClosed),
    }
}

fn render_error_from_runtime(error: RuntimeError) -> RenderError {
    match error {
        RuntimeError::ApplicationCallback(detail) => RenderError::ApplicationCallback(detail),
        error => RenderError::Stage(error),
    }
}

impl RuntimeConfig {
    /// Rejects an invalid configuration before any side effect exists: no
    /// thread, no raw mode, no alternate screen. A zero `poll_interval`
    /// busy-spins and one above 60 seconds makes input latency unbounded, so
    /// both are rejected rather than silently clamped, as is a zero
    /// `events_per_tick`.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.limits.validate()?;
        let poll = self.poll_interval;
        if poll.is_zero() || poll > Duration::from_secs(60) {
            return Err(ConfigError::InvalidPollInterval {
                millis: poll.as_millis(),
            });
        }
        if self.events_per_tick == 0 {
            return Err(ConfigError::InvalidEventsPerTick);
        }
        Ok(())
    }
}

/// Validates a configuration before any side effect exists, mapping the cause
/// to `RenderError::Config`.
fn validate_config(config: &RuntimeConfig) -> Result<(), RenderError> {
    config.validate().map_err(RenderError::Config)
}

/// Renders a tree until the configured exit key, with no lifecycle hooks; this
/// is `render_with` with an empty `AppLifecycle`, so an application that
/// registers no hooks is unaffected.
pub fn render(node: impl Into<Node>, config: RuntimeConfig) -> Result<(), RenderError> {
    render_with(node, config, AppLifecycle::new())
}

/// Renders a tree with application lifecycle hooks, whose phases and ordering
/// are documented in `lifecycle`. Every graceful termination path runs the
/// teardown phases, so terminal teardown is never stranded by a failing hook.
/// Validation runs before any side effect (no thread, raw mode, alternate
/// screen, or hook), system clipboard mirroring is installed for the session,
/// and the handle the loop watches is the one every component receives through
/// `cx.use_handle()`.
pub fn render_with(
    node: impl Into<Node>,
    config: RuntimeConfig,
    lifecycle: AppLifecycle,
) -> Result<(), RenderError> {
    validate_config(&config)?;
    let viewport = terminal::size().map(|(width, height)| Size::new(width, height))?;
    let _system_clipboard =
        SystemClipboard::install(config.system_clipboard && io::stdout().is_terminal());
    let terminal_config = config.clone();
    drive_session_with(
        AppHandle::default(),
        lifecycle,
        viewport,
        move || TerminalSession::enter(terminal_config).map_err(RenderError::from),
        move |runner, terminal| {
            run_session(node.into(), &config, runner, &mut |frame| {
                terminal.write(frame)
            })
        },
    )
}

/// Mirrors copies into the terminal's own clipboard for the life of a session.
/// A terminal application cannot read the system clipboard, but it can ask the
/// terminal to set it with an OSC 52 sequence; without this, Ctrl+Shift+C fills
/// only the framework's process-local buffer and the terminal's paste shortcut
/// pastes whatever the system clipboard already held. The writer is cleared on
/// drop so it never outlives the terminal it writes to.
struct SystemClipboard;

impl SystemClipboard {
    /// Installs the OSC 52 writer when `enabled`, otherwise installs nothing.
    fn install(enabled: bool) -> Self {
        if enabled {
            set_system_writer(Some(Arc::new(|text: &str| {
                let mut stdout = io::stdout();
                let _ = stdout.write_all(terminal_clipboard_sequence(text).as_bytes());
                let _ = stdout.flush();
            })));
        }
        Self
    }
}

impl Drop for SystemClipboard {
    fn drop(&mut self) {
        set_system_writer(None);
    }
}

/// The exact OSC 52 sequence that sets the terminal's clipboard from `text`,
/// built whole so the terminal never sees a partial escape and separated from
/// the write so the payload is testable. Public only for the crate's tests, not
/// a stable interface.
pub fn terminal_clipboard_sequence(text: &str) -> String {
    use crossterm::Command;
    use crossterm::clipboard::CopyToClipboard;
    let mut sequence = String::new();
    let _ = CopyToClipboard::to_clipboard_from(text).write_ansi(&mut sequence);
    sequence
}

/// The phase state machine. `enter` acquires whatever session resource the
/// caller needs (the terminal, in production) and it is dropped at the teardown
/// point between `Unmount` and `Exit`, so a test can substitute a drop-logging
/// token and still observe the ordering; `body` drives the event loop and
/// reports presented frames through `PhaseRunner::note_frame_presented`, which
/// runs the `Ready` phase once. Every path after a successful `enter` runs the
/// teardown sequence, and a failing hook never strands the terminal. Public only
/// for the crate's integration tests, not a stable interface.
pub fn drive_session<T>(
    lifecycle: AppLifecycle,
    viewport: Size,
    enter: impl FnOnce() -> Result<T, RenderError>,
    body: impl FnOnce(&mut PhaseRunner, &mut T) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    drive_session_with(AppHandle::default(), lifecycle, viewport, enter, body)
}

/// `drive_session` with an explicit session handle, so the handle the loop
/// watches is the one installed into the component tree. A `Boot` fault leaves
/// nothing to unmount, so only `Exit` still applies; if the terminal was never
/// acquired, `Exit` still runs for resources a `Boot` hook opened. A reason
/// already recorded by the loop is kept, and `Unmount` runs while the session
/// resource is still acquired, before that resource is dropped and `Exit` runs.
pub fn drive_session_with<T>(
    handle: AppHandle,
    lifecycle: AppLifecycle,
    viewport: Size,
    enter: impl FnOnce() -> Result<T, RenderError>,
    body: impl FnOnce(&mut PhaseRunner, &mut T) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    let mut runner = PhaseRunner::with_handle(handle, lifecycle, viewport);

    let boot_error = runner.run(AppPhase::Boot);
    if boot_error.is_some() {
        runner.set_exit(ExitReason::Aborted);
        return first_error(boot_error, None, runner.run(AppPhase::Exit));
    }

    let mut session = match enter() {
        Ok(session) => session,
        Err(error) => {
            runner.set_exit(ExitReason::Aborted);
            return first_error(Some(error), None, runner.run(AppPhase::Exit));
        }
    };

    let mut setup_error = runner.run(AppPhase::Mount);
    if setup_error.is_none() {
        setup_error = body(&mut runner, &mut session).err();
    }
    if runner.exit_reason().is_none() {
        let exit_reason = match &setup_error {
            Some(RenderError::RuntimeClosed) => ExitReason::RuntimeClosed,
            Some(_) => ExitReason::Failed,
            None => ExitReason::ExitKey,
        };
        runner.set_exit(exit_reason);
    }

    let teardown_error = runner.run(AppPhase::Unmount);
    drop(session);
    let exit_error = runner.run(AppPhase::Exit);
    first_error(setup_error, teardown_error, exit_error)
}

/// Returns the first error chronologically, so a teardown fault never masks a
/// setup or loop error that already stopped the session.
fn first_error(
    primary: Option<RenderError>,
    teardown: Option<RenderError>,
    exit: Option<RenderError>,
) -> Result<(), RenderError> {
    match primary.or(teardown).or(exit) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Runs the event loop for one session until exit and returns after the
/// pipeline shuts down; `write` receives every frame the loop presents, so
/// production passes the terminal and a test can pass a sink that needs no
/// terminal. A renderer construction failure returns before anything starts, so
/// there is nothing to unwind. The loop and every component share one handle,
/// so `request_exit` stops the session the same way the exit key does. Ready
/// renderer output is given first, then a bounded batch of terminal events is
/// drained with mouse moves and resizes coalesced, and the wait per frame is
/// capped so continuous input cannot starve rendering; `cell_pixel_size: None`
/// asks the renderer to refresh terminal geometry on viewport resizes, while
/// standalone renderers may set an exact value for deterministic tests. On exit
/// the root input and the handle's own sender are closed so every stage
/// unwinds, remaining frames are drained for at most five seconds so a wedged
/// stage cannot hold the terminal hostage, then the runtime is shut down; a
/// drain failure does not replace the error that ended the loop, and the
/// renderer's final Kitty cleanup frame keeps alternate-screen teardown from
/// leaving virtual placements behind. Public only for the crate's tests, not a
/// stable interface.
pub fn run_session(
    node: Node,
    config: &RuntimeConfig,
    runner: &mut PhaseRunner,
    write: &mut dyn FnMut(&str) -> io::Result<()>,
) -> Result<(), RenderError> {
    let viewport = runner.viewport();
    let handle = runner.handle();
    let (commit, _, dispatcher) = Commit::with_config_and_events(
        viewport,
        CommitConfig {
            emoji_merging: config.emoji_merging,
        },
    );
    let renderer = match Renderer::with_config(
        viewport,
        RendererConfig {
            image_protocol: config.image_protocol,
            image_cache_bytes: config.image_cache_bytes,
            image_update_policy: config.image_update_policy,
            cell_pixel_size: None,
            emoji_merging: config.emoji_merging,
            limits: config.limits,
        },
    ) {
        Ok(renderer) => renderer,
        Err(error) => return Err(RenderError::Frame(error)),
    };
    let mut runtime = Runtime::new(Lower::default().with_handle(handle.clone()))
        .then(commit)
        .then(renderer)
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let errors = runtime.errors();

    let mut outcome: Result<(), RenderError> = match input.send(root.apply(node)) {
        Ok(()) => Ok(()),
        Err(_) => Err(RenderError::RuntimeClosed),
    };

    let render_wait = config
        .poll_interval
        .min(MAX_RENDER_WAIT)
        .max(Duration::from_millis(1));

    if outcome.is_ok() {
        'render: loop {
            if handle.exit_requested() {
                runner.set_exit(ExitReason::Requested);
                break 'render;
            }
            match receive_frame(&output, render_wait) {
                Ok(Some(frame)) => {
                    if let Err(error) = write(&frame) {
                        outcome = Err(RenderError::Io(error));
                        break 'render;
                    }
                    if let Some(error) = runner.note_frame_presented() {
                        outcome = Err(error);
                        break 'render;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    outcome = match errors.try_recv() {
                        Ok(runtime_error) => Err(render_error_from_runtime(runtime_error)),
                        Err(_) => Err(error),
                    };
                    break 'render;
                }
            }
            if let Ok(error) = errors.try_recv() {
                outcome = Err(render_error_from_runtime(error));
                break 'render;
            }
            if handle.exit_requested() {
                runner.set_exit(ExitReason::Requested);
                break 'render;
            }
            let mut pending: Option<Event> = None;
            let mut processed = 0usize;
            while processed < config.events_per_tick {
                match event::poll(Duration::ZERO) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(error) => {
                        outcome = Err(RenderError::Io(error));
                        break 'render;
                    }
                }
                let event = match event::read() {
                    Ok(event) => event,
                    Err(error) => {
                        outcome = Err(RenderError::Io(error));
                        break 'render;
                    }
                };
                if coalesce_event(&mut pending, event) {
                    continue;
                }
                let event = pending.take().expect("a non-coalesced event is queued");
                if matches!(&event, Event::Key(key) if is_exit_key(*key, config.exit_key)) {
                    break 'render;
                }
                dispatcher.dispatch(event);
                if let Some(detail) = dispatcher.take_callback_fault() {
                    outcome = Err(RenderError::ApplicationCallback(detail));
                    break 'render;
                }
                if handle.exit_requested() {
                    runner.set_exit(ExitReason::Requested);
                    break 'render;
                }
                processed += 1;
            }
            if let Some(event) = pending.take() {
                if matches!(&event, Event::Key(key) if is_exit_key(*key, config.exit_key)) {
                    break 'render;
                }
                dispatcher.dispatch(event);
                if let Some(detail) = dispatcher.take_callback_fault() {
                    outcome = Err(RenderError::ApplicationCallback(detail));
                    break 'render;
                }
                if handle.exit_requested() {
                    runner.set_exit(ExitReason::Requested);
                    break 'render;
                }
            }
        }
    }

    drop(input);
    runtime.close_input();
    let drain_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match output.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(frame)) => {
                if let Err(error) = write(&frame)
                    && outcome.is_ok()
                {
                    outcome = Err(RenderError::Io(error));
                }
            }
            Ok(Err(error)) => {
                if outcome.is_ok() {
                    outcome = Err(error.into());
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if Instant::now() >= drain_deadline {
                    break;
                }
            }
        }
        if let Ok(error) = errors.try_recv()
            && outcome.is_ok()
        {
            outcome = Err(render_error_from_runtime(error));
        }
    }
    let drained = runtime.shutdown(ShutdownPolicy::default());
    match (outcome, drained) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(render_error_from_runtime(error)),
        (Ok(()), Ok(())) => Ok(()),
    }
}
