//! Application lifetime control: a cloneable [`AppHandle`] over shared session
//! state, so a component or worker thread can request a graceful exit that
//! restores the terminal instead of calling `std::process::exit`.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Shared state behind every [`AppHandle`] clone.
struct AppControl {
    exit_requested: AtomicBool,
    frames_presented: AtomicU64,
    started: Instant,
}

/// A cloneable control surface for the running session. Clone it into event
/// handlers and worker threads; every clone shares one session state.
#[derive(Clone)]
pub struct AppHandle {
    control: Arc<AppControl>,
}

impl AppHandle {
    /// A handle that observes its own session state. `render_with` installs the
    /// handle it watches into every component, so a handle obtained through
    /// `use_handle` is live; one created directly (or by `Lower::default`) is
    /// inert until a loop is wired to it.
    pub fn new() -> Self {
        Self {
            control: Arc::new(AppControl {
                exit_requested: AtomicBool::new(false),
                frames_presented: AtomicU64::new(0),
                started: Instant::now(),
            }),
        }
    }

    /// Ask the session to stop gracefully. The event loop observes the request
    /// on its next turn (immediately after event dispatch, and otherwise within
    /// one render wait), then runs the Unmount and Exit phases and restores the
    /// terminal. Safe to call from any thread.
    pub fn request_exit(&self) {
        self.control.exit_requested.store(true, Ordering::Relaxed);
    }

    /// Whether a graceful exit has been requested and not yet observed.
    pub fn exit_requested(&self) -> bool {
        self.control.exit_requested.load(Ordering::Relaxed)
    }

    /// Withdraw a pending request before the loop observes it. This is
    /// best-effort: once the loop has seen the request the session is already
    /// ending, so a cancellation after that point has no effect. It exists for
    /// flows such as an "are you sure?" prompt that requests, then reconsiders.
    pub fn cancel_exit_request(&self) {
        self.control.exit_requested.store(false, Ordering::Relaxed);
    }

    /// Frames presented so far in this session, excluding frames drained during
    /// shutdown.
    pub fn frames_presented(&self) -> u64 {
        self.control.frames_presented.load(Ordering::Relaxed)
    }

    /// Time since the session state was created, which is just before the Boot
    /// phase in a `render_with` call.
    pub fn uptime(&self) -> Duration {
        self.control.started.elapsed()
    }

    /// Count one presented frame and return the new total. Called by the phase
    /// runner on the loop thread.
    pub(crate) fn note_frame(&self) -> u64 {
        self.control
            .frames_presented
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }
}

impl Default for AppHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for AppHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppHandle")
            .field("exit_requested", &self.exit_requested())
            .field("frames_presented", &self.frames_presented())
            .finish()
    }
}
