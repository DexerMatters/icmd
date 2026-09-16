// Component lifecycle hook tests: mount effects, unmount cleanups, dependency
// re-runs, and the React-faithful ordering contract. These drive the real
// `Lower -> Commit -> Renderer` pipeline, so the log is complete once the
// runtime has been shut down and every worker joined.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use icmd::advanced::{Commit, Lower, Renderer, Runtime, ShutdownPolicy};
use icmd::{AppHandle, Component, ComponentContext, Node, Props, Size, StateSetter, text};

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<&'static str>>>);

impl Log {
    fn push(&self, entry: &'static str) {
        self.0.lock().expect("log poisoned").push(entry);
    }

    fn entries(&self) -> Vec<&'static str> {
        self.0.lock().expect("log poisoned").clone()
    }
}

type StageOutput = Result<String, icmd::advanced::FrameError>;
type Handle = icmd::advanced::RuntimeHandle<Node, StageOutput>;

fn start(node: Node) -> (Sender<Node>, Receiver<StageOutput>, Handle) {
    start_with(Lower::default(), node)
}

fn start_with(lower: Lower, node: Node) -> (Sender<Node>, Receiver<StageOutput>, Handle) {
    let viewport = Size::new(20, 4);
    let (commit, _) = Commit::new(viewport);
    let renderer = Renderer::new(viewport).expect("renderer");
    let runtime = Runtime::new(lower)
        .then(commit)
        .then(renderer)
        .start_handle();
    let input = runtime.input();
    let output = runtime.output();
    input.send(node).expect("the root is accepted");
    (input, output, runtime)
}

// One presented frame is also a synchronization point: effects are committed
// before the lowerer hands the frame to the next stage.
fn next_frame(output: &Receiver<StageOutput>) {
    output
        .recv_timeout(Duration::from_secs(2))
        .expect("a frame arrives")
        .expect("the renderer succeeds");
}

// Close the root, drain the unwind frames, and join every worker. The tree is
// unmounted during the join, so the log is complete when this returns.
fn unmount(input: Sender<Node>, output: Receiver<StageOutput>, mut runtime: Handle) {
    drop(input);
    runtime.close_input();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match output.recv_timeout(Duration::from_millis(20)) {
            Ok(_) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    break;
                }
            }
        }
    }
    runtime
        .shutdown(ShutdownPolicy::with_timeout(Duration::from_secs(2)))
        .expect("shutdown joins every worker");
}

fn run_until_unmounted(node: Node) {
    let (input, output, runtime) = start(node);
    next_frame(&output);
    unmount(input, output, runtime);
}

#[derive(Clone)]
struct TagProps {
    tag: &'static str,
    log: Log,
}

fn mount_tagged(cx: &mut ComponentContext, props: &Props<TagProps>) -> Node {
    let tag = props.data().tag;
    let log = props.data().log.clone();
    cx.use_mount_effect(move || log.push(tag));
    text("x")
}

fn unmount_tagged(cx: &mut ComponentContext, props: &Props<TagProps>) -> Node {
    let tag = props.data().tag;
    let log = props.data().log.clone();
    cx.use_unmount(move || log.push(tag));
    text("x")
}

#[derive(Clone)]
struct TreeProps {
    log: Log,
}

// Registers its effect before it renders its child, so the flush order is what
// decides which runs first.
fn mount_tree(cx: &mut ComponentContext, props: &Props<TreeProps>) -> Node {
    let log = props.data().log.clone();
    cx.use_mount_effect({
        let log = log.clone();
        move || log.push("parent")
    });
    mount_tagged.apply(Props::new(TagProps { tag: "child", log }))
}

fn unmount_tree(cx: &mut ComponentContext, props: &Props<TreeProps>) -> Node {
    let log = props.data().log.clone();
    cx.use_unmount({
        let log = log.clone();
        move || log.push("parent")
    });
    unmount_tagged.apply(Props::new(TagProps { tag: "child", log }))
}

fn two_effects(cx: &mut ComponentContext, props: &Props<TreeProps>) -> Node {
    let log = props.data().log.clone();
    cx.use_mount_effect({
        let log = log.clone();
        move || log.push("first")
    });
    cx.use_mount_effect({
        let log = log.clone();
        move || log.push("second")
    });
    text("x")
}

// `use_mount_effect` is the `useEffect(fn, [])` equivalent: the body runs once,
// and the value it returns runs on unmount.
#[test]
fn a_mount_effect_runs_once_and_cleans_up_on_unmount() {
    let log = Log::default();
    let hook_log = log.clone();
    let app = move |cx: &mut ComponentContext, _props: &Props<()>| {
        cx.use_mount_effect({
            let log = hook_log.clone();
            move || {
                log.push("mount");
                let log = log.clone();
                move || log.push("cleanup")
            }
        });
        text("x")
    };
    run_until_unmounted(app.apply(()));
    assert_eq!(log.entries(), vec!["mount", "cleanup"]);
}

