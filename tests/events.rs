use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::{
    Commit, Component, ComponentContext, Dimension, EventListener, Layout, Lower, Node, Props,
    Runtime, Size, ui, view,
};

fn root(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    ui! { <view dom={props.dom.clone()}>"root"</view> }
}

#[test]
fn mouse_listener_is_hit_tested_and_keyboard_focus_is_routed() {
    let (commit, viewport, dispatcher) = Commit::new_with_events(Size::new(20, 5));
    let (input, output) = Runtime::new(Lower::default()).then(commit).start();

    let mouse_hits = Arc::new(AtomicUsize::new(0));
    let mouse_up_hits = Arc::new(AtomicUsize::new(0));
    let key_hits = Arc::new(AtomicUsize::new(0));
    let mouse_hits_for_listener = mouse_hits.clone();
    let mouse_up_hits_for_listener = mouse_up_hits.clone();
    let key_hits_for_listener = key_hits.clone();
    let node = root
        .style(|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
        })
        .events(move |events| {
            let mouse_hits = mouse_hits_for_listener.clone();
            events.pointer_down /= EventListener::new(move |_event| {
                mouse_hits.fetch_add(1, Ordering::SeqCst);
            });

            let mouse_up_hits = mouse_up_hits_for_listener.clone();
            events.pointer_up /= EventListener::new(move |_event| {
                mouse_up_hits.fetch_add(1, Ordering::SeqCst);
            });

            let key_hits = key_hits_for_listener.clone();
            events.keyboard_event /= EventListener::new(move |_event| {
                key_hits.fetch_add(1, Ordering::SeqCst);
            });
        })
        .apply(());
    input.send(node).unwrap();
    output.recv().unwrap();

    let mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    };
    assert_eq!(dispatcher.dispatch(Event::Mouse(mouse)), 1);
    assert_eq!(mouse_hits.load(Ordering::SeqCst), 1);

    // Pointer capture keeps the pressed element as the target even after the
    // pointer leaves its rectangle, matching browser pointer-event behavior.
    let outside_up = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 30,
        row: 10,
        modifiers: KeyModifiers::empty(),
    };
    assert_eq!(dispatcher.dispatch(Event::Mouse(outside_up)), 1);
    assert_eq!(mouse_up_hits.load(Ordering::SeqCst), 1);

    let key = KeyEvent::new_with_kind(
        KeyCode::Char('x'),
        KeyModifiers::empty(),
        KeyEventKind::Press,
    );
    // SAF-04: targeted keyboard delivery requires an actual focused DOM target.
    // The press above focused only a node with pointer handlers, and the root is
    // not focusable, so nothing owns focus and no targeted listener runs.
    assert_eq!(dispatcher.dispatch(Event::Key(key)), 0);
    assert_eq!(key_hits.load(Ordering::SeqCst), 0);

    assert_eq!(dispatcher.dispatch(Event::Resize(30, 6)), 0);
    assert_eq!(viewport.viewport(), Size::new(30, 6));
}

#[test]
fn application_global_key_listener_fires_without_focus() {
    let (commit, _viewport, dispatcher) = Commit::new_with_events(Size::new(20, 5));
    let (input, output) = Runtime::new(Lower::default()).then(commit).start();

    let app_hits = Arc::new(AtomicUsize::new(0));
    let app_hits_for_listener = app_hits.clone();
    let node = root
        .style(|style| {
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
        })
        .events(move |events| {
            let app_hits = app_hits_for_listener.clone();
            events.app_key /= EventListener::new(move |_event| {
                app_hits.fetch_add(1, Ordering::SeqCst);
            });
        })
        .apply(());
    input.send(node).unwrap();
    output.recv().unwrap();

    let key = KeyEvent::new_with_kind(
        KeyCode::Char('q'),
        KeyModifiers::empty(),
        KeyEventKind::Press,
    );
    assert_eq!(dispatcher.dispatch(Event::Key(key)), 1);
    assert_eq!(app_hits.load(Ordering::SeqCst), 1);
}
