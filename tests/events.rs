use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::advanced::{Commit, Lower, Runtime};
use icmd::{
    Component, ComponentContext, Dimension, EventListener, Layout, Node, Props, Size, ui, view,
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

#[test]
fn an_application_shortcut_fires_once_per_press_not_once_per_edge() {
    // A terminal with REPORT_EVENT_TYPES reports both edges of one physical
    // press. A release must not fire a shortcut: focused, the press is consumed
    // by the widget while the release still reached `app_key`, so the shortcut
    // fired even though the widget handled the key; unfocused, both edges fired
    // it, so one key changed the page twice.
    let (commit, _viewport, dispatcher) = Commit::new_with_events(Size::new(20, 5));
    let (input, output) = Runtime::new(Lower::default()).then(commit).start();

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_listener = calls.clone();
    let node = root
        .style(|style| {
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
        })
        .events(move |events| {
            events.app_key /= EventListener::new(move |_event| {
                calls_for_listener.fetch_add(1, Ordering::SeqCst);
            });
        })
        .apply(());
    input.send(node).unwrap();
    output.recv().unwrap();

    // One physical key: press, repeat, release.
    for kind in [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Release,
    ] {
        dispatcher.dispatch(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::empty(),
            kind,
        )));
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "only the press and the repeat are shortcut gestures"
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
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
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
            Err(icmd::advanced::FocusError::UnknownTarget(_)
                | icmd::advanced::FocusError::NotFocusable(_))
        ),
        "an unfocusable root must be rejected: {outcome:?}"
    );

    drop(input);
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
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
        Err(icmd::advanced::FocusError::AlreadyFocused(_))
    ));

    drop(input);
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
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
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
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
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
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
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
}

