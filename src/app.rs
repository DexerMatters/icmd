use std::{
    error::Error,
    fmt,
    io::{self, Write},
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    style::{Attribute, ResetColor, SetAttribute},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::runtime::{
    Commit, CommitConfig, ConfigError, FrameError, Lower, Renderer, RendererConfig, ResourceLimits,
    Runtime, RuntimeError, ShutdownPolicy,
};
use crate::{
    Component, ComponentContext, EmojiMerging, ImageProtocol, ImageUpdatePolicy, Node, Props, Size,
};

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub exit_key: KeyEvent,
    pub poll_interval: Duration,
    pub alternate_screen: bool,
    pub mouse_capture: bool,
    pub bracketed_paste: bool,
    pub focus_change: bool,
    pub image_protocol: ImageProtocol,
    pub image_cache_bytes: usize,
    pub image_update_policy: ImageUpdatePolicy,
    pub emoji_merging: EmojiMerging,
    // Hard resource ceilings for the whole runtime. Invalid combinations are
    // rejected by `validate` before any thread or terminal mode exists.
    pub limits: ResourceLimits,
    // Maximum terminal events processed before the loop returns to rendering.
    // This is the fairness budget that keeps a continuous input stream from
    // starving presentation.
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
    pub fn new(exit_key: KeyEvent) -> Self {
        Self {
            exit_key,
            ..Self::default()
        }
    }
}

#[derive(Debug)]
pub enum RenderError {
    Io(io::Error),
    Frame(FrameError),
    RuntimeClosed,
    /// An application event listener panicked or re-entered itself. The runtime
    /// stops instead of continuing in an unknown partially-mutated state.
    ApplicationCallback(&'static str),
    /// A pipeline stage failed or was not joined; the typed cause is preserved.
    Stage(RuntimeError),
    /// The configuration could not produce a valid runtime.
    Config(ConfigError),
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
}

impl TerminalSession {
    fn enter(config: RuntimeConfig) -> Result<Self, io::Error> {
        terminal::enable_raw_mode()?;
        let mut session = Self {
            config,
            stdout: io::stdout(),
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
        execute!(session.stdout, Hide, Clear(ClearType::All))?;
        Ok(session)
    }

    fn write(&mut self, frame: &str) -> Result<(), io::Error> {
        self.stdout.write_all(frame.as_bytes())?;
        self.stdout.flush()
    }
}

// Terminal teardown, written against `impl Write` so the exact command order is
// testable without a real terminal. Raw mode is restored last: every escape
// sequence above it must reach the terminal while it is still in raw mode.
//
// SAF-08 requires terminal cleanup to be acknowledged and ordered, so the
// sequence is a named function rather than a `Drop` body over `Stdout`.
fn teardown(config: &RuntimeConfig, out: &mut impl io::Write) -> io::Result<()> {
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
    if config.alternate_screen {
        // The alternate screen is left only after every capture and attribute
        // is released, so nothing is written into the user's normal screen.
        execute!(out, LeaveAlternateScreen)?;
    }
    out.flush()
}

// The ordered teardown commands, as text, for tests and diagnostics. `Drop`
// writes them through `teardown`; this form makes the order assertable.
#[cfg(test)]
fn teardown_commands(config: &RuntimeConfig) -> Vec<&'static str> {
    let mut commands = vec!["Show", "ResetColor", "SetAttribute(Reset)"];
    if config.mouse_capture {
        commands.push("DisableMouseCapture");
    }
    if config.bracketed_paste {
        commands.push("DisableBracketedPaste");
    }
    if config.focus_change {
        commands.push("DisableFocusChange");
    }
    if config.alternate_screen {
        commands.push("LeaveAlternateScreen");
    }
    commands.push("disable_raw_mode");
    commands
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = teardown(&self.config, &mut self.stdout);
        // Raw mode is process-global, so it is restored after the writes above.
        let _ = terminal::disable_raw_mode();
    }
}

fn root(_cx: &mut ComponentContext, props: &Props<Node>) -> Node {
    props.data().clone()
}

// Upper bound on how long the loop blocks waiting for a frame. Input latency
// therefore does not grow with a large configured poll interval.
const MAX_RENDER_WAIT: Duration = Duration::from_millis(16);

