use std::time::Duration;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use icmd::{
    Commit, Component, ComponentContext, Dimension, Layout, Lower, Node, Overflow, Props, Renderer,
    Runtime, ScrollAxes, ScrollProps, Size, text, ui, view,
};

fn surface(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let children = props.children.iter().cloned().collect::<Node>();
    surface_content
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Scroll(ScrollProps::default());
        })
        .child(children)
}

fn surface_content(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    ui! { <view dom={props.dom.clone()}>{props.children.clone().into_iter().collect::<Node>()}</view> }
}

fn horizontal_surface(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let children = props.children.iter().cloned().collect::<Node>();
    let overflow = ScrollProps {
        axes: ScrollAxes::Horizontal,
        ..ScrollProps::default()
    };
    surface_content
        .style(move |style| {
            style.layout /= Layout::Horizontal;
            style.width /= Dimension::Cells(4);
            style.height /= Dimension::Cells(2);
            style.overflow /= Overflow::Scroll(overflow);
        })
        .child(children)
}

fn auto_surface(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let children = props.children.iter().cloned().collect::<Node>();
    let overflow = icmd::AutoProps {
        draw_scrollbar: true,
        ..icmd::AutoProps::default()
    };
    surface_content
        .style(move |style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Auto(overflow);
        })
        .child(children)
}

fn hidden_scroll_surface(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let children = props.children.iter().cloned().collect::<Node>();
    let overflow = ScrollProps {
        draw_scrollbar: false,
        ..ScrollProps::default()
    };
    surface_content
        .style(move |style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Scroll(overflow);
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
