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

use crate::{
    Commit, Component, ComponentContext, FrameError, Lower, Node, Props, Renderer, Runtime, Size,
};

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub exit_key: KeyEvent,
    pub poll_interval: Duration,
    pub alternate_screen: bool,
    pub mouse_capture: bool,
    pub bracketed_paste: bool,
    pub focus_change: bool,
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
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "terminal I/O failed: {error}"),
            Self::Frame(error) => write!(f, "rendering frame failed: {error}"),
            Self::RuntimeClosed => write!(f, "rendering runtime stopped unexpectedly"),
        }
    }
}

impl Error for RenderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::RuntimeClosed => None,
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

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(
            self.stdout,
            Show,
            ResetColor,
            SetAttribute(Attribute::Reset)
        );
        if self.config.mouse_capture {
            let _ = execute!(self.stdout, DisableMouseCapture);
        }
        if self.config.bracketed_paste {
            let _ = execute!(self.stdout, DisableBracketedPaste);
        }
        if self.config.focus_change {
            let _ = execute!(self.stdout, DisableFocusChange);
        }
        if self.config.alternate_screen {
            let _ = execute!(self.stdout, LeaveAlternateScreen);
        }
        let _ = self.stdout.flush();
        let _ = terminal::disable_raw_mode();
    }
}

fn root(_cx: &mut ComponentContext, props: &Props<Node>) -> Node {
    props.user_defined.clone()
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

pub fn render(node: impl Into<Node>, config: RuntimeConfig) -> Result<(), RenderError> {
    let viewport = terminal::size().map(|(width, height)| Size::new(width, height))?;
    let mut terminal = TerminalSession::enter(config.clone())?;
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let renderer = Renderer::try_new(viewport).map_err(RenderError::Frame)?;
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(renderer)
        .start();
    input
        .send(root.apply(node.into()))
        .map_err(|_| RenderError::RuntimeClosed)?;

    loop {
        if let Some(frame) = receive_frame(&output, config.poll_interval)? {
            terminal.write(&frame)?;
        }
        while event::poll(Duration::ZERO)? {
            let event = event::read()?;
            if matches!(&event, Event::Key(key) if is_exit_key(*key, config.exit_key)) {
                return Ok(());
            }
            dispatcher.dispatch(event);
        }
    }
}
