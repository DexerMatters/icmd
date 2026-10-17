//! A supported headless harness for widget tests.
//!
//! [`Harness`] drives the real lower/commit pipeline and snapshots the painted
//! cell grid, so tests can assert on what a terminal would show without a
//! terminal, ANSI parsing, or access to `__private`.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::advanced::{
    Cell, CellSlot, Commit, EventDispatcher, Frame, Lower, PipelineComponent, Renderer, Runtime,
    RuntimeError, RuntimeHandle, Stage,
};
use crate::{AppHandle, DomId, Node, Size};

/// A painted screen snapshot.
#[derive(Clone, Debug)]
pub struct Screen {
    width: usize,
    height: usize,
    cells: Vec<CellSlot>,
}

impl Screen {
    fn from_renderer(renderer: &Renderer) -> Self {
        Self {
            width: usize::from(renderer.viewport().width),
            height: usize::from(renderer.viewport().height),
            cells: renderer.presented_cells().to_vec(),
        }
    }

    /// Viewport width in cells.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Viewport height in cells.
    pub fn height(&self) -> usize {
        self.height
    }

    /// The cell at `(line, column)`, or `None` outside the viewport.
    pub fn cell(&self, line: usize, column: usize) -> Option<Cell> {
        if line >= self.height || column >= self.width {
            return None;
        }
        Some(self.cells[line * self.width + column].cell().clone())
    }

    /// A row's text, skipping wide-glyph continuation cells.
    pub fn row(&self, line: usize) -> String {
        if line >= self.height {
            return String::new();
        }
        let mut out = String::new();
        for column in 0..self.width {
            match &self.cells[line * self.width + column] {
                CellSlot::Lead(cell) => out.push_str(cell.symbol()),
                CellSlot::Continuation(_) => {}
            }
        }
        out
    }

    /// Every row, with trailing whitespace preserved.
    pub fn rows(&self) -> Vec<String> {
        (0..self.height).map(|line| self.row(line)).collect()
    }

    /// Every row joined with newlines.
    pub fn text(&self) -> String {
        self.rows().join("\n")
    }
}

struct Shared {
    screen: Mutex<Screen>,
    frames: Mutex<u64>,
}

/// Renderer substitute that snapshots the composed grid and notifies the
/// harness.
struct CaptureStage {
    renderer: Renderer,
    shared: Arc<Shared>,
    notify: Sender<()>,
}

impl PipelineComponent for CaptureStage {
    type Input = Frame;
    type Output = ();

    const STAGE: Stage = Stage::Renderer;

    fn run(
        mut self,
        input: Receiver<Self::Input>,
        _output: Sender<Self::Output>,
        errors: Sender<RuntimeError>,
    ) -> Result<(), RuntimeError> {
        while let Ok(frame) = input.recv() {
            if let Err(error) = self.renderer.apply_frame(frame) {
                let _ = errors.send(RuntimeError::Frame(error));
                break;
            }
            if let Err(error) = self.renderer.render_diff() {
                let _ = errors.send(RuntimeError::Frame(error));
                break;
            }
            *self.shared.screen.lock().expect("harness screen poisoned") =
                Screen::from_renderer(&self.renderer);
            if let Ok(mut frames) = self.shared.frames.lock() {
                *frames += 1;
            }
            let _ = self.notify.try_send(());
        }
        Ok(())
    }
}

/// Headless widget harness over the real pipeline.
pub struct Harness {
    runtime: RuntimeHandle<Node, ()>,
    output: Receiver<()>,
    errors: Receiver<RuntimeError>,
    dispatcher: EventDispatcher,
    handle: AppHandle,
    viewport: Size,
    shared: Arc<Shared>,
}

impl Harness {
    /// Creates a harness at `viewport` cells.
    pub fn new(viewport: Size) -> Self {
        let (commit, _, dispatcher) = Commit::new_with_events(viewport);
        let handle = AppHandle::default();
        handle.attach_services(Arc::new(dispatcher.clone()));
        let (notify, output) = crossbeam_channel::unbounded();
        let shared = Arc::new(Shared {
            screen: Mutex::new(Screen {
                width: usize::from(viewport.width),
                height: usize::from(viewport.height),
                cells: Vec::new(),
            }),
            frames: Mutex::new(0),
        });
        let stage = CaptureStage {
            renderer: Renderer::new(viewport).expect("renderer"),
            shared: shared.clone(),
            notify,
        };
        let runtime = Runtime::new(Lower::default().with_handle(handle.clone()))
            .then(commit)
            .then(stage)
            .start_handle();
        let errors = runtime.errors();
        Self {
            runtime,
            output,
            errors,
            dispatcher,
            handle,
            viewport,
            shared,
        }
    }

