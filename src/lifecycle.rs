//! Application lifecycle hooks: named session phases and ordered registration.
//! Startup phases run in registration order, teardown phases in reverse, on the
//! thread that called `render_with`; a panicking hook is contained.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::basic::AppHandle;
use crate::{RenderError, Size};

/// A named point in an application session, executed at most once and in the
/// order `Boot`, `Mount`, `Ready`, `Unmount`, `Exit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppPhase {
    /// After configuration validation and before any terminal side effect; no
    /// raw mode yet, so a failure here is an ordinary error.
    Boot,
    /// Terminal acquired (raw mode, alternate screen) but no frame presented; a
    /// failure here skips the event loop entirely.
    Mount,
    /// First frame presented; runs exactly once per session.
    Ready,
    /// Event loop ended while the terminal is still acquired; the last hook
    /// point that can write through the terminal session.
    Unmount,
    /// Terminal restored; the last hook point before `render_with` returns.
    Exit,
}

impl AppPhase {
    /// Every phase in execution order, for diagnostics and tests.
    pub const ALL: [AppPhase; 5] = [
        AppPhase::Boot,
        AppPhase::Mount,
        AppPhase::Ready,
        AppPhase::Unmount,
        AppPhase::Exit,
    ];

    /// Whether this phase runs in reverse registration order (`Unmount` or
    /// `Exit`, the "set up in order, tear down in reverse" rule).
    pub const fn is_teardown(self) -> bool {
        matches!(self, Self::Unmount | Self::Exit)
    }

    pub(crate) const fn index(self) -> u8 {
        match self {
            Self::Boot => 0,
            Self::Mount => 1,
            Self::Ready => 2,
            Self::Unmount => 3,
            Self::Exit => 4,
        }
    }
}

impl fmt::Display for AppPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Boot => "boot",
            Self::Mount => "mount",
            Self::Ready => "ready",
            Self::Unmount => "unmount",
            Self::Exit => "exit",
        })
    }
}

/// Why a session ended, as seen by the `Unmount` and `Exit` phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// The configured exit key was pressed.
    ExitKey,
    /// An application handle requested a graceful exit with `request_exit`,
    /// which stops the software without stranding the terminal.
    Requested,
    /// The runtime's channels closed before an exit key was seen.
    RuntimeClosed,
    /// A typed error ended the session (a stage failure, a callback fault, or
    /// an I/O or frame error); the returned error carries the detail.
    Failed,
    /// The session stopped before or during setup: a hook failed, or the
    /// terminal could not be acquired.
    Aborted,
}

impl fmt::Display for ExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ExitKey => "exit key",
            Self::Requested => "exit requested",
            Self::RuntimeClosed => "runtime closed",
            Self::Failed => "session failure",
            Self::Aborted => "aborted before the event loop",
        })
    }
}

/// Read-only facts handed to every lifecycle hook. The value is plain owned
/// data with no borrows, so a hook is a simple `FnOnce(&AppSession)` and can be
/// stored without lifetime parameters; a hook that needs runtime configuration
/// or an error should capture it when it is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppSession {
    /// The phase being executed.
    pub phase: AppPhase,
    /// The viewport measured before the session was entered.
    pub viewport: Size,
    /// Frames written in the event loop, not counting frames drained during
    /// shutdown.
    pub frames_presented: u64,
    /// `None` while the session is live; otherwise the reason the loop stopped,
    /// set for the `Unmount` and `Exit` phases.
    pub exit: Option<ExitReason>,
}

type LifecycleCallback = Box<dyn FnOnce(&AppSession) + 'static>;

/// Ordered application lifecycle hooks, registered with a builder; startup
/// phases run in registration order and teardown phases in reverse.
pub struct AppLifecycle {
    boot: Vec<LifecycleCallback>,
    mount: Vec<LifecycleCallback>,
    ready: Vec<LifecycleCallback>,
    unmount: Vec<LifecycleCallback>,
    exit: Vec<LifecycleCallback>,
}

impl Default for AppLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl AppLifecycle {
    /// Creates an empty lifecycle with no hooks; a session rendered with it
    /// behaves exactly like `render`.
    pub fn new() -> Self {
        Self {
            boot: Vec::new(),
            mount: Vec::new(),
            ready: Vec::new(),
            unmount: Vec::new(),
            exit: Vec::new(),
        }
    }

    /// Registers a hook to run once before any terminal side effect.
    pub fn on_boot(mut self, hook: impl FnOnce(&AppSession) + 'static) -> Self {
        self.boot.push(Box::new(hook));
        self
    }

    /// Registers a hook to run once after the terminal is acquired and before
    /// the event loop.
    pub fn on_mount(mut self, hook: impl FnOnce(&AppSession) + 'static) -> Self {
        self.mount.push(Box::new(hook));
        self
    }

    /// Registers a hook to run once after the first frame is presented.
    pub fn on_ready(mut self, hook: impl FnOnce(&AppSession) + 'static) -> Self {
        self.ready.push(Box::new(hook));
        self
    }

    /// Registers a hook to run once when the loop ends, while the terminal is
    /// still acquired. Hooks run in reverse registration order, so the last
    /// resource acquired in `on_boot` is released first.
    pub fn on_unmount(mut self, hook: impl FnOnce(&AppSession) + 'static) -> Self {
        self.unmount.push(Box::new(hook));
        self
    }

