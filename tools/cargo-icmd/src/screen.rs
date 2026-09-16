//! Test-only helpers that turn a committed frame into inspectable text.
//!
//! The framework publishes a frame as cell surfaces with positions, and every
//! cell exposes its symbol through the public API, so a frame can be replayed
//! into the grid the terminal would show. That makes it possible to assert what
//! a chapter actually paints — not merely that it painted something.
//!
//! The module also provides two probes: one mounts a chapter scrolled to a
//! section, and one drives wheel events over a real chapter while reporting the
//! document offset they produce.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::style::Color;
use icmd::advanced::{Commit, Lower, Runtime, ShutdownPolicy};
use icmd::{
    Component, ComponentContext, ElementRef, Frame, Node, Operation, Props, ScrollOffset, Size,
};

use crate::chapters::{ChapterProps, NavBindings, Navigator, selected_chapter};
use crate::metadata;

/// A replayed screen: one character per terminal cell.
pub(crate) struct Screen {
    rows: Vec<String>,
    backgrounds: Vec<Vec<Color>>,
}

/// One cell surface tracked across frames so incremental patches can be applied.
struct Surface {
    position: (i32, i32),
    image: icmd::Image,
    level: i32,
    order: u64,
}

impl Screen {
    /// Replays every frame in order and composites the resulting surfaces.
    ///
    /// A commit stage emits a full frame first and then patches, so a faithful
    /// screen has to apply `Create`, `PatchRect`, and `PatchCells` in sequence
    /// rather than read one frame in isolation.
    pub(crate) fn from_frames(frames: &[Frame], width: u16, height: u16) -> Self {
        let width = usize::from(width);
        let height = usize::from(height);
        let mut surfaces: Vec<(u64, Option<Surface>)> = Vec::new();
        let mut next_order = 0_u64;
        for frame in frames {
            for operation in &frame.operations {
                match operation {
                    Operation::Create {
                        id,
                        image,
                        position,
                        level,
                    } => {
                        next_order += 1;
                        set_surface(
                            &mut surfaces,
                            id.0,
                            Some(Surface {
                                position: (position.line, position.column),
                                image: image.clone(),
                                level: *level,
                                order: next_order,
                            }),
                        );
                    }
                    Operation::Remove { id } => set_surface(&mut surfaces, id.0, None),
                    Operation::Move { id, position } => {
                        if let Some(surface) = live_surface(&mut surfaces, id.0) {
                            surface.position = (position.line, position.column);
                        }
                    }
                    Operation::SetLevel { id, level } => {
                        if let Some(surface) = live_surface(&mut surfaces, id.0) {
                            surface.level = *level;
                        }
                    }
                    Operation::SetOrder { id, order } => {
                        if let Some(surface) = live_surface(&mut surfaces, id.0) {
                            surface.order = *order;
                        }
                    }
                    Operation::Replace { id, image } => {
                        if let Some(surface) = live_surface(&mut surfaces, id.0) {
                            surface.image = image.clone();
                        }
                    }
                    Operation::PatchRect { id, rect, rows } => {
                        if let Some(surface) = live_surface(&mut surfaces, id.0) {
                            let _ = surface.image.patch_rect(*rect, rows);
                        }
                    }
                    Operation::PatchCells { id, edits } => {
                        if let Some(surface) = live_surface(&mut surfaces, id.0) {
                            // `patch_cells` is internal, but a single-cell edit
                            // is exactly a one-by-one rectangle patch.
                            for edit in edits {
                                let rect =
                                    icmd::Rect::new(edit.position.line, edit.position.column, 1, 1);
                                let rows = vec![vec![edit.cell.clone()]];
                                let _ = surface.image.patch_rect(rect, &rows);
                            }
                        }
                    }
                    Operation::CreateRaster { .. }
                    | Operation::ReplaceRaster { .. }
                    | Operation::SetRasterClip { .. } => {}
                }
            }
        }

        let mut ordered: Vec<&Surface> = surfaces
            .iter()
            .filter_map(|(_, surface)| surface.as_ref())
            .collect();
        ordered.sort_by_key(|surface| (surface.level, surface.order));

        let mut rows = vec![vec![' '; width]; height];
        let mut backgrounds = vec![vec![Color::Reset; width]; height];
        for surface in ordered {
            let image = &surface.image;
            for line in 0..image.height() {
                let Some(target_line) = usize::try_from(surface.position.0)
                    .ok()
                    .and_then(|base| base.checked_add(line))
                    .filter(|line| *line < height)
                else {
                    continue;
                };
                let mut column = 0_usize;
                while column < image.width() {
                    let cell = image.cell_at(line, column).cell();
                    let symbol = cell.symbol();
                    let target = usize::try_from(surface.position.1).unwrap_or(0) + column;
                    if target < width && !symbol.is_empty() {
                        for (offset, ch) in symbol.chars().enumerate() {
                            if target + offset < width {
                                rows[target_line][target + offset] = ch;
                                backgrounds[target_line][target + offset] = cell.background();
                            }
                        }
                    }
                    column += cell.width().max(1);
                }
            }
        }
        Self {
            rows: rows
                .into_iter()
                .map(|row| row.into_iter().collect::<String>())
                .collect(),
            backgrounds,
        }
    }

    /// How many cells paint a different background than the same cell in
    /// `other`.
    ///
    /// A selection is painted as a background change, so comparing two frames
    /// that differ only by a pointer gesture tells whether anything was
    /// selected without knowing the theme's selection colour.
    pub(crate) fn background_diff_rows(
        &self,
        other: &Screen,
        rows: std::ops::Range<usize>,
    ) -> usize {
        let mut changed = 0;
        for line in rows {
            let (Some(row), Some(other_row)) =
                (self.backgrounds.get(line), other.backgrounds.get(line))
            else {
                continue;
            };
            for (cell, other_cell) in row.iter().zip(other_row) {
                if cell != other_cell {
                    changed += 1;
                }
            }
        }
        changed
    }

    /// Every row, in order.
    pub(crate) fn rows(&self) -> &[String] {
        &self.rows
    }

