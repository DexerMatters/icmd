//! The supported `icmd::testing` harness: painting, gestures, frames, resize,
//! and frame-clock ticking without a terminal.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crossterm::event::KeyCode;
use icmd::testing::Harness;
use icmd::{
    Attr, Component, ComponentContext, Dimension, DomProps, EventListener, Layout, Node, Props,
    Size, StateRef, text,
};

#[derive(Clone, Default)]
struct HitCtx {
    slot: Arc<Mutex<Option<StateRef<bool>>>>,
}

fn hit_target(cx: &mut ComponentContext, props: &Props<HitCtx>) -> Node {
    let hit = cx.use_live_state(|| false);
    *props.data().slot.lock().unwrap() = Some(hit.clone());
    let hit_for_listener = hit.clone();
    let label = if hit.read(|value| *value).unwrap_or(false) {
        "hit"
    } else {
        "idle"
    };
    let mut dom = DomProps::default();
    dom.style.layout = Attr::Set(Layout::Vertical);
    dom.style.width = Attr::Set(Dimension::Max);
    dom.style.height = Attr::Set(Dimension::Max);
    dom.events.pointer_down = Attr::Set(EventListener::new(move |_event: icmd::PointerEvent| {
        let _ = hit_for_listener.set(true);
    }));
    Node::element(dom, [text(label)])
}

#[test]
fn harness_mounts_paints_and_dispatches_gestures() {
    let mut harness = Harness::new(Size::new(20, 4));
    let ctx = HitCtx::default();
    let node = hit_target.apply(ctx.clone());
    harness.mount(node);
    assert!(harness.text().contains("idle"));
    harness.click(0, 1);
    assert!(
        harness.wait_for(Duration::from_secs(1), |screen| screen
            .text()
            .contains("hit")),
        "a click must reach the painted target"
    );
    assert!(harness.frames() >= 2);
    assert_eq!(harness.screen().width(), 20);
}

#[test]
fn harness_press_and_resize_are_observable() {
    #[derive(Clone, Default)]
    struct KeyCtx {
        seen: Arc<std::sync::atomic::AtomicUsize>,
    }

    fn keys(_cx: &mut ComponentContext, props: &Props<KeyCtx>) -> Node {
        let seen = props.data().seen.clone();
        let mut dom = DomProps::default();
        dom.style.width = Attr::Set(Dimension::Max);
        dom.style.height = Attr::Set(Dimension::Max);
        dom.events.app_key = Attr::Set(EventListener::new(move |_event: icmd::KeyboardEvent| {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        Node::element(dom, [text("keys")])
    }

    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut harness = Harness::new(Size::new(10, 2));
    harness.mount(keys.apply(KeyCtx { seen: seen.clone() }));
    harness.press(KeyCode::Char('x'), crossterm::event::KeyModifiers::empty());
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    harness.resize(Size::new(12, 3));
    assert_eq!(harness.screen().width(), 12);
    assert_eq!(harness.screen().height(), 3);
}

#[test]
fn harness_tick_drives_frame_clocks() {
    #[derive(Clone, Default)]
    struct ClockCtx {
        ticks: Arc<std::sync::atomic::AtomicU64>,
    }

    fn clocked(cx: &mut ComponentContext, props: &Props<ClockCtx>) -> Node {
        let ticks = props.data().ticks.clone();
        let counter = ticks.clone();
        let _clock = cx.use_frame_clock(move |_dt| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let count = ticks.load(std::sync::atomic::Ordering::SeqCst);
        text(format!("tick{count}"))
    }

    let ticks = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut harness = Harness::new(Size::new(12, 2));
    harness.mount(clocked.apply(ClockCtx {
        ticks: ticks.clone(),
    }));
    assert!(harness.text().contains("tick0"));
    harness.tick(Duration::from_millis(50));
    assert_eq!(ticks.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        harness.wait_for(Duration::from_secs(1), |screen| screen
            .text()
            .contains("tick1")),
        "a frame clock tick must repaint its component: {:?}",
        harness.text()
    );
}

#[test]
fn components_see_the_resized_viewport() {
    fn sized(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
        let viewport = cx.use_viewport();
        text(format!("{}x{}", viewport.width, viewport.height))
    }

    let mut harness = Harness::new(Size::new(10, 2));
    harness.mount(sized.apply(()));
    assert!(harness.text().contains("10x2"));
    harness.resize(Size::new(14, 4));
    assert!(
        harness.text().contains("14x4"),
        "a resize rerenders components that read use_viewport: {:?}",
        harness.text()
    );
}
