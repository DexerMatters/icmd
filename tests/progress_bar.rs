//! Progress-bar cell styling and the opt-in pointer drag.
//!
//! Rendering is checked against committed frames; drag behavior is checked
//! through the real dispatcher so capture, clamping, and propagation are the
//! runtime's, not a test double's.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use icmd::advanced::{Commit, Lower, Renderer, Runtime};
use icmd::{
    Attr, Component, EventListener, Glyph, GlyphRamp, Node, PointerEvent, ProgressBarDragEvent,
    ProgressBarProps, ProgressBarStyle, Size, ThemeBuilder, progress_bar, theme::ThemeColors,
    theme::ThemeMode, theme_provider, ui,
};

fn render(node: Node, viewport: Size) -> String {
    let (commit, _) = Commit::new(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start();
    input.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .expect("runtime did not produce a frame")
        .expect("renderer failed")
}

fn bar(props: ProgressBarProps) -> Node {
    progress_bar.props(props).node()
}

#[test]
fn fractional_cell_uses_the_ramp_and_keeps_the_painted_width() {
    // 30 / 100 of eight cells is 2.4 cells: two full, one fractional, five empty.
    let node = bar(ProgressBarProps {
        value: Attr::Set(30),
        max: Attr::Set(100),
        width: Attr::Set(8),
        show_percentage: Attr::Set(false),
        ..ProgressBarProps::default()
    });
    let frame = render(node, Size::new(8, 1));
    assert_eq!(frame.matches('█').count(), 2, "{frame:?}");
    assert_eq!(frame.matches('▌').count(), 1, "{frame:?}");
    assert_eq!(frame.matches('░').count(), 5, "{frame:?}");
}

#[test]
fn an_empty_ramp_rounds_the_filled_count_like_the_legacy_bar() {
    let node = bar(ProgressBarProps {
        value: Attr::Set(30),
        max: Attr::Set(100),
        width: Attr::Set(8),
        show_percentage: Attr::Set(false),
        partial: Attr::Set(GlyphRamp::default()),
        ..ProgressBarProps::default()
    });
    let frame = render(node, Size::new(8, 1));
    assert_eq!(frame.matches('█').count(), 2, "{frame:?}");
    assert_eq!(frame.matches('░').count(), 6, "{frame:?}");
}

#[test]
fn custom_glyphs_and_a_custom_ramp_render_literally() {
    let node = bar(ProgressBarProps {
        value: Attr::Set(30),
        max: Attr::Set(100),
        width: Attr::Set(8),
        show_percentage: Attr::Set(false),
        filled: Attr::Set(Glyph::new("=").unwrap()),
        empty: Attr::Set(Glyph::new(".").unwrap()),
        partial: Attr::Set(GlyphRamp::new("abc").unwrap()),
        ..ProgressBarProps::default()
    });
    let frame = render(node, Size::new(8, 1));
    assert_eq!(frame.matches('=').count(), 2, "{frame:?}");
    // ceil(0.4 * 3) - 1 = 1, the ramp's second glyph.
    assert_eq!(frame.matches('b').count(), 1, "{frame:?}");
    assert_eq!(frame.matches('.').count(), 5, "{frame:?}");
}

#[test]
fn an_invalid_glyph_through_operator_assignment_keeps_the_theme_default() {
    let node = progress_bar
        .extra(|props| {
            props.value /= 30;
            props.max /= 100;
            props.width /= 8;
            props.show_percentage /= false;
            // A wide glyph and a sequence containing one are both rejected and
            // ignored, exactly like `Style::fill`.
            props.filled /= "界";
            props.partial /= "a界";
        })
        .node();
    let frame = render(node, Size::new(8, 1));
    assert_eq!(frame.matches('█').count(), 2, "{frame:?}");
    assert_eq!(frame.matches('▌').count(), 1, "{frame:?}");
    assert_eq!(frame.matches('░').count(), 5, "{frame:?}");
}

#[test]
fn a_zero_width_bar_paints_nothing() {
    let node = bar(ProgressBarProps {
        value: Attr::Set(50),
        max: Attr::Set(100),
        width: Attr::Set(0),
        show_percentage: Attr::Set(false),
        ..ProgressBarProps::default()
    });
    let frame = render(node, Size::new(8, 1));
    assert!(!frame.contains('█'), "{frame:?}");
    assert!(!frame.contains('░'), "{frame:?}");
}

#[test]
fn the_theme_progress_token_is_the_default_and_stays_overridable() {
    let style = ProgressBarStyle {
        filled: Glyph::new("A").unwrap(),
        empty: Glyph::new("z").unwrap(),
        partial: GlyphRamp::new("mno").unwrap(),
        ..ProgressBarStyle::default()
    };
    let theme = ThemeBuilder::from_palette(ThemeMode::Dark, ThemeColors::dark())
        .progress(style)
        .build();
    let node = ui! {
        <theme_provider value={theme}>
            {bar(ProgressBarProps {
                value: Attr::Set(30),
                max: Attr::Set(100),
                width: Attr::Set(8),
                show_percentage: Attr::Set(false),
                ..ProgressBarProps::default()
            })}
        </theme_provider>
    };
    let frame = render(node, Size::new(8, 1));
    assert_eq!(frame.matches('A').count(), 2, "{frame:?}");
    assert_eq!(frame.matches('n').count(), 1, "{frame:?}");
    assert_eq!(frame.matches('z').count(), 5, "{frame:?}");
}

#[test]
fn a_label_and_percentage_share_the_row_with_the_bar() {
    let node = bar(ProgressBarProps {
        value: Attr::Set(50),
        max: Attr::Set(100),
        width: Attr::Set(4),
        label: Attr::Set("load".into()),
        ..ProgressBarProps::default()
    });
    let frame = render(node, Size::new(16, 1));
    assert!(frame.contains("load"), "{frame:?}");
    assert!(frame.contains("50%"), "{frame:?}");
    assert_eq!(frame.matches('█').count(), 2, "{frame:?}");
    assert_eq!(frame.matches('░').count(), 2, "{frame:?}");
}

// ---------------------------------------------------------------------------
// Drag interaction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    Move,
    End,
}

