use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::{
    Attr, Commit, Component, ComponentContext, Dimension, Layout, Lower, Node, Props, Renderer,
    Runtime, ScrollAreaProps, ScrollAxes, ScrollEvent, ScrollOffset, ScrollbarVisibility, Size,
    StateSetter, scroll_area, text,
};

fn render_pipeline(
    viewport: Size,
) -> (
    Sender<Node>,
    Receiver<Result<String, icmd::FrameError>>,
    icmd::EventDispatcher,
) {
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start();
    (input, output, dispatcher)
}

fn area(
    axes: ScrollAxes,
    visibility: ScrollbarVisibility,
    width: u16,
    height: u16,
    children: impl IntoIterator<Item = Node>,
) -> Node {
    scroll_area
        .props(ScrollAreaProps {
            axes: Attr::Set(axes),
            scrollbar_visibility: Attr::Set(visibility),
            ..ScrollAreaProps::default()
        })
        .style(move |style| {
            style.width /= Dimension::Cells(width);
            style.height /= Dimension::Cells(height);
            if matches!(axes, ScrollAxes::Horizontal | ScrollAxes::Both) {
                style.layout /= Layout::Horizontal;
            }
        })
        .children(children)
}

fn rows(count: usize) -> Vec<Node> {
    (0..count)
        .map(|index| text(format!("row{index}")))
        .collect()
}

fn controlled_area(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (offset, set_offset): (ScrollOffset, StateSetter<ScrollOffset>) =
        cx.use_state(ScrollOffset::default);
    scroll_area
        .props(ScrollAreaProps {
            axes: Attr::Set(ScrollAxes::Vertical),
            offset: Attr::Set(offset),
            ..ScrollAreaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
        })
        .events(move |handlers| {
            handlers.scroll /= move |event: ScrollEvent| set_offset.set(event.offset);
        })
        .children(rows(10))
}

#[test]
fn vertical_wheel_and_keyboard_scroll() {
    let viewport = Size::new(8, 4);
    let node = area(
        ScrollAxes::Vertical,
        ScrollbarVisibility::Auto,
        8,
        4,
        rows(10),
    );
    let (input, output, dispatcher) = render_pipeline(viewport);
    input.send(node).unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    // The diff encoder now writes contiguous row runs, so a leading border
    // can share the cursor move with this first content glyph.
    assert!(first.contains('0'));

    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    let second = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_ne!(first, second);
    assert!(second.contains('1'));

    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Key(KeyEvent::new_with_kind(
        KeyCode::End,
        KeyModifiers::empty(),
        KeyEventKind::Press,
    )));
    let last = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(last.contains('9'));
}

#[test]
fn horizontal_and_bidirectional_axes_scroll() {
    let viewport = Size::new(4, 2);
    let (input, output, dispatcher) = render_pipeline(viewport);
    input
        .send(area(
            ScrollAxes::Horizontal,
            ScrollbarVisibility::Always,
            4,
            2,
            [text("abcdefgh")],
        ))
        .unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollRight,
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    let second = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_ne!(first, second);

    let (input, output, dispatcher) = render_pipeline(viewport);
    input
        .send(area(
            ScrollAxes::Both,
            ScrollbarVisibility::Hidden,
            4,
            2,
            [text("abcdefgh\n01234567")],
        ))
        .unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(!first.contains('│'));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollRight,
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    assert_ne!(
        first,
        output
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap()
    );
}

#[test]
fn auto_and_hidden_visibility_follow_overflow() {
    let viewport = Size::new(8, 4);
    let (input, output, _) = render_pipeline(viewport);
    input
        .send(area(
            ScrollAxes::Vertical,
            ScrollbarVisibility::Auto,
            8,
            4,
            [text("one")],
        ))
        .unwrap();
    let no_overflow = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(!no_overflow.contains('│'));

    let (input, output, _) = render_pipeline(viewport);
    input
        .send(area(
            ScrollAxes::Vertical,
            ScrollbarVisibility::Always,
            8,
            4,
            rows(8),
        ))
        .unwrap();
    let always = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(always.contains('│'));

    let (input, output, dispatcher) = render_pipeline(viewport);
    input
        .send(area(
            ScrollAxes::Vertical,
            ScrollbarVisibility::Hidden,
            8,
            4,
            rows(8),
        ))
        .unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(!first.contains('│'));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert_ne!(
        first,
        output
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap()
    );
}

#[test]
fn input_flags_are_independent() {
    let viewport = Size::new(8, 4);
    let node = scroll_area
        .props(ScrollAreaProps {
            axes: Attr::Set(ScrollAxes::Vertical),
            enable_wheel: Attr::Set(false),
            ..ScrollAreaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
        })
        .children(rows(10));
    let (input, output, dispatcher) = render_pipeline(viewport);
    input.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(output.recv_timeout(Duration::from_millis(100)).is_err());
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Down,
        KeyModifiers::empty(),
        KeyEventKind::Press,
    )));
    assert!(
        output
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap()
            .contains('1')
    );
}

#[test]
fn bars_support_track_clicks_and_dragging() {
    let viewport = Size::new(8, 4);
    let (input, output, dispatcher) = render_pipeline(viewport);
    input
        .send(area(
            ScrollAxes::Vertical,
            ScrollbarVisibility::Always,
            8,
            4,
            rows(10),
        ))
        .unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 7,
        row: 3,
        modifiers: KeyModifiers::empty(),
    }));
    let clicked = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(clicked.contains('6') || clicked.contains('7'));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 7,
        row: 3,
        modifiers: KeyModifiers::empty(),
    }));
}

#[test]
fn scroll_events_expose_structured_offsets_and_bubble() {
    let seen = Arc::new(AtomicUsize::new(0));
    let last_offset = Arc::new(std::sync::Mutex::new(ScrollOffset::default()));
    let seen_clone = seen.clone();
    let offset_clone = last_offset.clone();
    let node = scroll_area
        .props(ScrollAreaProps {
            axes: Attr::Set(ScrollAxes::Vertical),
            ..ScrollAreaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
        })
        .events(move |handlers| {
            handlers.scroll /= move |event: ScrollEvent| {
                seen_clone.fetch_add(1, Ordering::SeqCst);
                *offset_clone.lock().unwrap() = event.offset;
                assert!(event.max_offset.y > 0);
                assert!(event.delta.y > 0);
            };
        })
        .children(rows(10));
    let (input, output, dispatcher) = render_pipeline(Size::new(8, 4));
    input.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    assert_eq!(last_offset.lock().unwrap().y, 1);
}

#[test]
fn controlled_offset_is_not_mutated_by_runtime() {
    let node = scroll_area
        .props(ScrollAreaProps {
            axes: Attr::Set(ScrollAxes::Vertical),
            offset: Attr::Set(ScrollOffset::new(0, 0)),
            ..ScrollAreaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
        })
        .children(rows(10));
    let (input, output, dispatcher) = render_pipeline(Size::new(8, 4));
    input.send(node).unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(output.recv_timeout(Duration::from_millis(100)).is_err());
    assert!(first.contains('0'));
}

#[test]
fn controlled_offset_round_trips_through_component_state() {
    let (input, output, dispatcher) = render_pipeline(Size::new(8, 4));
    input.send(controlled_area.node()).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    let updated = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(updated.contains('1'));
}
