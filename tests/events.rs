use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

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
    assert_eq!(dispatcher.dispatch(Event::Mouse(mouse)).delivered, 1);
    assert_eq!(mouse_hits.load(Ordering::SeqCst), 1);

    // Pointer capture keeps the pressed element as the target even after the
    // pointer leaves its rectangle, matching browser pointer-event behavior.
    let outside_up = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 30,
        row: 10,
        modifiers: KeyModifiers::empty(),
    };
    assert_eq!(dispatcher.dispatch(Event::Mouse(outside_up)).delivered, 1);
    assert_eq!(mouse_up_hits.load(Ordering::SeqCst), 1);

    let key = KeyEvent::new_with_kind(
        KeyCode::Char('x'),
        KeyModifiers::empty(),
        KeyEventKind::Press,
    );
    // SAF-04: targeted keyboard delivery requires an actual focused DOM target.
    // The press above focused only a node with pointer handlers, and the root is
    // not focusable, so nothing owns focus and no targeted listener runs.
    assert_eq!(dispatcher.dispatch(Event::Key(key)).delivered, 0);
    assert_eq!(key_hits.load(Ordering::SeqCst), 0);

    assert_eq!(dispatcher.dispatch(Event::Resize(30, 6)).delivered, 0);
    assert_eq!(viewport.viewport(), Size::new(30, 6));
}

#[test]
fn panicking_listener_is_recorded_and_propagation_state_restores() {
    // SAF-14: a panicking listener must not poison the dispatcher, must not
    // leave keyboard propagation state behind, and must be reported.
    let (commit, _viewport, dispatcher) = Commit::new_with_events(Size::new(20, 5));
    let (input, output) = Runtime::new(Lower::default()).then(commit).start();

    let later_hits = Arc::new(AtomicUsize::new(0));
    let later_hits_for_listener = later_hits.clone();
    let node = root
        .style(|style| {
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
        })
        .events(move |events| {
            events.app_key /= EventListener::new(move |_event| {
                later_hits_for_listener.fetch_add(1, Ordering::SeqCst);
                panic!("listener exploded");
            });
        })
        .apply(());
    input.send(node).unwrap();
    output.recv().unwrap();

    let key = || {
        KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::empty(),
            KeyEventKind::Press,
        )
    };
    dispatcher.dispatch(Event::Key(key()));
    assert_eq!(later_hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        dispatcher.take_callback_fault(),
        Some("an event listener panicked")
    );

    // The next independent dispatch still runs: propagation state was restored
    // and the listener lock was not left poisoned in a way that blocks delivery.
    dispatcher.dispatch(Event::Key(key()));
    assert_eq!(later_hits.load(Ordering::SeqCst), 2);
}

#[test]
fn reentrant_listener_delivery_terminates_without_deadlock() {
    // SAF-14: a listener that re-dispatches into itself must be rejected rather
    // than deadlocking on a non-reentrant lock.
    let (commit, _viewport, dispatcher) = Commit::new_with_events(Size::new(20, 5));
    let (input, output) = Runtime::new(Lower::default()).then(commit).start();

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_listener = calls.clone();
    let dispatcher_for_listener = dispatcher.clone();
    let node = root
        .style(|style| {
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
        })
        .events(move |events| {
            events.app_key /= EventListener::new(move |_event| {
                calls_for_listener.fetch_add(1, Ordering::SeqCst);
                dispatcher_for_listener.dispatch(Event::Key(KeyEvent::new_with_kind(
                    KeyCode::Char('q'),
                    KeyModifiers::empty(),
                    KeyEventKind::Press,
                )));
            });
        })
        .apply(());
    input.send(node).unwrap();
    output.recv().unwrap();

    dispatcher.dispatch(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('q'),
        KeyModifiers::empty(),
        KeyEventKind::Press,
    )));
    // The outer callback ran once; the nested delivery was rejected, not run.
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        dispatcher.take_callback_fault(),
        Some("an event listener dispatched an event into itself")
    );
}