    /// The whole screen as one string with newlines between rows.
    pub(crate) fn text(&self) -> String {
        self.rows.join("\n")
    }

    /// Whether any row contains `needle`.
    pub(crate) fn contains(&self, needle: &str) -> bool {
        self.rows.iter().any(|row| row.contains(needle))
    }

    /// The first `(line, column)` where `needle` appears.
    ///
    /// The column is counted in characters, not bytes, so it can index a row
    /// that contains multi-byte box drawing without landing inside a glyph.
    pub(crate) fn find(&self, needle: &str) -> Option<(usize, usize)> {
        self.rows.iter().enumerate().find_map(|(line, row)| {
            row.find(needle)
                .map(|byte| (line, row[..byte].chars().count()))
        })
    }
}

/// Stores or removes one surface by numeric id.
fn set_surface(surfaces: &mut Vec<(u64, Option<Surface>)>, id: u64, surface: Option<Surface>) {
    for entry in surfaces.iter_mut() {
        if entry.0 == id {
            entry.1 = surface;
            return;
        }
    }
    surfaces.push((id, surface));
}

/// Borrows a live surface by numeric id.
fn live_surface(surfaces: &mut [(u64, Option<Surface>)], id: u64) -> Option<&mut Surface> {
    surfaces
        .iter_mut()
        .find(|entry| entry.0 == id)
        .and_then(|entry| entry.1.as_mut())
}

/// Collects up to `frames` frames, stopping when the pipeline goes quiet.
///
/// The receive is a closure so this module never has to name the runtime's
/// channel type.
fn drain(mut recv: impl FnMut() -> Option<Frame>, frames: usize) -> Vec<Frame> {
    let mut collected = Vec::new();
    for _ in 0..frames {
        match recv() {
            Some(frame) => collected.push(frame),
            None => break,
        }
    }
    collected
}

/// Commits an arbitrary node and returns the screen it paints.
pub(crate) fn render_node(node: Node, viewport: Size) -> Screen {
    let (commit, _, _) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();
    input.send(node).expect("the node enters the pipeline");
    let frames = drain(|| output.recv_timeout(Duration::from_secs(5)).ok(), 6);
    drop(input);
    runtime.shutdown(ShutdownPolicy::default()).unwrap();
    assert!(!frames.is_empty(), "the node paints at least one frame");
    Screen::from_frames(&frames, viewport.width, viewport.height)
}

/// What a chapter probe should render and where it should scroll.
struct ChapterProbe {
    chapter: usize,
    section: usize,
}

fn chapter_probe(cx: &mut ComponentContext, props: &Props<ChapterProbe>) -> Node {
    let chapter = props.data().chapter;
    let section = props.data().section;
    let (_current, set_chapter) = cx.use_state(move || chapter);
    let (_active, set_active) = cx.use_state(|| 0_usize);
    let (scroll, set_scroll) = cx.use_state(ScrollOffset::default);
    let active_ref = cx.use_ref(|| 0_usize);
    let pending = cx.use_ref(|| None::<usize>);
    let viewport_ref = cx.use_element_ref();
    let section_refs = cx.use_memo(chapter, || {
        (0..metadata::section_count(chapter))
            .map(|_| ElementRef::new())
            .collect::<Vec<_>>()
    });
    let navigator = Navigator::new(
        chapter,
        section_refs,
        viewport_ref,
        NavBindings::new(set_chapter, set_active, set_scroll, active_ref, pending),
    );
    let scoped = navigator.clone();
    cx.use_mount_effect(move || scoped.goto_section(section));
    let chapter_props = ChapterProps::new(navigator, scroll, true);
    selected_chapter(chapter, chapter_props)
}

/// Commits `chapter` scrolled to `section` and returns the screen it paints.
///
/// The probe asks the document to scroll to the section after mount, so the
/// screen shows that lesson rather than the chapter masthead.
pub(crate) fn render_section(chapter: usize, section: usize, viewport: Size) -> Screen {
    let (commit, _, _) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();
    input
        .send(chapter_probe.apply(ChapterProbe { chapter, section }))
        .expect("the chapter probe enters the pipeline");
    let frames = drain(|| output.recv_timeout(Duration::from_secs(5)).ok(), 6);
    drop(input);
    runtime.shutdown(ShutdownPolicy::default()).unwrap();
    assert!(!frames.is_empty(), "the chapter paints at least one frame");
    Screen::from_frames(&frames, viewport.width, viewport.height)
}

/// Mirrors a real chapter's document offset while wheel events are dispatched.
struct ChapterWheelProbe {
    chapter: usize,
    scroll: Arc<Mutex<ScrollOffset>>,
    max: Arc<Mutex<ScrollOffset>>,
}

fn chapter_wheel_probe(cx: &mut ComponentContext, props: &Props<ChapterWheelProbe>) -> Node {
    let chapter = props.data().chapter;
    let (_current, set_chapter) = cx.use_state(move || chapter);
    let (_active, set_active) = cx.use_state(|| 0_usize);
    let (scroll, set_scroll) = cx.use_state(ScrollOffset::default);
    let active_ref = cx.use_ref(|| 0_usize);
    let pending = cx.use_ref(|| None::<usize>);
    let viewport_ref = cx.use_element_ref();
    let section_refs = cx.use_memo(chapter, || {
        (0..metadata::section_count(chapter))
            .map(|_| ElementRef::new())
            .collect::<Vec<_>>()
    });
    let navigator = Navigator::new(
        chapter,
        section_refs,
        viewport_ref.clone(),
        NavBindings::new(set_chapter, set_active, set_scroll, active_ref, pending),
    );
    *props.data().scroll.lock().expect("scroll cell") = scroll;
    if let Some(snapshot) = viewport_ref
        .current()
        .and_then(|snapshot| snapshot.scroll())
    {
        *props.data().max.lock().expect("max cell") = snapshot.max_offset;
    }
    let chapter_props = ChapterProps::new(navigator, scroll, true);
    selected_chapter(chapter, chapter_props)
}

