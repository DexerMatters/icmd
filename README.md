# icmd

`runtime::Lower` is a typed pipeline stage. It owns component fibers and hooks,
accepts a `Node` component, and emits a host-only `DomNode`; it does not emit
renderer operations and has no direct rendering API.

```rust
use std::sync::{Arc, Mutex};
use icmd::{
    Cell, DomNode, ElementComponent, ElementContext, Image, Lower, Node, Props,
    Runtime, ScreenPosition, StateSetter,
};

struct Counter;

impl ElementComponent for Counter {
    type Props = Arc<Mutex<Option<StateSetter<u32>>>>;

    fn render(cx: &mut ElementContext, props: &Props<Self::Props>) -> Node {
        let (count, set_count) = cx.use_state(|| 0);
        *props.user_defined.lock().unwrap() = Some(set_count);
        let row = count
            .to_string()
            .chars()
            .map(|c| Cell::plain(c.to_string()).unwrap())
            .collect();
        Node::image(Image::from_rows(vec![row]).unwrap())
    }
}

let setter = Arc::new(Mutex::new(None));
let (input, output) = Runtime::new(Lower::default()).start();
input.send(Node::component::<Counter>(
    Props::with_size(10, 1, setter.clone()).at(ScreenPosition::new(2, 4)),
)).unwrap();
let dom = output.recv().unwrap();
assert!(matches!(dom, DomNode::Element { .. }));

setter.lock().unwrap().as_ref().unwrap().set(1);
let updated_dom = output.recv().unwrap();
assert_eq!(dom.id(), updated_dom.id());
```

`Props<T>` has three non-overlapping parts:

- `dom`: properties retained on the lowered DOM element;
- `children`: declarative children which the component may place explicitly;
- `user_defined`: application-specific component data.

The lowerer supports keyed reconciliation, `use_state`, `use_ref`, `use_memo`,
dependency-aware `use_effect`, and batched cross-thread state setters. A
separate DOM commit layer can later translate `DomNode` changes into renderer
operations.

## Event listeners

`DomProps` can carry typed, thread-safe listeners for terminal events. Builders
on `Props<T>` configure the element's listener while preserving its children and
application data. For example, a component can attach a keyboard listener that
updates its state when the eventual DOM event dispatcher invokes it:

```rust
struct KeyboardCounter;

impl ElementComponent for KeyboardCounter {
    type Props = ();

    fn render(cx: &mut ElementContext, _props: &Props<Self::Props>) -> Node {
        let (_count, set_count) = cx.use_state(|| 0);
        let child = Node::empty(); // Render `_count` here in a real component.
        Node::component::<KeyboardSurface>(
            Props::new(())
                .on_keyboard_event(move |_| set_count.update(|value| *value += 1))
                .child(child),
        )
    }
}

struct KeyboardSurface;

impl ElementComponent for KeyboardSurface {
    type Props = ();

    fn render(_cx: &mut ElementContext, props: &Props<Self::Props>) -> Node {
        Node::fragment(props.children.clone())
    }
}
```

Listeners are retained by lowering but are not polled or dispatched by
`Lower`; those responsibilities belong to a later DOM commit/input layer.