fn recording_bar(events: Arc<Mutex<Vec<(Phase, u64)>>>, draggable: bool) -> Node {
    let start = events.clone();
    let drag = events.clone();
    let end = events.clone();
    bar(ProgressBarProps {
        value: Attr::Set(0),
        max: Attr::Set(100),
        width: Attr::Set(10),
        show_percentage: Attr::Set(false),
        draggable: Attr::Set(draggable),
        on_drag_start: Attr::Set(EventListener::new(move |event: ProgressBarDragEvent| {
            start.lock().unwrap().push((Phase::Start, event.value));
        })),
        on_drag: Attr::Set(EventListener::new(move |event: ProgressBarDragEvent| {
            drag.lock().unwrap().push((Phase::Move, event.value));
        })),
        on_drag_end: Attr::Set(EventListener::new(move |event: ProgressBarDragEvent| {
            end.lock().unwrap().push((Phase::End, event.value));
        })),
        ..ProgressBarProps::default()
    })
}

struct Harness {
    // Held so the pipeline stays open for the lifetime of the test.
    _input: crossbeam_channel::Sender<Node>,
    output: crossbeam_channel::Receiver<Result<String, icmd::advanced::FrameError>>,
    dispatcher: icmd::advanced::EventDispatcher,
}

impl Harness {
    fn mount(node: Node) -> Self {
        let viewport = Size::new(16, 2);
        let (commit, _, dispatcher) = Commit::new_with_events(viewport);
        let runtime = Runtime::new(Lower::default())
            .then(commit)
            .then(Renderer::new(viewport).unwrap())
            .start_handle();
        let input = runtime.input();
        let output = runtime.output();
        input.send(node).unwrap();
        output
            .recv_timeout(Duration::from_secs(2))
            .expect("initial frame")
            .expect("render");
        Self {
            _input: input,
            output,
            dispatcher,
        }
    }

    fn mouse(&self, kind: MouseEventKind, column: u16) {
        self.dispatcher.dispatch(Event::Mouse(MouseEvent {
            kind,
            column,
            row: 0,
            modifiers: KeyModifiers::empty(),
        }));
    }

    fn settle(&self) {
        let _ = self.output.recv_timeout(Duration::from_millis(150));
    }
}

#[test]
fn dragging_reports_start_move_and_end_with_the_cell_value() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let harness = Harness::mount(recording_bar(events.clone(), true));

    harness.mouse(MouseEventKind::Down(MouseButton::Left), 3);
    assert_eq!(&*events.lock().unwrap(), &[(Phase::Start, 30)]);

    harness.mouse(MouseEventKind::Drag(MouseButton::Left), 6);
    assert_eq!(
        &*events.lock().unwrap(),
        &[(Phase::Start, 30), (Phase::Move, 60)]
    );

    // Staying inside one cell does not report the same value twice.
    harness.mouse(MouseEventKind::Drag(MouseButton::Left), 6);
    assert_eq!(events.lock().unwrap().len(), 2);

    harness.mouse(MouseEventKind::Up(MouseButton::Left), 6);
    assert_eq!(
        &*events.lock().unwrap(),
        &[(Phase::Start, 30), (Phase::Move, 60), (Phase::End, 60)]
    );
    harness.settle();
}

#[test]
fn dragging_past_the_right_edge_clamps_to_max() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let harness = Harness::mount(recording_bar(events.clone(), true));

    harness.mouse(MouseEventKind::Down(MouseButton::Left), 3);
    harness.mouse(MouseEventKind::Drag(MouseButton::Left), 15);
    harness.mouse(MouseEventKind::Up(MouseButton::Left), 15);
    assert_eq!(
        &*events.lock().unwrap(),
        &[(Phase::Start, 30), (Phase::Move, 100), (Phase::End, 100)]
    );
    harness.settle();
}