/// Dispatches `steps` wheel-down events over a real chapter and reports the
/// document offset and maximum after each one.
pub(crate) fn drive_chapter_wheel(
    chapter: usize,
    viewport: Size,
    at: (u16, u16),
    steps: usize,
) -> Vec<(ScrollOffset, ScrollOffset)> {
    use crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};

    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let probe = ChapterWheelProbe {
        chapter,
        scroll: Arc::new(Mutex::new(ScrollOffset::default())),
        max: Arc::new(Mutex::new(ScrollOffset::default())),
    };
    let shared = ChapterWheelProbe {
        chapter: probe.chapter,
        scroll: probe.scroll.clone(),
        max: probe.max.clone(),
    };
    input
        .send(chapter_wheel_probe.apply(shared))
        .expect("the chapter enters the pipeline");
    // Settle the mount sequence before observing.
    let _ = drain(|| output.recv_timeout(Duration::from_secs(2)).ok(), 3);
    let mut observed = Vec::new();
    for _ in 0..steps {
        dispatcher.dispatch(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: at.0,
            row: at.1,
            modifiers: KeyModifiers::NONE,
        }));
        // One committed frame carries the render that mirrored the new offset;
        // the short second drain only collects a follow-up frame if one exists.
        let _ = drain(|| output.recv_timeout(Duration::from_secs(2)).ok(), 1);
        let _ = drain(|| output.recv_timeout(Duration::from_millis(50)).ok(), 2);
        observed.push((
            *probe.scroll.lock().expect("scroll cell"),
            *probe.max.lock().expect("max cell"),
        ));
    }
    drop(input);
    runtime.shutdown(ShutdownPolicy::default()).unwrap();
    observed
}

/// One synthetic terminal action used by an interaction test.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Action {
    /// A primary-button press and release at a cell.
    Click {
        /// Screen column.
        column: u16,
        /// Screen row.
        row: u16,
    },
    /// A pointer drag from one cell to another with the primary button held.
    Drag {
        /// Screen column to press at.
        from_column: u16,
        /// Screen row to press at.
        from_row: u16,
        /// Screen column to release at.
        to_column: u16,
        /// Screen row to release at.
        to_row: u16,
    },
    /// One wheel notch downward at a cell.
    WheelDown {
        /// Screen column.
        column: u16,
        /// Screen row.
        row: u16,
    },
    /// Many wheel notches with no frame collection in between, which is what a
    /// fast wheel or trackpad actually delivers.
    WheelBurst {
        /// Screen column.
        column: u16,
        /// Screen row.
        row: u16,
        /// How many notches to deliver.
        count: usize,
    },
    /// The viewport changed size.
    Resize {
        /// New width in cells.
        width: u16,
        /// New height in cells.
        height: u16,
    },
    /// A printable character typed at whatever holds focus.
    Type(char),
    /// A key press with modifiers.
    Key(crossterm::event::KeyCode, crossterm::event::KeyModifiers),
}

/// What the pipeline did after one action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The action produced this many frames.
    Advanced(usize),
    /// No frame arrived; the session is alive but nothing changed.
    Quiet,
    /// The frame channel closed, so the session ended.
    Ended,
}

/// Mounts a lesson, applies `actions`, and reports the screen and the pipeline's
/// response to each action.
///
/// A `Step::Ended` is how an unexpected exit shows up: the runtime stops
/// producing frames, which is what a panic in a listener or a failed stage looks
/// like from outside. `outcome` carries the shutdown error when the session
/// ended badly.
pub(crate) fn interact_section(
    chapter: usize,
    section: usize,
    viewport: Size,
    actions: &[Action],
) -> (Screen, Vec<Step>, Option<String>) {
    use crossterm::event::{
        Event, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    let (commit, setter, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let probe = SectionProbe {
        chapter,
        section,
        scroll: Arc::new(Mutex::new(ScrollOffset::default())),
    };
    let shared = SectionProbe {
        chapter,
        section,
        scroll: probe.scroll.clone(),
    };
    input
        .send(section_probe.apply(shared))
        .expect("the lesson enters the pipeline");

    let mut frames = Vec::new();
    let collect = |frames: &mut Vec<Frame>| -> Step {
        let mut count = 0;
        loop {
            match output.recv_timeout(Duration::from_millis(400)) {
                Ok(frame) => {
                    frames.push(frame);
                    count += 1;
                    if count >= 3 {
                        break;
                    }
                }
                Err(error) if error.is_disconnected() => return Step::Ended,
                Err(_) => break,
            }
        }
        if count > 0 {
            Step::Advanced(count)
        } else {
            Step::Quiet
        }
    };

    let settled = collect(&mut frames);
    let mut steps = Vec::new();
    let mut ended = settled == Step::Ended;
    for action in actions {
        if ended {
            steps.push(Step::Ended);
            continue;
        }
        match *action {
            Action::Click { column, row } => {
                for kind in [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ] {
                    dispatcher.dispatch(Event::Mouse(MouseEvent {
                        kind,
                        column,
                        row,
                        modifiers: KeyModifiers::NONE,
                    }));
                }
            }
            Action::Drag {
                from_column,
                from_row,
                to_column,
                to_row,
            } => {
                dispatcher.dispatch(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: from_column,
                    row: from_row,
                    modifiers: KeyModifiers::NONE,
                }));
                dispatcher.dispatch(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column: to_column,
                    row: to_row,
                    modifiers: KeyModifiers::NONE,
                }));
                dispatcher.dispatch(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: to_column,
                    row: to_row,
                    modifiers: KeyModifiers::NONE,
                }));
            }
            Action::WheelDown { column, row } => {
                dispatcher.dispatch(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column,
                    row,
                    modifiers: KeyModifiers::NONE,
                }));
            }
            Action::WheelBurst { column, row, count } => {
                for _ in 0..count {
                    dispatcher.dispatch(Event::Mouse(MouseEvent {
                        kind: MouseEventKind::ScrollDown,
                        column,
                        row,
                        modifiers: KeyModifiers::NONE,
                    }));
                }
            }
            Action::Resize { width, height } => {
                setter.set(Size::new(width, height));
                dispatcher.dispatch(Event::Resize(width, height));
            }
            Action::Type(ch) => {
                dispatcher.dispatch(Event::Key(KeyEvent::new(
                    crossterm::event::KeyCode::Char(ch),
                    KeyModifiers::NONE,
                )));
            }
            Action::Key(code, modifiers) => {
                dispatcher.dispatch(Event::Key(KeyEvent::new(code, modifiers)));
            }
        }
        let step = collect(&mut frames);
        ended = step == Step::Ended;
        steps.push(step);
    }

    let screen = Screen::from_frames(&frames, viewport.width, viewport.height);
    let _ = setter.viewport();
    drop(input);
    let outcome = runtime
        .shutdown(ShutdownPolicy::default())
        .err()
        .map(|error| format!("{error}"));
    (screen, steps, outcome)
}

