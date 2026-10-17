//! Live shared state: `use_live_state` writes are visible immediately and wake
//! the owning component, while a detached `StateRef` and `update_quiet` stay
//! quiet.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use icmd::advanced::{Commit, FrameError, Lower, Renderer, Runtime, RuntimeHandle};
use icmd::{Component, ComponentContext, Node, Props, Size, StateRef, StateSetter, text};

type StageOutput = Result<String, FrameError>;

fn start(node: Node) -> (RuntimeHandle<Node, StageOutput>, Receiver<StageOutput>) {
    let viewport = Size::new(24, 3);
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).expect("renderer"))
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    input.send(node).expect("the root is accepted");
    (runtime, output)
}

fn next_frame(output: &Receiver<StageOutput>) -> String {
    output
        .recv_timeout(Duration::from_secs(2))
        .expect("a frame arrives")
        .expect("the renderer succeeds")
}

/// Collect frames until one contains `needle`, or the timeout expires.
fn frame_within(output: &Receiver<StageOutput>, needle: &str) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match output.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(frame)) => {
                if frame.contains(needle) {
                    return true;
                }
            }
            Ok(Err(error)) => panic!("renderer failed: {error}"),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
    false
}

#[derive(Clone)]
struct LiveProps {
    slot: Arc<Mutex<Option<StateRef<char>>>>,
}

fn live_component(cx: &mut ComponentContext, props: &Props<LiveProps>) -> Node {
    let live = cx.use_live_state(|| 'a');
    *props.data().slot.lock().expect("slot poisoned") = Some(live.clone());
    let value = live.read(|value| *value).unwrap_or('?');
    text(format!("value-{value}"))
}

#[test]
fn live_state_update_rerenders_without_a_redraw_tick() {
    let slot = Arc::new(Mutex::new(None));
    let node = live_component.apply(LiveProps { slot: slot.clone() });
    let (runtime, output) = start(node);
    let _first = next_frame(&output);
    let live = slot
        .lock()
        .unwrap()
        .clone()
        .expect("the hook publishes a handle");
    assert!(live.is_live(), "a hook-created StateRef is live");

    live.update(|value| *value = 'z')
        .expect("the value is writable");
    assert!(
        frame_within(&output, "z"),
        "a live update must wake the component without a caller redraw tick"
    );

    drop(runtime);
}

#[test]
fn update_quiet_does_not_wake_the_runtime() {
    let slot = Arc::new(Mutex::new(None));
    let node = live_component.apply(LiveProps { slot: slot.clone() });
    let (runtime, output) = start(node);
    let _first = next_frame(&output);
    let live = slot
        .lock()
        .unwrap()
        .clone()
        .expect("the hook publishes a handle");

    live.update_quiet(|value| *value = 'q')
        .expect("quiet update");
    match output.recv_timeout(Duration::from_millis(200)) {
        Err(RecvTimeoutError::Timeout) => {}
        Ok(Ok(frame)) => panic!("a quiet update must not paint: {frame:?}"),
        Ok(Err(error)) => panic!("renderer failed: {error}"),
        Err(RecvTimeoutError::Disconnected) => panic!("the pipeline stopped unexpectedly"),
    }

    live.update(|value| *value = 'm').expect("live update");
    assert!(
        frame_within(&output, "m"),
        "the next live update still reaches the screen"
    );

    drop(runtime);
}

#[test]
fn cloned_live_handles_share_value_and_wake_target() {
    let slot = Arc::new(Mutex::new(None));
    let node = live_component.apply(LiveProps { slot: slot.clone() });
    let (runtime, output) = start(node);
    let _first = next_frame(&output);
    let live = slot
        .lock()
        .unwrap()
        .clone()
        .expect("the hook publishes a handle");
    let clone = live.clone();
    clone.set('v').expect("the clone writes");
    assert_eq!(live.read(|value| *value).unwrap(), 'v');
    assert!(clone.is_live());
    assert!(frame_within(&output, "v"));
    drop(runtime);
}

#[test]
fn detached_state_ref_is_quiet_and_still_shares_values() {
    let state = StateRef::new(1_u32);
    assert!(!state.is_live());
    state.update(|value| *value += 1).expect("detached update");
    assert_eq!(state.read(|value| *value).unwrap(), 2);
    let shared = state.clone();
    shared.set(5).expect("detached set");
    assert_eq!(state.read(|value| *value).unwrap(), 5);
}

#[test]
fn a_state_setter_still_only_queues() {
    // The snapshot itself is from the current render; the setter is applied
    // before the next render. This test pins the documented distinction.
    #[derive(Clone, Default)]
    struct SetterProps {
        seen: Arc<Mutex<Vec<char>>>,
        setter: Arc<Mutex<Option<StateSetter<char>>>>,
    }

    fn component(cx: &mut ComponentContext, props: &Props<SetterProps>) -> Node {
        let (value, set) = cx.use_state(|| 'a');
        *props.data().setter.lock().unwrap() = Some(set);
        props.data().seen.lock().unwrap().push(value);
        text(format!("value-{value}"))
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    let setter = Arc::new(Mutex::new(None));
    let node = component.apply(SetterProps {
        seen: seen.clone(),
        setter: setter.clone(),
    });
    let (runtime, output) = start(node);
    let _first = next_frame(&output);
    let set = setter.lock().unwrap().clone().expect("setter");
    set.set('d');
    assert!(frame_within(&output, "d"));
    assert!(
        seen.lock().unwrap().contains(&'d'),
        "the next render observes the applied setter value"
    );
    drop(runtime);
}

#[test]
fn request_frame_rerenders_components_without_a_state_setter() {
    // A component can read a quiet Ref; a frame request must re-execute every
    // mounted component so such external values reach the screen.
    #[derive(Clone)]
    struct Ctx {
        value: Arc<Mutex<String>>,
    }

    fn component(cx: &mut ComponentContext, props: &Props<Ctx>) -> Node {
        let _ = cx;
        let value = props.value.lock().unwrap().clone();
        text(value)
    }

    let value = Arc::new(Mutex::new("old".to_string()));
    let node = component.apply(Ctx {
        value: value.clone(),
    });
    let viewport = Size::new(24, 3);
    let handle = icmd::AppHandle::default();
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::default().with_handle(handle.clone()))
        .then(commit)
        .then(Renderer::new(viewport).expect("renderer"))
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    input.send(node).expect("root accepted");
    let _first = next_frame(&output);

    *value.lock().unwrap() = "new".to_string();
    handle.request_frame();
    assert!(
        frame_within(&output, "new"),
        "request_frame must rerender and repaint"
    );
    drop(runtime);
}

#[test]
fn viewport_is_available_to_components() {
    #[derive(Clone, Default)]
    struct Ctx {
        seen: Arc<Mutex<Vec<Size>>>,
    }

    fn component(cx: &mut ComponentContext, props: &Props<Ctx>) -> Node {
        props.data().seen.lock().unwrap().push(cx.use_viewport());
        text("viewport")
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    let node = component.apply(Ctx { seen: seen.clone() });
    let viewport = Size::new(24, 3);
    let handle = icmd::AppHandle::default();
    let (commit, _) = Commit::new(viewport);
    let runtime = Runtime::new(Lower::default().with_handle(handle))
        .then(commit)
        .then(Renderer::new(viewport).expect("renderer"))
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    input.send(node).expect("root accepted");
    let _first = next_frame(&output);
    assert_eq!(
        seen.lock().unwrap().last().copied(),
        Some(Size::new(80, 24)),
        "a detached pipeline uses the documented default"
    );
    drop(runtime);
}