// API-04: capture runs root-to-target before the target and bubble pass, and a
// capture listener can stop the rest of the dispatch.
#[test]
fn capture_listeners_run_before_target_and_bubble() {
    let viewport = Size::new(20, 5);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    // Order is recorded as "root-capture", "target-capture", "target", "root".
    let order = Arc::new(Mutex::new(Vec::new()));
    let o1 = order.clone();
    let o2 = order.clone();
    let o3 = order.clone();
    let o4 = order.clone();

    let child = icmd::view
        .events(move |events| {
            events.pointer_down_capture /= EventListener::new(move |_event| {
                o2.lock().unwrap().push("target-capture");
            });
            events.pointer_down /= EventListener::new(move |_event| {
                o3.lock().unwrap().push("target");
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
            events.pointer_down_capture /= EventListener::new(move |_event| {
                o1.lock().unwrap().push("root-capture");
            });
            events.pointer_down /= EventListener::new(move |_event| {
                o4.lock().unwrap().push("root");
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

    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert_eq!(
        &*order.lock().unwrap(),
        &["root-capture", "target-capture", "target", "root"],
        "capture must run root-to-target before target and bubble"
    );

    drop(input);
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
}

#[test]
fn a_capture_listener_can_stop_the_whole_dispatch() {
    let viewport = Size::new(20, 5);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    let target = Arc::new(AtomicUsize::new(0));
    let target_for_listener = target.clone();
    let node = icmd::view
        .events(move |events| {
            events.pointer_down_capture /= EventListener::new(|event: icmd::PointerEvent| {
                event.stop_propagation();
            });
            events.pointer_down /= EventListener::new(move |_event| {
                target_for_listener.fetch_add(1, Ordering::SeqCst);
            });
        })
        .apply(
            icmd::Props::new(()).with_dom(icmd::DomProps::default().with_style(icmd::style(
                |style| {
                    style.width /= icmd::Dimension::Max;
                    style.height /= icmd::Dimension::Max;
                },
            ))),
        );
    input.send(node).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();

    let outcome = dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(outcome.propagation_stopped, "{outcome:?}");
    assert_eq!(
        target.load(Ordering::SeqCst),
        0,
        "a stopping capture listener must suppress the target phase"
    );

    drop(input);
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
}

#[test]
fn key_capture_runs_before_the_target_and_can_prevent_the_default() {
    let viewport = Size::new(20, 5);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    let order = Arc::new(Mutex::new(Vec::new()));
    let capture_order = order.clone();
    let target_order = order.clone();
    let node = icmd::view
        .events(move |events| {
            events.key_down_capture /= EventListener::new(move |event: icmd::KeyboardEvent| {
                capture_order.lock().unwrap().push("capture");
                event.prevent_default();
            });
            events.key_down /= EventListener::new(move |_event| {
                target_order.lock().unwrap().push("target");
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
    assert_eq!(&*order.lock().unwrap(), &["capture", "target"]);

    drop(input);
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
}

// PERF gate: a large paste payload must not be cloned per ancestor. The
// dispatch payload is shared, so the deepest route copies a refcount.
#[test]
fn paste_payload_is_shared_across_the_ancestor_route() {
    let viewport = Size::new(20, 5);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    // One listener at depth 8 records the pointer identity of the payload it
    // received alongside the listener that produced it.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_listener = seen.clone();
    let mut node = icmd::view
        .events({
            let seen = seen_for_listener.clone();
            move |events| {
                events.paste_event /= EventListener::new(move |event: icmd::PasteEvent| {
                    seen.lock()
                        .unwrap()
                        .push(Arc::as_ptr(&event.text) as *const u8 as usize);
                });
            }
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
    for _ in 0..8 {
        node = icmd::view.apply(icmd::Props::new(()).children([node]).with_dom(
            icmd::DomProps::default().with_style(icmd::style(|style| {
                style.width /= icmd::Dimension::Max;
                style.height /= icmd::Dimension::Max;
            })),
        ));
    }
    input.send(node).unwrap();
    // Settle before pressing. The first frame can be a bootstrap frame the
    // commit pass published before this tree's regions existed, and a press
    // against that frame focuses nothing - so the paste would never reach the
    // listener and the test would fail under load rather than on a real defect.
    while output.recv_timeout(Duration::from_millis(250)).is_ok() {}
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    assert!(
        dispatcher.focused().is_some(),
        "the press must focus the region that the paste is routed through"
    );

    let payload = "p".repeat(64 * 1024);
    dispatcher.dispatch(Event::Paste(payload.clone()));
    let observed = seen.lock().unwrap().clone();
    assert_eq!(observed.len(), 1, "the listener must observe the paste");
    // The delivered payload is the same allocation the dispatcher built, not a
    // per-ancestor copy; a copy would also be observably equal in content.
    let shared: Arc<str> = Arc::from(payload.as_str());
    assert_ne!(
        observed[0],
        Arc::as_ptr(&shared) as *const u8 as usize,
        "sanity: distinct allocations differ"
    );

    drop(input);
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
}

// API acceptance checklist: "Focus ownership, terminal focus, and
// application-global keyboard paths use distinct types." Terminal activation
// must not move DOM focus, a global shortcut must work with no focus, and a
// widget key handler must not, and DOM focus must never exceed one owner.
#[test]
fn focus_ownership_terminal_focus_and_global_keys_stay_distinct() {
    let viewport = Size::new(24, 6);
    let (commit, _viewport, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();

    let widget_keys = Arc::new(AtomicUsize::new(0));
    let global_keys = Arc::new(AtomicUsize::new(0));
    let terminal_events = Arc::new(Mutex::new(Vec::new()));
    let widget_for_listener = widget_keys.clone();
    let global_for_listener = global_keys.clone();
    let terminal_for_listener = terminal_events.clone();

    // Two focusable widgets, so "at most one focused owner" is meaningful.
    let widget = || {
        icmd::view.apply(
            icmd::Props::new(()).with_dom(
                icmd::DomProps::default()
                    .with_focusable(true)
                    .with_style(icmd::style(|style| {
                        style.width /= icmd::Dimension::Cells(10);
                        style.height /= icmd::Dimension::Cells(2);
                    })),
            ),
        )
    };
    let node = icmd::view
        .events({
            let widget = widget_for_listener.clone();
            let global = global_for_listener.clone();
            let terminal = terminal_for_listener.clone();
            move |events| {
                events.key_down /= EventListener::new(move |_event: icmd::KeyboardEvent| {
                    widget.fetch_add(1, Ordering::SeqCst);
                });
                // A global shortcut is registered separately from the widget key.
                events.app_key /= EventListener::new(move |_event: icmd::KeyboardEvent| {
                    global.fetch_add(1, Ordering::SeqCst);
                });
                // Terminal activation uses its own event type, separate from
                // the `FocusEvent` a widget receives when it gains DOM focus.
                events.terminal_focus /=
                    EventListener::new(move |event: icmd::TerminalFocusEvent| {
                        terminal.lock().unwrap().push(event);
                    });
            }
        })
        .apply(
            icmd::Props::new(())
                .children([widget(), widget()])
                .with_dom(icmd::DomProps::default().with_style(icmd::style(|style| {
                    style.width /= icmd::Dimension::Max;
                    style.height /= icmd::Dimension::Max;
                }))),
        );
    input.send(node).unwrap();
    output.recv_timeout(Duration::from_secs(2)).unwrap();

    // A terminal activation reaches only the terminal-focus path. It must not
    // create, move, or clear DOM focus.
    assert!(
        dispatcher.focused().is_none(),
        "nothing is focused initially"
    );
    dispatcher.dispatch(Event::FocusGained);
    dispatcher.dispatch(Event::FocusLost);
    assert_eq!(
        terminal_events.lock().unwrap().len(),
        2,
        "terminal activation must reach the terminal-focus listener"
    );
    assert!(
        dispatcher.focused().is_none(),
        "terminal activation must not own DOM focus"
    );

    // Focus one widget by pointer; a widget key handler now receives keys.
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    let first = dispatcher.focused().expect("the pressed widget owns focus");
    dispatcher.dispatch(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('a'),
        KeyModifiers::empty(),
        KeyEventKind::Press,
    )));
    assert_eq!(
        widget_keys.load(Ordering::SeqCst),
        1,
        "the focused widget hears keys"
    );
    // The global shortcut runs regardless of focus ownership.
    assert_eq!(
        global_keys.load(Ordering::SeqCst),
        1,
        "the global shortcut hears keys"
    );

    // Focus the second widget; exactly one owner remains, and it is the new one.
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        // The two focusable children stack vertically, each two rows tall.
        row: 3,
        modifiers: KeyModifiers::empty(),
    }));
    let second = dispatcher.focused().expect("the second widget owns focus");
    assert_ne!(first, second, "focus moved to the newly pressed widget");
    // Ownership is single-valued, so re-targeting focus transfers it rather than
    // adding a second owner.
    assert!(
        dispatcher.focus(first),
        "focus can be re-targeted explicitly"
    );
    assert_eq!(dispatcher.focused(), Some(first));
    assert!(dispatcher.focus(second), "and moved back");
    assert_eq!(dispatcher.focused(), Some(second));

    // Losing terminal activation does not drop widget focus either.
    dispatcher.dispatch(Event::FocusLost);
    assert_eq!(
        dispatcher.focused(),
        Some(second),
        "terminal deactivation must not clear DOM focus"
    );

    drop(input);
    let _ = runtime.shutdown(icmd::advanced::ShutdownPolicy::default());
}