/// Mirrors a lesson's document offset while actions are applied to it.
struct SectionProbe {
    chapter: usize,
    section: usize,
    scroll: Arc<Mutex<ScrollOffset>>,
}

fn section_probe(cx: &mut ComponentContext, props: &Props<SectionProbe>) -> Node {
    let chapter = props.data().chapter;
    let section = props.data().section;
    let (_current, set_chapter) = cx.use_state(move || chapter);
    let (_active, set_active) = cx.use_state(|| 0_usize);
    let (scroll, set_scroll) = cx.use_state(ScrollOffset::default);
    let active_ref = cx.use_ref(|| 0_usize);
    let pending = cx.use_ref(|| None::<usize>);
    let viewport_ref = cx.use_element_ref();
    let section_refs = cx.use_memo(chapter, || {
        (0..metadata::section_count(chapter))
            .map(|_| ElementRef::new())
            .collect::<Vec<_>>()
    });
    let navigator = Navigator::new(
        chapter,
        section_refs,
        viewport_ref,
        NavBindings::new(set_chapter, set_active, set_scroll, active_ref, pending),
    );
    *props.data().scroll.lock().expect("scroll cell") = scroll;
    let scoped = navigator.clone();
    cx.use_mount_effect(move || scoped.goto_section(section));
    let chapter_props = ChapterProps::new(navigator, scroll, true);
    selected_chapter(chapter, chapter_props)
}

/// Mounts the full shell and applies `actions`, reporting the screen and the
/// pipeline's response to each action.
///
/// This is the faithful counterpart to [`interact_section`]: it exercises the
/// real navigation state, the real document scroller, and the real sidebar, so a
/// defect that only appears once the guide is driven as an application is
/// reproducible here.
pub(crate) fn interact_shell(
    props: crate::shell::ShellProps,
    viewport: Size,
    actions: &[Action],
) -> (Screen, Vec<Step>, Option<String>) {
    use crossterm::event::{
        Event, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    let (commit, setter, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();
    input
        .send(crate::shell::mount(props))
        .expect("the shell enters the pipeline");

    let mut frames = Vec::new();
    let collect = |frames: &mut Vec<Frame>| -> Step {
        let mut count = 0;
        loop {
            match output.recv_timeout(Duration::from_millis(400)) {
                Ok(frame) => {
                    frames.push(frame);
                    count += 1;
                    if count >= 4 {
                        break;
                    }
                }
                Err(error) if error.is_disconnected() => return Step::Ended,
                Err(_) => break,
            }
        }
        if count > 0 {
            Step::Advanced(count)
        } else {
            Step::Quiet
        }
    };

    let settled = collect(&mut frames);
    let mut steps = Vec::new();
    let mut ended = settled == Step::Ended;
    for action in actions {
        if ended {
            steps.push(Step::Ended);
            continue;
        }
        match *action {
            Action::Click { column, row } => {
                for kind in [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ] {
                    dispatcher.dispatch(Event::Mouse(MouseEvent {
                        kind,
                        column,
                        row,
                        modifiers: KeyModifiers::NONE,
                    }));
                }
            }
            Action::Drag {
                from_column,
                from_row,
                to_column,
                to_row,
            } => {
                dispatcher.dispatch(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: from_column,
                    row: from_row,
                    modifiers: KeyModifiers::NONE,
                }));
                dispatcher.dispatch(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column: to_column,
                    row: to_row,
                    modifiers: KeyModifiers::NONE,
                }));
                dispatcher.dispatch(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: to_column,
                    row: to_row,
                    modifiers: KeyModifiers::NONE,
                }));
            }
            Action::WheelDown { column, row } => {
                dispatcher.dispatch(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column,
                    row,
                    modifiers: KeyModifiers::NONE,
                }));
            }
            Action::WheelBurst { column, row, count } => {
                for _ in 0..count {
                    dispatcher.dispatch(Event::Mouse(MouseEvent {
                        kind: MouseEventKind::ScrollDown,
                        column,
                        row,
                        modifiers: KeyModifiers::NONE,
                    }));
                }
            }
            Action::Resize { width, height } => {
                setter.set(Size::new(width, height));
                dispatcher.dispatch(Event::Resize(width, height));
            }
            Action::Type(ch) => {
                dispatcher.dispatch(Event::Key(KeyEvent::new(
                    crossterm::event::KeyCode::Char(ch),
                    KeyModifiers::NONE,
                )));
            }
            Action::Key(code, modifiers) => {
                dispatcher.dispatch(Event::Key(KeyEvent::new(code, modifiers)));
            }
        }
        let step = collect(&mut frames);
        ended = step == Step::Ended;
        steps.push(step);
    }

    let screen = Screen::from_frames(&frames, viewport.width, viewport.height);
    drop(input);
    let outcome = runtime
        .shutdown(ShutdownPolicy::default())
        .err()
        .map(|error| format!("{error}"));
    (screen, steps, outcome)
}