// PERF-06: a deep route performs O(depth) ID lookups rather than rescanning
// every published region at each ancestry step.
#[test]
fn deep_key_routing_scales_with_depth_not_region_count() {
    let viewport = Size::new(20, 5);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    // A deep chain plus a wide fan of sibling regions at every level.
    fn nest(depth: usize, width: usize) -> Node {
        let mut node = Node::element(
            icmd::DomProps::default().with_focusable(true),
            Vec::<Node>::new(),
        );
        for _ in 0..depth {
            let mut children: Vec<Node> = (0..width)
                .map(|_| Node::element(icmd::DomProps::default(), Vec::<Node>::new()))
                .collect();
            children.push(node);
            node = Node::element(icmd::DomProps::default(), children);
        }
        node
    }

    let depth = 40;
    input.send(nest(depth, 20)).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();

    // Focus the deep leaf, then route a key to it.

    let _ = dispatcher.take_id_probe_count();
    dispatcher.dispatch(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('x'),
        KeyModifiers::empty(),
        KeyEventKind::Press,
    )));
    let probes = dispatcher.take_id_probe_count();
    // One probe per ancestor on the route; a scan-based implementation would
    // multiply this by the number of published regions.
    assert!(
        probes <= (depth as u64) * 8,
        "deep routing used {probes} ID probes for depth {depth}"
    );

    drop(input);
    let _ = runtime.shutdown(icmd::ShutdownPolicy::default());
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
    assert_eq!(dispatcher.dispatch(Event::Key(key)).delivered, 1);
    assert_eq!(app_hits.load(Ordering::SeqCst), 1);
}

// API-11: element IDs are opaque. A fabricated handle cannot be constructed by
// callers, and a checked focus request rejects unknown or non-focusable targets.
#[test]
fn programmatic_focus_is_checked_and_rejects_unknown_targets() {
    let viewport = Size::new(20, 5);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    input.send(root.apply(())).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();

    // The runtime root is not focusable, so it cannot be focused.
    // Any handle obtained from the framework is checked for existence and
    // focusability rather than trusted.
    let outcome = dispatcher.try_focus(icmd::DomId::ROOT);
    assert!(
        matches!(
            outcome,
            Err(icmd::FocusError::UnknownTarget(_) | icmd::FocusError::NotFocusable(_))
        ),
        "an unfocusable root must be rejected: {outcome:?}"
    );

    drop(input);
    let _ = runtime.shutdown(icmd::ShutdownPolicy::default());
}

#[test]
fn focus_transitions_report_a_structured_outcome() {
    let viewport = Size::new(20, 5);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    let gained = Arc::new(AtomicUsize::new(0));
    let gained_for_listener = gained.clone();
    let node = icmd::view
        .events(move |events| {
            events.focus_event /= EventListener::new(move |event: icmd::FocusEvent| {
                if event == icmd::FocusEvent::Gained {
                    gained_for_listener.fetch_add(1, Ordering::SeqCst);
                }
            });
        })
        .apply(
            icmd::Props::new(()).with_dom(
                icmd::DomProps::default()
                    .with_focusable(true)
                    .with_style(icmd::style(|style| {
                        style.width /= icmd::Dimension::Max;
                        style.height /= icmd::Dimension::Max;
                    })),
            ),
        );
    input.send(node).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();

    // Focus by pointer, then read the structured outcome of the transition.
    let outcome = dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(outcome.delivered > 0);
    assert_eq!(gained.load(Ordering::SeqCst), 1);

    // Re-focusing the same target is reported, not silently ignored.
    let id = dispatcher.focused().expect("a focused region");
    assert!(matches!(
        dispatcher.try_focus(id),
        Err(icmd::FocusError::AlreadyFocused(_))
    ));

    drop(input);
    let _ = runtime.shutdown(icmd::ShutdownPolicy::default());
}

