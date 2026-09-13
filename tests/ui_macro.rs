use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use icmd::advanced::{Commit, Lower, Renderer, Runtime};
#[allow(unused_imports)]
use icmd::{
    Attr, Component, ComponentContext, Node, Props, StateSetter, button, checkbox, column, heading,
    progress_bar, ui, view,
};

fn render_node(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    props.children.iter().cloned().collect()
}

#[test]
fn macro_builds_nested_tree() {
    let _value = "hello";
    let node = ui! {
        <column>
            <heading>{_value}</heading>
            <button key="save" events={|events| events.click /= |_| {}}>{"Go"}</button>
        </column>
    };
    let _ = node;
}

#[test]
fn macro_assigns_props_style_and_evaluates_values_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_value = calls.clone();
    let node = ui! {
        <progress_bar
            value={next_value(&calls_for_value)}
            max={4}
            show_percentage={false}
            style={|style| style.width /= icmd::Dimension::Cells(4)}
        />
    };
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let _ = node;
    let _checkbox = ui! { <checkbox checked disabled label="Accept" /> };
}

fn next_value(calls: &AtomicUsize) -> u64 {
    calls.fetch_add(1, Ordering::SeqCst);
    2
}

#[derive(Default)]
struct NonClone;

#[derive(Default)]
struct NonCloneProps {
    value: Attr<NonClone>,
}

fn non_clone_component(_cx: &mut ComponentContext, props: &Props<NonCloneProps>) -> Node {
    if props.value.is_set() {
        "set".into()
    } else {
        "unset".into()
    }
}

#[test]
fn macro_moves_non_clone_props_without_cloning() {
    let value = NonClone;
    let node = ui! { <non_clone_component value={value} /> };
    let _ = node;
}

#[test]
fn macro_event_attribute_is_dispatched() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_for_listener = hits.clone();
    let node = ui! {
        <button on_click={move |_| {
            hits_for_listener.fetch_add(1, Ordering::SeqCst);
        }}>
            "Go"
        </button>
    };
    let (commit, _, dispatcher) = icmd::advanced::Commit::new_with_events(icmd::Size::new(8, 2));
    let (input, output) = icmd::advanced::Runtime::new(icmd::advanced::Lower::default())
        .then(commit)
        .start();
    input.send(node).unwrap();
    output.recv().unwrap();

    let mouse = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        })
    };
    dispatcher.dispatch(mouse(MouseEventKind::Down(MouseButton::Left)));
    dispatcher.dispatch(mouse(MouseEventKind::Up(MouseButton::Left)));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[derive(Default)]
struct ItemProps {
    label: Attr<String>,
    capture: Attr<Arc<Mutex<Option<StateSetter<usize>>>>>,
}

fn item(cx: &mut ComponentContext, props: &Props<ItemProps>) -> Node {
    let (count, set_count) = cx.use_state(|| 0usize);
    if let Some(capture) = props.capture.as_ref() {
        *capture.lock().unwrap() = Some(set_count);
    }
    format!("{}:{count}", props.label.clone() | String::new()).into()
}

struct RootProps {
    a_capture: Arc<Mutex<Option<StateSetter<usize>>>>,
    b_capture: Arc<Mutex<Option<StateSetter<usize>>>>,
    order_capture: Arc<Mutex<Option<StateSetter<bool>>>>,
}

fn keyed_root(cx: &mut ComponentContext, props: &Props<RootProps>) -> Node {
    let (reversed, set_reversed) = cx.use_state(|| false);
    *props.order_capture.lock().unwrap() = Some(set_reversed);
    if reversed {
        ui! {
            <view>
                <>
                    <item key="b" label="b" capture={props.b_capture.clone()} />
                    <item key="a" label="a" capture={props.a_capture.clone()} />
                </>
            </view>
        }
    } else {
        ui! {
            <view>
                <>
                    <item key="a" label="a" capture={props.a_capture.clone()} />
                    <item key="b" label="b" capture={props.b_capture.clone()} />
                </>
            </view>
        }
    }
}

#[test]
fn macro_key_preserves_state_when_children_reorder() {
    let a_capture = Arc::new(Mutex::new(None));
    let b_capture = Arc::new(Mutex::new(None));
    let order_capture = Arc::new(Mutex::new(None));
    let root = keyed_root.apply(RootProps {
        a_capture: a_capture.clone(),
        b_capture: b_capture.clone(),
        order_capture: order_capture.clone(),
    });
    let size = icmd::Size::new(20, 2);
    let (commit, _) = Commit::new(size);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(size).unwrap())
        .start();
    input.send(root).unwrap();
    let _ = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();

    a_capture.lock().unwrap().as_ref().unwrap().set(7);
    let _ = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    order_capture.lock().unwrap().as_ref().unwrap().set(true);
    let reordered = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let debug = reordered;
    assert!(debug.contains('7'));
    assert!(debug.find('b').unwrap() < debug.find('a').unwrap());
}

#[test]
fn macro_builds_fragment_and_custom_component() {
    let component = render_node;
    let node = ui! { <><component>"a"</component><column /></> };
    let _ = node;
}

#[test]
fn macro_single_text_root_is_a_node() {
    let node: Node = ui! { "text" };
    let _ = node;
}