/// A section's committed frames encoded by the symbols renderer, with control
/// sequences removed.
///
/// Raster placeholders are painted as surfaces rather than cells, so a
/// cell-level replay cannot see them; this runs the real renderer in symbol mode
/// and returns the text it would put on the terminal.
pub(crate) fn rendered_section_text(chapter: usize, section: usize, viewport: Size) -> String {
    use icmd::ImageProtocol;
    use icmd::advanced::{ChannelRenderer, RendererConfig};

    let (commit, _setter, _dispatcher) = Commit::new_with_events(viewport);
    let renderer = ChannelRenderer::with_config(
        viewport,
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            cell_pixel_size: Some(Size::new(8, 16)),
            ..Default::default()
        },
    )
    .expect("renderer");
    let runtime = Runtime::new(Lower::default())
        .then(commit)
        .then(renderer)
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let probe = SectionProbe {
        chapter,
        section,
        scroll: Arc::new(Mutex::new(ScrollOffset::default())),
    };
    let shared = SectionProbe {
        chapter,
        section,
        scroll: probe.scroll.clone(),
    };
    input
        .send(section_probe.apply(shared))
        .expect("the lesson enters the pipeline");

    // Keep draining until the deadline: a source that fails to load reports back
    // on a later tick, after the frames that painted the loading marker.
    let mut painted = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match output.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(payload)) => painted.push_str(&strip_control(&payload)),
            Ok(Err(_)) => {}
            Err(error) if error.is_disconnected() => break,
            Err(_) => {}
        }
    }
    drop(input);
    let _ = runtime.shutdown(ShutdownPolicy::default());
    painted
}