// `use_unmount` registers a cleanup without a mount body. A total count of one
// proves it did not also run as a mount effect.
#[test]
fn an_unmount_hook_does_not_run_at_mount_and_runs_once_on_unmount() {
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = calls.clone();
    let app = move |cx: &mut ComponentContext, _props: &Props<()>| {
        let calls = hook_calls.clone();
        cx.use_unmount(move || {
            calls.fetch_add(1, Ordering::Relaxed);
        });
        text("x")
    };
    run_until_unmounted(app.apply(()));
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "the cleanup must run exactly once, at unmount"
    );
}

// A dependency change runs that effect's previous cleanup before the new body;
// an unchanged dependency does not run the effect at all.
#[test]
fn an_effect_reruns_only_when_its_dependencies_change_and_cleans_up_first() {
    #[derive(Clone, Default)]
    struct Slot(Arc<Mutex<Option<StateSetter<usize>>>>);

    let log = Log::default();
    let hook_log = log.clone();
    let slot = Slot::default();
    let hook_slot = slot.clone();
    let app = move |cx: &mut ComponentContext, _props: &Props<()>| {
        let (value, set) = cx.use_state(|| 0usize);
        *hook_slot.0.lock().expect("slot poisoned") = Some(set);
        // The renderer drops a frame identical to the previous one, so a
        // per-render counter keeps every update observable. That makes the
        // frame a synchronization point for the effect flush.
        let renders = cx.use_ref(|| 0u32);
        let render = {
            let mut guard = renders.lock().expect("render counter poisoned");
            *guard += 1;
            *guard
        };
        cx.use_effect(value, {
            let log = hook_log.clone();
            move || {
                log.push("body");
                move || log.push("cleanup")
            }
        });
        text(format!("{value}:{render}"))
    };

    let (input, output, runtime) = start(app.apply(()));
    next_frame(&output);
    assert_eq!(
        log.entries(),
        vec!["body"],
        "the mount body runs and its cleanup does not"
    );

    let setter = slot
        .0
        .lock()
        .expect("slot poisoned")
        .clone()
        .expect("the setter is published");

    setter.set(1);
    next_frame(&output);
    assert_eq!(
        log.entries(),
        vec!["body", "cleanup", "body"],
        "a changed dependency cleans up before rerunning"
    );

    // The same value is not a change: a frame is still produced (the render
    // counter differs), but the effect does not run.
    setter.set(1);
    next_frame(&output);
    assert_eq!(log.entries(), vec!["body", "cleanup", "body"]);

    unmount(input, output, runtime);
    assert_eq!(
        log.entries(),
        vec!["body", "cleanup", "body", "cleanup"],
        "unmount cleans up the live effect"
    );
}

// React flushes mount effects bottom-up: the inner component's effect is
// observable before the outer one that composed it.
#[test]
fn mount_effects_run_child_before_parent() {
    let log = Log::default();
    run_until_unmounted(mount_tree.apply(Props::new(TreeProps { log: log.clone() })));
    assert_eq!(log.entries(), vec!["child", "parent"]);
}

// Unmount is the reverse traversal: an outer component's cleanup runs before the
// cleanup of the inner component it composed.
#[test]
fn unmount_cleanups_run_parent_before_child() {
    let log = Log::default();
    run_until_unmounted(unmount_tree.apply(Props::new(TreeProps { log: log.clone() })));
    assert_eq!(log.entries(), vec!["parent", "child"]);
}

// Within one component, hooks run in declaration order.
#[test]
fn effects_run_in_declaration_order_within_a_component() {
    let log = Log::default();
    run_until_unmounted(two_effects.apply(Props::new(TreeProps { log: log.clone() })));
    assert_eq!(log.entries(), vec!["first", "second"]);
}

// The handle a component obtains with `use_handle` is the session state the
// installed handle wraps, so a request made from a component reaches whatever
// loop is watching that handle. This is the plumbing behind "a button press
// exits the application": `on_press` receives a clone of this handle.
#[test]
fn a_component_handle_shares_state_with_the_installed_handle() {
    let handle = AppHandle::default();
    let slot = Arc::new(Mutex::new(None::<AppHandle>));
    let hook_slot = slot.clone();
    let app = move |cx: &mut ComponentContext, _props: &Props<()>| {
        *hook_slot.lock().expect("slot poisoned") = Some(cx.use_handle());
        text("x")
    };

    let (input, output, runtime) =
        start_with(Lower::default().with_handle(handle.clone()), app.apply(()));
    next_frame(&output);

    let from_component = slot
        .lock()
        .expect("slot poisoned")
        .clone()
        .expect("the component published its handle");
    assert!(!handle.exit_requested());
    from_component.request_exit();
    assert!(
        handle.exit_requested(),
        "a request through the component's handle reaches the installed handle"
    );

    unmount(input, output, runtime);
}