    /// The viewport the harness was created with.
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// The dispatcher backing the harness.
    pub fn dispatcher(&self) -> &EventDispatcher {
        &self.dispatcher
    }

    /// The session handle backing the harness.
    pub fn handle(&self) -> &AppHandle {
        &self.handle
    }

    /// Mounts a root tree and drains the resulting frames.
    pub fn mount(&mut self, node: impl Into<Node>) {
        self.runtime
            .input()
            .send(node.into())
            .expect("harness root accepted");
        self.pump();
    }

    /// Sends a new root tree and drains the resulting frames.
    pub fn update(&mut self, node: impl Into<Node>) {
        self.mount(node);
    }

    /// Drains every frame currently in flight.
    pub fn pump(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match self.output.recv_timeout(Duration::from_millis(25)) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        if let Ok(error) = self.errors.try_recv() {
            panic!("harness pipeline failed: {error}");
        }
    }

    /// Copies the current painted screen.
    pub fn screen(&self) -> Screen {
        self.shared
            .screen
            .lock()
            .expect("harness screen poisoned")
            .clone()
    }

    /// The painted rows.
    pub fn rows(&self) -> Vec<String> {
        self.screen().rows()
    }

    /// The painted text.
    pub fn text(&self) -> String {
        self.screen().text()
    }

    /// Frames composed so far.
    pub fn frames(&self) -> u64 {
        *self.shared.frames.lock().expect("harness frames poisoned")
    }

    /// The DOM id that currently owns focus.
    pub fn focused(&self) -> Option<DomId> {
        self.dispatcher.focused()
    }

    /// Dispatches one terminal event and drains frames.
    pub fn dispatch(&mut self, event: Event) {
        self.dispatcher.dispatch(event);
        self.pump();
    }

    /// Presses one key.
    pub fn press(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.dispatch(Event::Key(KeyEvent::new(code, modifiers)));
    }

    /// Dispatches a key event with an explicit kind.
    pub fn press_event(&mut self, event: KeyEvent) {
        self.dispatch(Event::Key(event));
    }

    /// Clicks the primary button at a cell.
    pub fn click(&mut self, line: u16, column: u16) {
        self.pointer(MouseEventKind::Down(MouseButton::Left), line, column);
        self.pointer(MouseEventKind::Up(MouseButton::Left), line, column);
    }

    /// Drags the primary button between two cells.
    pub fn drag(&mut self, from: (u16, u16), to: (u16, u16)) {
        self.pointer(MouseEventKind::Down(MouseButton::Left), from.0, from.1);
        self.pointer(MouseEventKind::Drag(MouseButton::Left), to.0, to.1);
        self.pointer(MouseEventKind::Up(MouseButton::Left), to.0, to.1);
    }

    /// Sends a wheel notch; positive `dy` scrolls down.
    pub fn wheel(&mut self, line: u16, column: u16, dx: i16, dy: i16) {
        let kind = if dy < 0 {
            MouseEventKind::ScrollUp
        } else if dy > 0 {
            MouseEventKind::ScrollDown
        } else if dx < 0 {
            MouseEventKind::ScrollLeft
        } else {
            MouseEventKind::ScrollRight
        };
        self.pointer(kind, line, column);
    }

    /// Resizes the harness viewport.
    pub fn resize(&mut self, viewport: Size) {
        self.viewport = viewport;
        self.dispatch(Event::Resize(viewport.width, viewport.height));
        self.handle.request_frame();
        self.pump();
    }

    /// Advances the frame clock by `elapsed` and drains frames.
    pub fn tick(&mut self, elapsed: Duration) {
        self.handle.tick_frame_clocks(Instant::now() + elapsed);
        self.pump();
    }

    /// Pumps until `predicate` accepts the screen or the timeout expires.
    pub fn wait_for(&mut self, timeout: Duration, predicate: impl Fn(&Screen) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if predicate(&self.screen()) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            match self.output.recv_timeout(Duration::from_millis(10)) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return predicate(&self.screen()),
            }
        }
    }

    /// A pointer event helper used by the public gesture methods.
    fn pointer(&mut self, kind: MouseEventKind, line: u16, column: u16) {
        self.dispatch(Event::Mouse(MouseEvent {
            kind,
            column,
            row: line,
            modifiers: KeyModifiers::empty(),
        }));
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        /* Dropping the runtime closes the input and joins the workers. */
    }
}