/// Removes ANSI control sequences, leaving the printable text of a frame.
fn strip_control(payload: &str) -> String {
    let mut out = String::new();
    let mut chars = payload.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        if let Some('[') = chars.next() {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;

    fn query_props(query: &str) -> crate::shell::ShellProps {
        crate::shell::ShellProps {
            initial_search: true,
            initial_query: query.to_string(),
            ..Default::default()
        }
    }

    fn enter() -> Action {
        Action::Key(KeyCode::Enter, KeyModifiers::NONE)
    }

    fn ended(steps: &[Step]) -> bool {
        steps.iter().any(|step| matches!(step, Step::Ended))
    }

    /// Clicking a text control must place the caret where the pointer is.
    ///
    /// The editor publishes the offset applied to its own text, and its pointer
    /// mapping adds that offset to a position already relative to the box the
    /// scroller sits in. Publishing the sum of every ancestor scroller meant the
    /// page offset was added twice, so a click resolved past the end of the
    /// value and the caret jumped to the end however far into the text it landed.
    #[test]
    fn clicking_an_input_places_the_caret_at_the_pointer() {
        let viewport = Size::new(120, 40);
        let (screen, _, _) = interact_shell(
            query_props("Controlled and uncontrolled input"),
            viewport,
            &[enter()],
        );
        let (line, value_column) = screen
            .find("field-guide")
            .expect("the controlled input paints its value");

        for (offset, expected) in [
            (2_usize, "fiXeld-guide"),
            (4, "fielXd-guide"),
            (6, "field-Xguide"),
        ] {
            let (after, steps, outcome) = interact_shell(
                query_props("Controlled and uncontrolled input"),
                viewport,
                &[
                    enter(),
                    Action::Click {
                        column: (value_column + offset) as u16,
                        row: line as u16,
                    },
                    Action::Type('X'),
                ],
            );
            assert!(!ended(&steps), "the session ended: {outcome:?}");
            assert!(
                after.contains(expected),
                "clicking {offset} cells into the value must insert there, expected `{expected}`:\n{}",
                after.text()
            );
        }
    }

    /// Scrolling through the chapter that contains the guide's images must not
    /// end the session.
    ///
    /// A surface that has scrolled below the viewport still reports damage, and
    /// the renderer's damage normalization clamped only the end of the row span.
    /// A start past the last row made `damage_rows[top..bottom]` panic, which
    /// killed the renderer thread and ended the session while the reader was
    /// simply scrolling through chapter seven. This walks the whole chapter, so
    /// the raster surfaces are dragged through the viewport, which is what
    /// produces that damage.
    #[test]
    fn scrolling_chapter_seven_never_ends_the_session() {
        use crossterm::event::{Event, KeyEvent, MouseEvent, MouseEventKind};
        use icmd::ImageProtocol;
        use icmd::advanced::{ChannelRenderer, RendererConfig};

        let viewport = Size::new(120, 40);
        let (commit, _setter, dispatcher) = Commit::new_with_events(viewport);
        let renderer = ChannelRenderer::with_config(
            viewport,
            RendererConfig {
                image_protocol: ImageProtocol::Kitty,
                cell_pixel_size: Some(Size::new(8, 16)),
                ..Default::default()
            },
        )
        .expect("renderer");
        let runtime = Runtime::new(Lower::default())
            .then(commit)
            .then(renderer)
            .start_handle();
        let input = runtime.input();
        let output = runtime.output();
        input
            .send(crate::shell::mount(query_props("Scrollable regions")))
            .expect("the shell enters the pipeline");

        let mut payloads = 0_usize;
        let mut ended = false;
        let drain = |payloads: &mut usize, ended: &mut bool| {
            loop {
                match output.recv_timeout(Duration::from_millis(120)) {
                    Ok(Ok(_)) => *payloads += 1,
                    Ok(Err(_)) => {}
                    Err(error) if error.is_disconnected() => {
                        *ended = true;
                        break;
                    }
                    Err(_) => break,
                }
            }
        };
        drain(&mut payloads, &mut ended);
        dispatcher.dispatch(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        drain(&mut payloads, &mut ended);

        for step in 0..160 {
            for _ in 0..2 {
                dispatcher.dispatch(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 60,
                    row: 20,
                    modifiers: KeyModifiers::NONE,
                }));
            }
            drain(&mut payloads, &mut ended);
            assert!(
                !ended,
                "the session ended at scroll step {step} while reading chapter seven"
            );
        }
        drop(input);
        let _ = runtime.shutdown(ShutdownPolicy::default());
        assert!(payloads > 0, "the renderer must encode frames");
    }

    /// Dragging inside a scrolled selectable region reports the text it covers.
    #[test]
    fn selecting_inside_a_scrolled_region_reports_a_range() {
        let viewport = Size::new(120, 40);
        let (before, _, _) = interact_shell(
            query_props("Selection inside scrolling"),
            viewport,
            &[enter()],
        );
        assert!(
            before.contains("nothing selected yet"),
            "the readout starts empty:\n{}",
            before.text()
        );
        let (line, column) = before
            .find("Selection follows painted content")
            .expect("the release notes paint their first line");
        let (after, steps, outcome) = interact_shell(
            query_props("Selection inside scrolling"),
            viewport,
            &[
                enter(),
                Action::Drag {
                    from_column: column as u16,
                    from_row: line as u16,
                    to_column: (column + 30) as u16,
                    to_row: line as u16,
                },
            ],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert!(
            !after.contains("nothing selected yet") && after.contains("characters"),
            "a drag must report the selected range:\n{}",
            after.text()
        );
    }

    /// Resizing and scrolling keep the shell painting its chrome and content.
    #[test]
    fn resizing_and_scrolling_keep_the_shell_alive() {
        let viewport = Size::new(140, 45);
        let (after, steps, outcome) = interact_shell(
            query_props("Themes"),
            viewport,
            &[
                enter(),
                Action::WheelBurst {
                    column: 70,
                    row: 20,
                    count: 6,
                },
                Action::Resize {
                    width: 80,
                    height: 24,
                },
                Action::WheelBurst {
                    column: 40,
                    row: 12,
                    count: 4,
                },
                Action::Resize {
                    width: 60,
                    height: 20,
                },
                Action::WheelBurst {
                    column: 30,
                    row: 10,
                    count: 4,
                },
                Action::Resize {
                    width: 120,
                    height: 40,
                },
            ],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert!(
            after.contains("ICMD FIELD GUIDE"),
            "the header must survive every resize:\n{}",
            after.text()
        );
    }

    /// A lesson renders and paints its controls without the surrounding shell.
    #[test]
    fn a_lesson_paints_its_controls_without_the_shell() {
        let (screen, steps, outcome) = interact_section(4, 2, Size::new(110, 40), &[]);
        assert!(!ended(&steps), "the lesson ended: {outcome:?}");
        for expected in ["field-guide", "draft-001"] {
            assert!(
                screen.contains(expected),
                "the lesson must paint `{expected}`:\n{}",
                screen.text()
            );
        }
    }

    /// Wheel scrolling a lesson keeps it alive and keeps its own scroller from
    /// resetting the page.
    #[test]
    fn wheel_scrolling_a_lesson_keeps_it_alive() {
        let actions: Vec<Action> = (0..10)
            .map(|_| Action::WheelDown {
                column: 50,
                row: 22,
            })
            .collect();
        let (screen, steps, outcome) = interact_section(6, 3, Size::new(120, 40), &actions);
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert!(
            screen.contains("release") || screen.contains("Selection"),
            "the lesson must keep painting:\n{}",
            screen.text()
        );
    }

    /// Dragging inside an input selects the glyphs under the pointer, so typing
    /// replaces exactly that run instead of inserting at the end.
    #[test]
    fn dragging_inside_an_input_selects_its_text() {
        let viewport = Size::new(120, 40);
        let (screen, _, _) = interact_shell(
            query_props("Controlled and uncontrolled input"),
            viewport,
            &[enter()],
        );
        let (line, column) = screen
            .find("field-guide")
            .expect("the controlled input paints its value");
        let (after, steps, outcome) = interact_shell(
            query_props("Controlled and uncontrolled input"),
            viewport,
            &[
                enter(),
                Action::Drag {
                    from_column: column as u16,
                    from_row: line as u16,
                    to_column: (column + 4) as u16,
                    to_row: line as u16,
                },
                Action::Type('Z'),
            ],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert!(
            after.contains("Z-guide"),
            "a drag across the first glyphs then typing must replace them:\n{}",
            after.text()
        );
    }

    /// Prose is selectable, and a drag across it paints a selection.
    #[test]
    fn dragging_across_prose_paints_a_selection() {
        let viewport = Size::new(120, 40);
        let query = "Checkboxes, radios, and switches";
        let (before, _, _) = interact_shell(query_props(query), viewport, &[enter()]);
        let (line, column) = before
            .find("Checkboxes, radios, and switches are controlled")
            .expect("the section prose paints");
        let header = before
            .find("LIVE EXAMPLE")
            .expect("the example frame paints")
            .0;
        let (after, steps, outcome) = interact_shell(
            query_props(query),
            viewport,
            &[
                enter(),
                Action::Drag {
                    from_column: column as u16,
                    from_row: line as u16,
                    to_column: (column + 30) as u16,
                    to_row: line as u16,
                },
            ],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert!(
            before.background_diff_rows(&after, 3..header) > 0,
            "a drag across prose must paint a selection"
        );
    }

    /// A demonstration is not prose: a drag inside it selects nothing above it,
    /// and it still answers a click.
    #[test]
    fn dragging_across_a_demonstration_selects_nothing() {
        let viewport = Size::new(120, 40);
        let query = "Checkboxes, radios, and switches";
        let (before, _, _) = interact_shell(query_props(query), viewport, &[enter()]);
        let (line, column) = before
            .find("Send anonymous telemetry")
            .expect("the demonstration paints its checkbox");
        let (after, steps, outcome) = interact_shell(
            query_props(query),
            viewport,
            &[
                enter(),
                Action::Drag {
                    from_column: column as u16,
                    from_row: line as u16,
                    to_column: (column + 30) as u16,
                    to_row: line as u16,
                },
            ],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert_eq!(
            before.background_diff_rows(&after, 3..before.rows().len()),
            0,
            "a drag inside a demonstration must not paint a selection anywhere:\n{}",
            after.text()
        );

        let (clicked, steps, outcome) = interact_shell(
            query_props(query),
            viewport,
            &[
                enter(),
                Action::Click {
                    column: column as u16,
                    row: line as u16,
                },
            ],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert!(
            before.contains("telemetry on") && clicked.contains("telemetry off"),
            "the demonstration must still toggle:\n{}",
            clicked.text()
        );
    }

    /// A demonstration that is about selection keeps its own selectable region:
    /// the inner region takes over its subtree from the barrier around the
    /// example, so the exception is the example's to opt into.
    #[test]
    fn a_selection_demonstration_remains_selectable() {
        let viewport = Size::new(120, 40);
        let query = "Selectable documents";
        let (before, _, _) = interact_shell(query_props(query), viewport, &[enter()]);
        assert!(
            before.contains("nothing selected"),
            "the readout starts empty:\n{}",
            before.text()
        );
        let (line, column) = before
            .find("Cell-aware shaping")
            .expect("the selectable demonstration paints its prose");
        let (after, steps, outcome) = interact_shell(
            query_props(query),
            viewport,
            &[
                enter(),
                Action::Drag {
                    from_column: column as u16,
                    from_row: line as u16,
                    to_column: (column + 30) as u16,
                    to_row: line as u16,
                },
            ],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert!(
            after.contains("bytes ") && !after.contains("nothing selected"),
            "a demonstration about selection must stay selectable:\n{}",
            after.text()
        );
    }

    /// The text of the expanded index, one line per row, with the columns the
    /// index occupies.
    fn sidebar_text(screen: &Screen) -> String {
        screen
            .rows()
            .iter()
            .map(|row| row.chars().take(INDEX_COLUMNS).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The sidebar lines from the one holding `from` up to the one holding
    /// `until`, so a check can be scoped to the chapter list or the contents
    /// list instead of matching wherever the text happens to appear.
    fn sidebar_between(sidebar: &str, from: &str, until: &str) -> String {
        let lines = sidebar.lines().collect::<Vec<_>>();
        let start = lines
            .iter()
            .position(|line| line.contains(from))
            .unwrap_or(0);
        let end = lines[start..]
            .iter()
            .position(|line| line.contains(until))
            .map(|offset| start + offset)
            .unwrap_or(lines.len());
        lines[start..end].join("\n")
    }

    /// The sidebar lines from the one holding `from` to the end.
    fn sidebar_from(sidebar: &str, from: &str) -> String {
        let lines = sidebar.lines().collect::<Vec<_>>();
        let start = lines
            .iter()
            .position(|line| line.contains(from))
            .unwrap_or(0);
        lines[start..].join("\n")
    }

    /// The first line holding `first` and the following line holding `second`,
    /// each with the character column where the text starts.
    fn wrapped_pair(
        sidebar: &str,
        first: &str,
        second: &str,
    ) -> Option<((usize, usize), (usize, usize))> {
        let lines = sidebar.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            let Some(start) = line.find(first) else {
                continue;
            };
            let Some(next) = lines.get(index + 1) else {
                continue;
            };
            let Some(next_start) = next.find(second) else {
                continue;
            };
            return Some((
                (index, line[..start].chars().count()),
                (index + 1, next[..next_start].chars().count()),
            ));
        }
        None
    }

    /// The number of columns the expanded index occupies.
    const INDEX_COLUMNS: usize = 30;

    /// Index titles wrap under themselves instead of being ellipsized, so a
    /// chapter or section name is always readable in full.
    #[test]
    fn the_index_wraps_its_titles_instead_of_clipping_them() {
        let viewport = Size::new(120, 44);
        let (screen, steps, outcome) = interact_shell(
            query_props("Selection inside scrolling"),
            viewport,
            &[enter()],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        let sidebar = sidebar_text(&screen);
        assert!(
            !sidebar.contains('…'),
            "the index must not ellipsize its titles:\n{sidebar}"
        );

        let chapters = sidebar_between(&sidebar, "CHAPTERS", "─");
        let (chapter_first, chapter_next) =
            wrapped_pair(&chapters, "Scroll, Selection, and", "Raster Media")
                .expect("the longest chapter title wraps onto a second line");
        assert_eq!(
            chapter_first.1, chapter_next.1,
            "the wrapped chapter title keeps a hanging indent:\n{sidebar}"
        );

        let contents = sidebar_from(&sidebar, "ON THIS PAGE");
        let (section_first, section_next) =
            wrapped_pair(&contents, "Controlled versus", "runtime-owned")
                .expect("a section title wraps onto a second line");
        assert_eq!(
            section_first.1, section_next.1,
            "the wrapped section title keeps a hanging indent:\n{sidebar}"
        );
        assert!(
            contents.contains("Selection inside") && contents.contains("scrolling"),
            "the active section title is complete:\n{sidebar}"
        );
    }

    /// The top bar never cuts the chapter title: at every width that matters it
    /// shows the whole name, dropping its own chrome rather than the title.
    #[test]
    fn the_bar_shows_the_chapter_title_in_full() {
        for width in [120_u16, 100, 80, 60] {
            let viewport = Size::new(width, 24);
            let (screen, steps, outcome) = interact_shell(
                query_props("Selection inside scrolling"),
                viewport,
                &[enter()],
            );
            assert!(!ended(&steps), "the session ended at {width}: {outcome:?}");
            let header = screen.rows().first().cloned().unwrap_or_default();
            assert!(
                header.contains("Scroll, Selection, and Raster Media"),
                "the title must fit whole at {width} columns:\n{}",
                screen.text()
            );
            assert!(
                !header.contains('…'),
                "the title must not be ellipsized at {width} columns:\n{header}"
            );
        }
    }

    /// The interior of the topmost dialog card, between its own borders, so a
    /// check can ignore the sidebar and the content behind the overlay.
    fn dialog_interior(screen: &Screen) -> String {
        let rows = screen
            .rows()
            .iter()
            .map(|row| row.chars().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let top = rows
            .iter()
            .position(|row| row.contains(&'╭'))
            .expect("a dialog card opens");
        let bottom = rows[top..]
            .iter()
            .position(|row| row.contains(&'╰'))
            .map(|offset| top + offset)
            .expect("the dialog card closes");
        let left = rows[top]
            .iter()
            .position(|cell| *cell == '╭')
            .expect("the card's left corner");
        let right = rows[top]
            .iter()
            .rposition(|cell| *cell == '╮')
            .expect("the card's right corner");
        rows[top + 1..bottom]
            .iter()
            .map(|row| row[left..=right].iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every box in the 3.6 bounds specimen shows its content.
    ///
    /// The two policy boxes were three cells tall with a border and a cell of
    /// padding on every side, which left no content row at all, so both painted
    /// as empty frames.
    #[test]
    fn the_bounds_specimen_paints_content_in_every_box() {
        let screen = render_section(2, 5, Size::new(96, 40));
        assert_eq!(
            screen.text().matches("this sentence is").count(),
            4,
            "all four specimen boxes must paint their text:\n{}",
            screen.text()
        );
        assert!(
            screen.contains("deliberately longer tha"),
            "Overflow::Visible must paint past the border:\n{}",
            screen.text()
        );
    }

    /// The search dialog draws no horizontal rule across itself.
    #[test]
    fn the_search_dialog_has_no_dividing_rule() {
        let (screen, steps, outcome) =
            interact_shell(query_props("scroll"), Size::new(120, 30), &[]);
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        let interior = dialog_interior(&screen);
        assert!(
            interior.contains("SEARCH") && interior.contains("scroll"),
            "the dialog still paints its field and results:\n{}",
            screen.text()
        );
        assert!(
            !interior.contains('─'),
            "the dialog must not draw a dividing rule:\n{interior}"
        );
    }

    /// The image specimen names what a box paints while its source is missing.
    #[test]
    fn the_image_specimen_paints_its_alternative_label() {
        // The specimen is tall, and the label sits on the placeholder's middle
        // row, so the viewport has to hold the whole box.
        let text = rendered_section_text(6, 4, Size::new(110, 70));
        assert!(
            text.contains("guide.png"),
            "a missing source must paint its alt label:\n{text}"
        );
        assert!(
            text.contains("its alt label"),
            "the caption must describe the label:\n{text}"
        );
    }

    /// Opening the dialog must focus its field, even when the page behind it owns
    /// focus. Otherwise the first keystroke goes to the chapter and the dialog
    /// looks dead.
    #[test]
    fn the_search_dialog_takes_focus_when_it_opens() {
        let viewport = Size::new(120, 30);
        let query = "A UI that belongs in the terminal";
        let (first, _, _) = interact_shell(query_props(query), viewport, &[enter()]);
        let (line, column) = first
            .find("terminal interface is not a web page")
            .expect("the chapter paints its prose");

        let (screen, steps, outcome) = interact_shell(
            query_props(query),
            viewport,
            &[
                enter(),
                Action::Click {
                    column: column as u16,
                    row: line as u16,
                },
                Action::Key(KeyCode::Char('k'), KeyModifiers::CONTROL),
                Action::Key(KeyCode::Char('z'), KeyModifiers::NONE),
            ],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert!(
            screen.contains("terminalz"),
            "the first keystroke must reach the dialog's field:\n{}",
            screen.text()
        );
    }

    /// The dialog is a card near the top of the page, not a sheet covering it: it
    /// keeps a fixed, modest height and leaves the document visible around it.
    #[test]
    fn the_search_dialog_is_a_bounded_card() {
        for viewport in [Size::new(120, 24), Size::new(120, 44), Size::new(80, 24)] {
            let (screen, steps, outcome) = interact_shell(query_props("scroll"), viewport, &[]);
            assert!(!ended(&steps), "the session ended: {outcome:?}");
            let rows = screen.rows();
            let top = rows
                .iter()
                .position(|row| row.contains('╭'))
                .expect("the card opens");
            let bottom = rows
                .iter()
                .rposition(|row| row.contains('╰'))
                .expect("the card closes");
            assert!(
                top >= 4,
                "the card must clear the header at {viewport:?}:\n{}",
                screen.text()
            );
            assert!(
                bottom - top <= 17,
                "the card must stay compact at {viewport:?}:\n{}",
                screen.text()
            );
            assert!(
                bottom + 2 <= rows.len(),
                "the card must leave the page visible below it at {viewport:?}:\n{}",
                screen.text()
            );
            assert!(
                !rows[bottom + 1].trim().is_empty(),
                "the document must keep painting below the card at {viewport:?}:\n{}",
                screen.text()
            );
        }
    }

    /// The dialog wins focus from a page that also asks for it, which is what an
    /// overlay has to do: chapter five's demonstration autofocuses a button, so
    /// the first keystroke used to land there instead of in the field.
    #[test]
    fn the_search_dialog_takes_focus_from_an_autofocused_page() {
        let viewport = Size::new(120, 44);
        let query = "Buttons and semantic";
        let (first, _, _) = interact_shell(query_props(query), viewport, &[enter()]);
        let (line, column) = first
            .find("Publish")
            .expect("the demonstration paints its autofocused button");

        let (screen, steps, outcome) = interact_shell(
            query_props(query),
            viewport,
            &[
                enter(),
                Action::Click {
                    column: column as u16,
                    row: line as u16,
                },
                Action::Key(KeyCode::Char('k'), KeyModifiers::CONTROL),
                Action::Key(KeyCode::Char('z'), KeyModifiers::NONE),
            ],
        );
        assert!(!ended(&steps), "the session ended: {outcome:?}");
        assert!(
            screen.contains("semanticz"),
            "the dialog must take focus from the page:\n{}",
            screen.text()
        );
    }

    /// The guide shows the version it ships with.
    ///
    /// The framework and this documentation tool are versioned together, so the
    /// tool's package version is the one the release card has to paint.
    #[test]
    fn the_guide_shows_the_crate_version() {
        let screen = render_section(0, 0, Size::new(110, 44));
        let expected = concat!("icmd ", env!("CARGO_PKG_VERSION"));
        assert!(
            screen.contains(expected),
            "the release card must show `{expected}`:\n{}",
            screen.text()
        );
    }
}