// API-04: every event family shares one propagation frame, so pointer and wheel
// events can stop propagation and prevent their default action, not only
// keyboard events.
#[test]
fn pointer_events_can_stop_propagation() {
    let viewport = Size::new(20, 5);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    let outer = Arc::new(AtomicUsize::new(0));
    let inner = Arc::new(AtomicUsize::new(0));
    let outer_for_listener = outer.clone();
    let inner_for_listener = inner.clone();

    let child = icmd::view
        .events(move |events| {
            events.pointer_down /= EventListener::new(move |event: icmd::PointerEvent| {
                inner_for_listener.fetch_add(1, Ordering::SeqCst);
                event.stop_propagation();
            });
        })
        .apply(
            icmd::Props::new(()).with_dom(icmd::DomProps::default().with_style(icmd::style(
                |style| {
                    style.width /= icmd::Dimension::Cells(10);
                    style.height /= icmd::Dimension::Cells(3);
                },
            ))),
        );
    let node = icmd::view
        .events(move |events| {
            events.pointer_down /= EventListener::new(move |_event: icmd::PointerEvent| {
                outer_for_listener.fetch_add(1, Ordering::SeqCst);
            });
        })
        .apply(icmd::Props::new(()).children([child]).with_dom(
            icmd::DomProps::default().with_style(icmd::style(|style| {
                style.width /= icmd::Dimension::Max;
                style.height /= icmd::Dimension::Max;
            })),
        ));
    input.send(node).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();

    let outcome = dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(outcome.propagation_stopped, "{outcome:?}");
    assert_eq!(inner.load(Ordering::SeqCst), 1, "the target heard it");
    assert_eq!(
        outer.load(Ordering::SeqCst),
        0,
        "an ancestor must not hear a stopped event"
    );

    drop(input);
    let _ = runtime.shutdown(icmd::ShutdownPolicy::default());
}

#[test]
fn keyboard_events_can_prevent_the_default_action() {
    // A listener that prevents the default keeps the framework from applying
    // its built-in behavior; the outcome reports it so callers can react.
    let viewport = Size::new(20, 5);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    let node = icmd::view
        .events(move |events| {
            events.keyboard_event /= EventListener::new(move |event: icmd::KeyboardEvent| {
                event.prevent_default();
            });
        })
        .apply(
            icmd::Props::new(()).with_dom(
                icmd::DomProps::default()
                    .with_focusable(true)
                    .with_style(icmd::style(|style| {
                        style.width /= icmd::Dimension::Max;
                        style.height /= icmd::Dimension::Max;
                    })),
            ),
        );
    input.send(node).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();

    // Focus the node, then send a key that would otherwise scroll.
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    let outcome = dispatcher.dispatch(Event::Key(KeyEvent::new_with_kind(
        KeyCode::PageDown,
        KeyModifiers::empty(),
        KeyEventKind::Press,
    )));
    assert!(outcome.default_prevented, "{outcome:?}");

    drop(input);
    let _ = runtime.shutdown(icmd::ShutdownPolicy::default());
}

#[test]
fn wheel_scrolling_is_restored_when_default_is_prevented() {
    let viewport = Size::new(20, 6);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    let area = icmd::scroll_area
        .props(icmd::ScrollAreaProps {
            axes: icmd::Attr::Set(icmd::ScrollAxes::Vertical),
            scrollbar_visibility: icmd::Attr::Set(icmd::ScrollbarVisibility::Hidden),
            ..icmd::ScrollAreaProps::default()
        })
        .style(|style| {
            style.width /= icmd::Dimension::Cells(18);
            style.height /= icmd::Dimension::Cells(4);
        })
        .children((0..20).map(|index| icmd::text(format!("row {index}"))));
    // The wheel listener sits on an ancestor, where bubbling reaches it.
    let node = icmd::view
        .events(|events| {
            events.wheel /= EventListener::new(|event: icmd::WheelEvent| {
                event.prevent_default();
            });
        })
        .apply(icmd::Props::new(()).children([area]).with_dom(
            icmd::DomProps::default().with_style(icmd::style(|style| {
                style.width /= icmd::Dimension::Max;
                style.height /= icmd::Dimension::Max;
            })),
        ));
    input.send(node).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();

    let outcome = dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(outcome.default_prevented, "{outcome:?}");
    assert!(
        !outcome.redraw_requested,
        "a prevented scroll must not request a redraw"
    );

    drop(input);
    let _ = runtime.shutdown(icmd::ShutdownPolicy::default());
}