    /// Registers a hook to run once after the terminal is restored and before
    /// `render_with` returns. Hooks run in reverse registration order and are
    /// the place to write to the normal screen, flush logs, or persist state.
    pub fn on_exit(mut self, hook: impl FnOnce(&AppSession) + 'static) -> Self {
        self.exit.push(Box::new(hook));
        self
    }

    /// Whether no phase has a hook.
    pub fn is_empty(&self) -> bool {
        self.boot.is_empty()
            && self.mount.is_empty()
            && self.ready.is_empty()
            && self.unmount.is_empty()
            && self.exit.is_empty()
    }

    fn take(&mut self, phase: AppPhase) -> Vec<LifecycleCallback> {
        match phase {
            AppPhase::Boot => std::mem::take(&mut self.boot),
            AppPhase::Mount => std::mem::take(&mut self.mount),
            AppPhase::Ready => std::mem::take(&mut self.ready),
            AppPhase::Unmount => std::mem::take(&mut self.unmount),
            AppPhase::Exit => std::mem::take(&mut self.exit),
        }
    }
}

impl fmt::Debug for AppLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppLifecycle")
            .field("boot", &self.boot.len())
            .field("mount", &self.mount.len())
            .field("ready", &self.ready.len())
            .field("unmount", &self.unmount.len())
            .field("exit", &self.exit.len())
            .finish()
    }
}

/// Runs the application phases exactly once each, in order. This is the engine
/// behind `render_with`; it is public only so the crate's integration tests can
/// drive phase ordering without a real terminal, and it is not a stable
/// interface.
#[doc(hidden)]
pub struct PhaseRunner {
    lifecycle: AppLifecycle,
    viewport: Size,
    /// The session control handle shared with every component, so
    /// `cx.use_handle()` and the loop observe the same frame counter and exit
    /// request.
    handle: AppHandle,
    /// One bit per `AppPhase::index`, so a phase runs at most once even when two
    /// control-flow paths reach it.
    entered: u8,
    exit: Option<ExitReason>,
}

impl PhaseRunner {
    /// Creates a runner over a fresh session handle.
    pub fn new(lifecycle: AppLifecycle, viewport: Size) -> Self {
        Self::with_handle(AppHandle::default(), lifecycle, viewport)
    }

    /// Binds the runner to the handle the event loop watches, so a component's
    /// `request_exit` reaches the loop and is reported as
    /// `ExitReason::Requested`.
    pub fn with_handle(handle: AppHandle, lifecycle: AppLifecycle, viewport: Size) -> Self {
        Self {
            lifecycle,
            viewport,
            handle,
            entered: 0,
            exit: None,
        }
    }

    /// A clone of the session control handle the loop holds.
    pub fn handle(&self) -> AppHandle {
        self.handle.clone()
    }

    /// The viewport this session was measured at.
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// Why the loop stopped, once the loop has recorded it; `None` while the
    /// session is still running.
    pub fn exit_reason(&self) -> Option<ExitReason> {
        self.exit
    }

    /// The facts a hook in `phase` would see right now.
    pub fn session(&self, phase: AppPhase) -> AppSession {
        AppSession {
            phase,
            viewport: self.viewport,
            frames_presented: self.handle.frames_presented(),
            exit: self.exit,
        }
    }

    /// Records why the loop stopped; the `Unmount` and `Exit` phases report it.
    pub fn set_exit(&mut self, reason: ExitReason) {
        self.exit = Some(reason);
    }

    /// Counts one presented frame and runs the `Ready` phase on the first call.
    /// Returns the first hook fault, if any; a fault does not stop the caller by
    /// itself, and the event loop treats it as a session error so teardown
    /// still runs.
    pub fn note_frame_presented(&mut self) -> Option<RenderError> {
        let count = self.handle.note_frame();
        if count == 1 {
            self.run(AppPhase::Ready)
        } else {
            None
        }
    }

    /// Runs every hook registered for `phase`, at most once, and returns the
    /// first fault; a panicking hook is contained, so the remaining hooks in
    /// the phase still run. Startup phases run in registration order, teardown
    /// phases in reverse, and a reported fault's `index` is the hook's
    /// registration position within the phase.
    pub fn run(&mut self, phase: AppPhase) -> Option<RenderError> {
        let bit = 1u8 << phase.index();
        if self.entered & bit != 0 {
            return None;
        }
        self.entered |= bit;

        let mut callbacks: Vec<Option<LifecycleCallback>> =
            self.lifecycle.take(phase).into_iter().map(Some).collect();
        let count = callbacks.len();
        let session = self.session(phase);
        let mut fault = None;
        for step in 0..count {
            let index = if phase.is_teardown() {
                count - 1 - step
            } else {
                step
            };
            let Some(callback) = callbacks[index].take() else {
                continue;
            };
            if catch_unwind(AssertUnwindSafe(move || callback(&session))).is_err()
                && fault.is_none()
            {
                fault = Some(RenderError::Lifecycle { phase, index });
            }
        }
        fault
    }
}

impl fmt::Debug for PhaseRunner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PhaseRunner")
            .field("viewport", &self.viewport)
            .field("handle", &self.handle)
            .field("entered", &self.entered)
            .field("exit", &self.exit)
            .finish()
    }
}
