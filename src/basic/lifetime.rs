//! Application lifetime control: a cloneable [`AppHandle`] over shared session
//! state, so a component or worker thread can request a graceful exit that
//! restores the terminal instead of calling `std::process::exit`.

use std::{
    collections::HashMap,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;

use super::events::{FocusError, FocusOutcome};
use crate::{DomId, Size, StateRef};

/// The runtime services a session handle exposes to components without making
/// the context tier depend on the runtime tier.
#[doc(hidden)]
pub trait FocusControl: Send + Sync {
    /// The committed viewport.
    fn viewport(&self) -> Size;
    /// Requests a repaint of the current DOM.
    fn request_redraw(&self);
    /// Moves focus to `id`.
    fn focus(&self, id: DomId) -> bool;
    /// Moves focus with a typed rejection.
    fn try_focus(&self, id: DomId) -> Result<FocusOutcome, FocusError>;
    /// The current focus owner.
    fn focused(&self) -> Option<DomId>;
    /// Clears focus.
    fn blur(&self) -> bool;
}

/// One frame-clock callback, replaceable on every render while a tick may be
/// reading the previous callback.
pub(crate) type FrameCallback = Arc<Mutex<Box<dyn FnMut(Duration) + Send>>>;

/// The lowerer's wake target, shared with the session handle.
#[derive(Clone)]
pub(crate) struct FrameWaker {
    pub(crate) wake: Sender<()>,
    pub(crate) rerender_all: Arc<AtomicBool>,
}

/// Fallback viewport for a detached handle, used by tests that mount a
/// pipeline without a terminal.
const DEFAULT_VIEWPORT: Size = Size {
    width: 80,
    height: 24,
};

/// Shared state behind every [`AppHandle`] clone.
struct AppControl {
    exit_requested: AtomicBool,
    frames_presented: AtomicU64,
    started: Instant,
    /// The runtime services the live session owns; detached handles leave this
    /// empty.
    services: Mutex<Option<Arc<dyn FocusControl>>>,
    /// Wakes the lowerer and requests a full component rerender.
    frame_waker: Mutex<Option<FrameWaker>>,
    /// Registered frame clocks, keyed by id.
    frame_clocks: Mutex<HashMap<u64, Arc<Mutex<FrameClockState>>>>,
    next_frame_clock: AtomicU64,
    frame_clock_fault: AtomicBool,
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
                services: Mutex::new(None),
                frame_waker: Mutex::new(None),
                frame_clocks: Mutex::new(HashMap::new()),
                next_frame_clock: AtomicU64::new(1),
                frame_clock_fault: AtomicBool::new(false),
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

    /// Registers a clock shared with one component and returns its id.
    pub(crate) fn register_frame_clock(&self, clock: Arc<Mutex<FrameClockState>>) -> u64 {
        let id = self
            .control
            .next_frame_clock
            .fetch_add(1, Ordering::Relaxed);
        let interval = clock.lock().expect("frame clock poisoned").interval;
        clock.lock().expect("frame clock poisoned").next_due = Some(Instant::now() + interval);
        self.control
            .frame_clocks
            .lock()
            .expect("frame clocks poisoned")
            .insert(id, clock);
        id
    }

    pub(crate) fn unregister_frame_clock(&self, id: u64) {
        self.control
            .frame_clocks
            .lock()
            .expect("frame clocks poisoned")
            .remove(&id);
    }

    /// Duration until the next due frame clock, or `None` when no clock is
    /// active.
    #[doc(hidden)]
    pub fn next_frame_wait(&self, now: Instant) -> Option<Duration> {
        let clocks = self
            .control
            .frame_clocks
            .lock()
            .expect("frame clocks poisoned");
        let mut earliest: Option<Instant> = None;
        for clock in clocks.values() {
            let state = clock.lock().expect("frame clock poisoned");
            if !state.active {
                continue;
            }
            let due = state.next_due.unwrap_or(now);
            earliest = Some(earliest.map_or(due, |current| current.min(due)));
        }
        earliest.map(|due| due.saturating_duration_since(now))
    }

    /// Runs every active clock whose deadline has passed. Returns whether any
    /// callback ran.
    #[doc(hidden)]
    pub fn tick_frame_clocks(&self, now: Instant) -> bool {
        let clocks: Vec<_> = {
            let map = self
                .control
                .frame_clocks
                .lock()
                .expect("frame clocks poisoned");
            map.values().cloned().collect()
        };
        let mut ran = false;
        for clock in clocks {
            let (callback, signal, dt) = {
                let mut state = clock.lock().expect("frame clock poisoned");
                if !state.active {
                    continue;
                }
                let due = state.next_due.unwrap_or(now);
                if now < due {
                    continue;
                }
                let dt = state
                    .last_run
                    .map_or(state.interval, |last| now.saturating_duration_since(last))
                    .min(state.interval.saturating_mul(4));
                state.last_run = Some(now);
                state.next_due = Some(now + state.interval);
                (state.callback.clone(), state.signal.clone(), dt)
            };
            if let Some(callback) = callback {
                let mut callback = callback.lock().expect("frame clock callback poisoned");
                let outcome = catch_unwind(AssertUnwindSafe(|| callback(dt)));
                drop(callback);
                if outcome.is_err() {
                    let mut state = clock.lock().expect("frame clock poisoned");
                    state.active = false;
                    state.callback = None;
                    self.control.frame_clock_fault.store(true, Ordering::SeqCst);
                }
            }
            /*
             * A clock forces its own component pass even when the callback did
             * not touch a state setter.
             */
            let _ = signal.update(|value| *value = value.wrapping_add(1));
            ran = true;
        }
        ran
    }

    #[doc(hidden)]
    pub fn take_frame_clock_fault(&self) -> bool {
        self.control.frame_clock_fault.swap(false, Ordering::SeqCst)
    }

    /// Wires the dispatcher the session's event loop owns.
    ///
    /// Called once by `run_session`; a handle created directly stays inert so
    /// tests can mount a pipeline without a terminal.
    #[doc(hidden)]
    pub fn attach_services(&self, services: Arc<dyn FocusControl>) {
        *self.control.services.lock().expect("app services poisoned") = Some(services);
    }

    pub(crate) fn attach_frame_waker(&self, waker: FrameWaker) {
        *self
            .control
            .frame_waker
            .lock()
            .expect("frame waker poisoned") = Some(waker);
    }

    /// Asks for one more frame: every component rerenders and the current
    /// scene repaints. Safe to call from any thread.
    ///
    /// The request is observed within one render wait. It is the one-shot
    /// primitive for animation and externally triggered repaints; use
    /// [`ComponentContext::use_frame_clock`] for continuous animation.
    ///
    /// [`ComponentContext::use_frame_clock`]: crate::ComponentContext::use_frame_clock
    pub fn request_frame(&self) {
        let waker = self
            .control
            .frame_waker
            .lock()
            .expect("frame waker poisoned")
            .clone();
        if let Some(waker) = waker {
            waker.rerender_all.store(true, Ordering::SeqCst);
            let _ = waker.wake.try_send(());
        }
        if let Some(services) = self
            .control
            .services
            .lock()
            .expect("app services poisoned")
            .as_ref()
        {
            services.request_redraw();
        }
    }

    /// The committed viewport size, or the detached-pipeline default.
    pub fn viewport(&self) -> Size {
        self.control
            .services
            .lock()
            .expect("app services poisoned")
            .as_ref()
            .map_or(DEFAULT_VIEWPORT, |services| services.viewport())
    }

    /// Moves DOM focus to `id`, reporting whether a region accepted it.
    pub fn focus(&self, id: DomId) -> bool {
        self.control
            .services
            .lock()
            .expect("app services poisoned")
            .as_ref()
            .is_some_and(|services| services.focus(id))
    }

    /// Moves DOM focus with a typed rejection for unknown or non-focusable
    /// targets.
    pub fn try_focus(&self, id: DomId) -> Result<FocusOutcome, FocusError> {
        match self
            .control
            .services
            .lock()
            .expect("app services poisoned")
            .as_ref()
        {
            Some(services) => services.try_focus(id),
            None => Err(FocusError::UnknownTarget(id)),
        }
    }

    /// Returns the DOM id that currently owns focus, if any.
    pub fn focused(&self) -> Option<DomId> {
        self.control
            .services
            .lock()
            .expect("app services poisoned")
            .as_ref()
            .and_then(|services| services.focused())
    }

    /// Removes focus from the current region, reporting whether one held it.
    pub fn blur(&self) -> bool {
        self.control
            .services
            .lock()
            .expect("app services poisoned")
            .as_ref()
            .is_some_and(|services| services.blur())
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

/// The per-clock callback and pacing state.
pub(crate) struct FrameClockState {
    pub(crate) active: bool,
    pub(crate) interval: Duration,
    pub(crate) next_due: Option<Instant>,
    pub(crate) last_run: Option<Instant>,
    pub(crate) callback: Option<FrameCallback>,
    pub(crate) signal: StateRef<u64>,
}

impl FrameClockState {
    pub(crate) fn new(signal: StateRef<u64>) -> Self {
        Self {
            active: true,
            interval: Duration::from_millis(16),
            next_due: None,
            last_run: None,
            callback: None,
            signal,
        }
    }
}

/// A live frame clock returned by
/// [`ComponentContext::use_frame_clock`](crate::ComponentContext::use_frame_clock).
#[derive(Clone)]
pub struct FrameClock {
    pub(crate) state: Arc<Mutex<FrameClockState>>,
}

impl FrameClock {
    /// Stops ticks until [`FrameClock::resume`].
    pub fn pause(&self) {
        self.state.lock().expect("frame clock poisoned").active = false;
    }

    /// Resumes ticks at the current interval.
    pub fn resume(&self) {
        let mut state = self.state.lock().expect("frame clock poisoned");
        state.active = true;
        state.next_due = Some(Instant::now() + state.interval);
    }

    /// Whether the clock is currently ticking.
    pub fn is_active(&self) -> bool {
        self.state.lock().expect("frame clock poisoned").active
    }

    /// Sets the target interval, clamped to `1ms..=1s`.
    pub fn interval(&self, interval: Duration) {
        let interval = interval.clamp(Duration::from_millis(1), Duration::from_secs(1));
        let mut state = self.state.lock().expect("frame clock poisoned");
        state.interval = interval;
        state.next_due = Some(Instant::now() + interval);
    }

    /// The current target interval.
    pub fn current_interval(&self) -> Duration {
        self.state.lock().expect("frame clock poisoned").interval
    }
}

impl fmt::Debug for FrameClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameClock")
            .field("active", &self.is_active())
            .field("interval", &self.current_interval())
            .finish()
    }
}
