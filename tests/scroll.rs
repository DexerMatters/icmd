use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::{
    Commit, Component, ComponentContext, Dimension, Layout, Lower, Node, Overflow, Props, Renderer,
    Runtime, Size, text, ui, view,
};

fn surface(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let children = props.children.iter().cloned().collect::<Node>();
    surface_content
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Scroll;
        })
        .child(children)
}

fn surface_content(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    ui! { <view dom={props.dom.clone()}>{props.children.clone().into_iter().collect::<Node>()}</view> }
}

fn horizontal_surface(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let children = props.children.iter().cloned().collect::<Node>();
    surface_content
        .style(move |style| {
            style.layout /= Layout::Horizontal;
            style.width /= Dimension::Cells(4);
            style.height /= Dimension::Cells(2);
            style.overflow_x /= Overflow::Scroll;
            style.overflow_y /= Overflow::Clip;
        })
        .child(children)
}

fn auto_surface(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let children = props.children.iter().cloned().collect::<Node>();
    surface_content
        .style(move |style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Auto;
        })
        .child(children)
}

fn hidden_scroll_surface(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let children = props.children.iter().cloned().collect::<Node>();
    surface_content
        .style(move |style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Scroll;
            style.scroll.draw_scrollbar /= false;
        })
        .child(children)
}

#[test]
fn wheel_scrolls_and_keyboard_pages_scroll_container() {
    let children = (0..10)
        .map(|index| text(format!("row{index}")))
        .collect::<Vec<_>>();
    let node = surface.children(children);
    let viewport = Size::new(8, 4);
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
    input.send(node).unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(first.contains("H0"));

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
    assert!(!second.contains("H0"));
    assert!(second.contains("H1"));

    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(crossterm::event::MouseButton::Left),
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
    assert!(last.contains("H9"));
}

#[test]
fn horizontal_wheel_scrolls_unwrapped_content() {
    let node = horizontal_surface
        .children([text("abcdefgh")])
        .key("horizontal");
    let viewport = Size::new(4, 2);
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
    input.send(node).unwrap();
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
}

#[test]
fn auto_hides_bars_without_overflow_and_hidden_scroll_keeps_scrolling() {
    let viewport = Size::new(8, 4);
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
    input.send(auto_surface.child("one")).unwrap();
    let auto_frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(!auto_frame.contains('│'));
    assert!(!auto_frame.contains('─'));
    drop(dispatcher);
    drop(input);

    let rows = (0..8)
        .map(|index| text(format!("row{index}")))
        .collect::<Vec<_>>();
    let (commit, _, _) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
    input.send(auto_surface.children(rows)).unwrap();
    let overflowing_auto = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(overflowing_auto.contains('│'));
    assert!(!overflowing_auto.contains('─'));
    drop(input);

    let children = (0..8)
        .map(|index| text(format!("row{index}")))
        .collect::<Vec<_>>();
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
    input
        .send(hidden_scroll_surface.children(children))
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
    let second = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_ne!(first, second);
}

#[test]
fn scrollbar_can_be_dragged_by_default_and_disabled_independently() {
    let children = (0..10)
        .map(|index| text(format!("row{index}")))
        .collect::<Vec<_>>();
    let viewport = Size::new(8, 4);
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
    input.send(surface.children(children)).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();

    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 7,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 7,
        row: 2,
        modifiers: KeyModifiers::empty(),
    }));
    let dragged = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(dragged.contains("H7"));
    assert!(!dragged.contains("H0"));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 7,
        row: 2,
        modifiers: KeyModifiers::empty(),
    }));

    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 7,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    let clicked = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(clicked.contains("H0"));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 7,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 2,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(output.recv_timeout(Duration::from_millis(100)).is_err());

    let children = (0..10)
        .map(|index| text(format!("row{index}")))
        .collect::<Vec<_>>();
    let node = surface_content
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Scroll;
            style.scroll.enable_mouse /= false;
        })
        .children(children);
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
    input.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 7,
        row: 2,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 7,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(output.recv_timeout(Duration::from_millis(100)).is_err());
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(output.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn wheel_can_be_disabled_without_disabling_keyboard_scrolling() {
    let children = (0..10)
        .map(|index| text(format!("row{index}")))
        .collect::<Vec<_>>();
    let viewport = Size::new(8, 4);
    let node = surface_content
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Scroll;
            style.scroll.enable_wheel /= false;
        })
        .children(children);
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
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
    let keyboard = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(keyboard.contains("H1"));
}

#[test]
fn overflow_surfaces_emit_scroll_events() {
    let events = Arc::new(AtomicUsize::new(0));
    let listener_events = events.clone();
    let node = surface_content
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Scroll;
        })
        .events(move |handlers| {
            handlers.scroll /= move |_event| {
                listener_events.fetch_add(1, Ordering::SeqCst);
            };
        })
        .children(
            (0..10)
                .map(|index| text(format!("row{index}")))
                .collect::<Vec<_>>(),
        );
    let viewport = Size::new(8, 4);
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport))
        .start();
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
    assert!(events.load(Ordering::SeqCst) > 0);
}