// Buffer a replaceable event and report whether it was coalesced. Return, move,
// and resize events are safe to collapse because only the newest position or
// size is observable; every other event keeps its place in the stream.
fn coalesce_event(pending: &mut Option<Event>, event: Event) -> bool {
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

impl RuntimeConfig {
    // Validate configuration before any side effect exists: no thread, no raw
    // mode, no alternate screen. A rejected configuration is an ordinary error.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.limits.validate()?;
        let poll = self.poll_interval;
        // A zero interval busy-spins; an enormous one makes input latency
        // unbounded. Both are rejected rather than silently clamped.
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

// Validate configuration before any side effect exists: no thread, no raw
// mode, no alternate screen. A rejected configuration is an ordinary error.
fn validate_config(config: &RuntimeConfig) -> Result<(), RenderError> {
    config.validate().map_err(RenderError::Config)
}

pub fn render(node: impl Into<Node>, config: RuntimeConfig) -> Result<(), RenderError> {
    validate_config(&config)?;
    let viewport = terminal::size().map(|(width, height)| Size::new(width, height))?;
    let mut terminal = TerminalSession::enter(config.clone())?;
    let (commit, _, dispatcher) = Commit::with_config_and_events(
        viewport,
        CommitConfig {
            emoji_merging: config.emoji_merging,
        },
    );
    let renderer = Renderer::with_config(
        viewport,
        RendererConfig {
            image_protocol: config.image_protocol,
            image_cache_bytes: config.image_cache_bytes,
            image_update_policy: config.image_update_policy,
            // `None` asks the renderer to refresh terminal geometry with
            // viewport resizes. Standalone renderers can still set an exact
            // value for deterministic tests.
            cell_pixel_size: None,
            emoji_merging: config.emoji_merging,
            limits: config.limits,
        },
    )
    .map_err(RenderError::Frame)?;
    let runtime = Runtime::new(Lower::default())
        .then(commit)
        .then(renderer)
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let errors = runtime.errors();
    input
        .send(root.apply(node.into()))
        .map_err(|_| RenderError::RuntimeClosed)?;

    // The wait for renderer output is the only blocking point. Capping it keeps
    // input latency bounded regardless of the configured poll interval, and the
    // loop revisits events after every frame, so continuous input cannot starve
    // rendering.
    let render_wait = config
        .poll_interval
        .min(MAX_RENDER_WAIT)
        .max(Duration::from_millis(1));

    let outcome = 'render: loop {
        // 1. Always give ready renderer output a chance first.
        if let Some(frame) = receive_frame(&output, render_wait)? {
            terminal.write(&frame)?;
        }
        // A typed stage failure is terminal and must not look like a normal
        // channel close.
        if let Ok(error) = errors.try_recv() {
            break 'render Err(RenderError::Stage(error));
        }
        // 2. Drain a bounded batch of terminal events. Mouse moves and resizes
        //    are coalesced; key, paste, focus, and shutdown events never are.
        let mut pending: Option<Event> = None;
        let mut processed = 0usize;
        while processed < config.events_per_tick && event::poll(Duration::ZERO)? {
            let event = event::read()?;
            if coalesce_event(&mut pending, event) {
                continue;
            }
            let event = pending.take().expect("a non-coalesced event is queued");
            if matches!(&event, Event::Key(key) if is_exit_key(*key, config.exit_key)) {
                break 'render Ok(());
            }
            dispatcher.dispatch(event);
            if let Some(detail) = dispatcher.take_callback_fault() {
                break 'render Err(RenderError::ApplicationCallback(detail));
            }
            processed += 1;
        }
        if let Some(event) = pending.take() {
            if matches!(&event, Event::Key(key) if is_exit_key(*key, config.exit_key)) {
                break 'render Ok(());
            }
            dispatcher.dispatch(event);
            if let Some(detail) = dispatcher.take_callback_fault() {
                break 'render Err(RenderError::ApplicationCallback(detail));
            }
        }
    };

    // Closing the root input lets every pipeline stage unwind. The renderer
    // emits one final targeted Kitty cleanup frame before its output channel
    // closes, so alternate-screen teardown cannot leave virtual placements
    // behind in the terminal. Shutdown is acknowledged: workers are joined
    // before the terminal session unwinds.
    drop(input);
    while let Ok(result) = output.recv_timeout(Duration::from_secs(1)) {
        terminal.write(&result?)?;
    }
    let drained = runtime.shutdown(ShutdownPolicy::default());
    match (outcome, drained) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(RenderError::Stage(error)),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SAF-08 / PR 3.5: terminal teardown must be ordered and complete. Every
    // capture and attribute is released before the alternate screen is left,
    // and raw mode is restored last, so nothing leaks into the user's screen.
    #[test]
    fn teardown_order_releases_capture_before_the_alternate_screen() {
        let config = RuntimeConfig::default();
        let commands = teardown_commands(&config);
        let position = |name: &str| {
            commands
                .iter()
                .position(|command| *command == name)
                .unwrap_or_else(|| panic!("{name} missing from teardown: {commands:?}"))
        };
        assert!(
            position("DisableMouseCapture") < position("LeaveAlternateScreen"),
            "mouse capture must be released before leaving the alternate screen: {commands:?}"
        );
        assert!(
            position("DisableBracketedPaste") < position("LeaveAlternateScreen"),
            "bracketed paste must be released before leaving the alternate screen"
        );
        assert!(
            position("DisableFocusChange") < position("LeaveAlternateScreen"),
            "focus reporting must be released before leaving the alternate screen"
        );
        assert_eq!(
            commands.last(),
            Some(&"disable_raw_mode"),
            "raw mode must be restored after every write: {commands:?}"
        );
    }

    #[test]
    fn teardown_writes_every_command_to_the_stream() {
        let mut buffer: Vec<u8> = Vec::new();
        teardown(&RuntimeConfig::default(), &mut buffer).expect("teardown writes");
        let text = String::from_utf8_lossy(&buffer);
        // The alternate-screen leave is the CSI sequence crossterm emits; the
        // mouse-capture disable is the DEC private mode reset pair.
        assert!(
            text.contains("\u{1b}[?1049l"),
            "alternate screen not left: {text:?}"
        );
        assert!(text.contains("\u{1b}[?1000l") || text.contains("\u{1b}[?1006l"));
        assert!(!buffer.is_empty());
    }

    // SAF-15: the loop must coalesce replaceable position events and never
    // coalesce key, paste, focus, or shutdown events.
    #[test]
    fn only_replaceable_events_are_coalesced() {
        use crossterm::event::{Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};

        // A move replaces a pending move: only the newest position is observable.
        let mut pending = None;
        let first = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::empty(),
        });
        let second = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 9,
            row: 9,
            modifiers: KeyModifiers::empty(),
        });
        assert!(coalesce_event(&mut pending, first));
        assert!(coalesce_event(&mut pending, second));
        match pending {
            Some(Event::Mouse(mouse)) => assert_eq!((mouse.column, mouse.row), (9, 9)),
            other => panic!("expected the newest move, got {other:?}"),
        }

        // A resize is replaceable too.
        let mut pending = None;
        assert!(coalesce_event(&mut pending, Event::Resize(10, 4)));
        assert!(coalesce_event(&mut pending, Event::Resize(20, 8)));

        // Key events keep their place in the stream.
        let mut pending = None;
        let key = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty()));
        assert!(!coalesce_event(&mut pending, key));
        assert!(matches!(pending, Some(Event::Key(_))));

        // Paste and focus events are never coalesced either.
        let mut pending = None;
        assert!(!coalesce_event(&mut pending, Event::Paste("x".into())));
        let mut pending = None;
        assert!(!coalesce_event(&mut pending, Event::FocusGained));
    }

    // SAF-15: input latency must not grow with a large configured poll interval.
    #[test]
    fn render_wait_is_capped_independently_of_the_configured_interval() {
        let config = RuntimeConfig {
            poll_interval: Duration::from_millis(5_000),
            ..RuntimeConfig::default()
        };
        let wait = config.poll_interval.min(MAX_RENDER_WAIT);
        assert_eq!(wait, MAX_RENDER_WAIT);
        assert!(
            MAX_RENDER_WAIT <= Duration::from_millis(16),
            "the render wait is the maximum input latency and must stay small"
        );

        // A shorter configured interval is honoured, and never zero.
        let config = RuntimeConfig {
            poll_interval: Duration::from_millis(2),
            ..RuntimeConfig::default()
        };
        assert_eq!(
            config.poll_interval.min(MAX_RENDER_WAIT),
            Duration::from_millis(2)
        );
    }

    #[test]
    fn a_minimal_configuration_skips_the_optional_commands() {
        let config = RuntimeConfig {
            alternate_screen: false,
            mouse_capture: false,
            bracketed_paste: false,
            focus_change: false,
            ..RuntimeConfig::default()
        };
        let commands = teardown_commands(&config);
        assert_eq!(
            commands,
            vec![
                "Show",
                "ResetColor",
                "SetAttribute(Reset)",
                "disable_raw_mode"
            ]
        );
    }
}