#[test]
fn dragging_left_of_the_bar_clamps_to_zero() {
    // The label shifts the bar to column 3, so a drag to column 1 lands left of
    // the bar and still reaches the captured bar.
    let events = Arc::new(Mutex::new(Vec::new()));
    let node = progress_bar
        .props(ProgressBarProps {
            value: Attr::Set(0),
            max: Attr::Set(100),
            width: Attr::Set(10),
            label: Attr::Set("ab".into()),
            show_percentage: Attr::Set(false),
            draggable: Attr::Set(true),
            on_drag: Attr::Set(EventListener::new({
                let events = events.clone();
                move |event: ProgressBarDragEvent| {
                    events.lock().unwrap().push((Phase::Move, event.value));
                }
            })),
            ..ProgressBarProps::default()
        })
        .node();
    let harness = Harness::mount(node);

    harness.mouse(MouseEventKind::Down(MouseButton::Left), 4);
    harness.mouse(MouseEventKind::Drag(MouseButton::Left), 1);
    assert_eq!(&*events.lock().unwrap(), &[(Phase::Move, 0)]);
    harness.settle();
}

#[test]
fn a_release_on_a_new_cell_reports_the_change_then_the_end() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let harness = Harness::mount(recording_bar(events.clone(), true));

    // The release lands on a cell no motion event reached, so the change is
    // still reported before the drag ends.
    harness.mouse(MouseEventKind::Down(MouseButton::Left), 3);
    harness.mouse(MouseEventKind::Up(MouseButton::Left), 8);
    assert_eq!(
        &*events.lock().unwrap(),
        &[(Phase::Start, 30), (Phase::Move, 80), (Phase::End, 80)]
    );
    harness.settle();
}

#[test]
fn a_cancelled_drag_still_reports_its_end() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let harness = Harness::mount(recording_bar(events.clone(), true));

    harness.mouse(MouseEventKind::Down(MouseButton::Left), 4);
    harness.dispatcher.dispatch(Event::FocusLost);
    assert_eq!(
        &*events.lock().unwrap(),
        &[(Phase::Start, 40), (Phase::End, 40)]
    );
    harness.settle();
}

#[test]
fn a_non_draggable_bar_leaves_the_caller_pointer_listeners_alone() {
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    let node = progress_bar
        .props(ProgressBarProps {
            value: Attr::Set(0),
            max: Attr::Set(100),
            width: Attr::Set(10),
            show_percentage: Attr::Set(false),
            ..ProgressBarProps::default()
        })
        .events(move |handlers| {
            handlers.pointer_down /= move |_event: PointerEvent| {
                seen.fetch_add(1, Ordering::SeqCst);
            };
        })
        .node();
    let harness = Harness::mount(node);

    harness.mouse(MouseEventKind::Down(MouseButton::Left), 3);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "an opted-out bar must not consume the press"
    );
    harness.settle();
}

#[test]
fn a_draggable_bar_consumes_the_press_so_ancestors_do_not_also_react() {
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    let node = progress_bar
        .props(ProgressBarProps {
            value: Attr::Set(0),
            max: Attr::Set(100),
            width: Attr::Set(10),
            show_percentage: Attr::Set(false),
            draggable: Attr::Set(true),
            ..ProgressBarProps::default()
        })
        .events(move |handlers| {
            handlers.pointer_down /= move |_event: PointerEvent| {
                seen.fetch_add(1, Ordering::SeqCst);
            };
        })
        .node();
    let harness = Harness::mount(node);

    harness.mouse(MouseEventKind::Down(MouseButton::Left), 3);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "a drag must stop at the bar, not reach the host's own listener"
    );
    harness.settle();
}

#[test]
fn pressing_the_label_does_not_start_a_drag() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let node = progress_bar
        .props(ProgressBarProps {
            value: Attr::Set(0),
            max: Attr::Set(100),
            width: Attr::Set(10),
            label: Attr::Set("ab".into()),
            show_percentage: Attr::Set(false),
            draggable: Attr::Set(true),
            on_drag_start: Attr::Set(EventListener::new({
                let events = events.clone();
                move |event: ProgressBarDragEvent| {
                    events.lock().unwrap().push((Phase::Start, event.value));
                }
            })),
            ..ProgressBarProps::default()
        })
        .node();
    let harness = Harness::mount(node);

    // Column 1 is inside the label; the bar starts at column 3.
    harness.mouse(MouseEventKind::Down(MouseButton::Left), 1);
    assert!(
        events.lock().unwrap().is_empty(),
        "the label is not a drag target"
    );
    harness.settle();
}
