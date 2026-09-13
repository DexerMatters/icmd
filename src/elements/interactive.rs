use crossterm::event::{KeyCode, KeyModifiers};

use crate::{
    Attr, DomProps, EventListener, KeyboardEvent, Node, PointerEvent,
    basic::events::PointerEventKind, ui, view,
};

// Shared private activation mechanics for the interactive controls. It owns the
// focus policy, the disabled gate, and the mapping from physical input
// (primary click, Space, Enter) to one semantic activation. Domain components
// keep their own policy: what activation means is still theirs to decide.
pub(crate) struct Activation {
    pub(crate) disabled: bool,
    pub(crate) autofocus: bool,
    pub(crate) on_activate: Option<Box<dyn FnMut() + Send + 'static>>,
}

impl Activation {
    pub(crate) fn new(disabled: bool, autofocus: bool) -> Self {
        Self {
            disabled,
            autofocus,
            on_activate: None,
        }
    }

    pub(crate) fn on_activate(mut self, callback: impl FnMut() + Send + 'static) -> Self {
        self.on_activate = Some(Box::new(callback));
        self
    }
}

// Builds the interactive host node. The control's own defaults are merged with
// the caller's DOM props exactly once, and focusability plus autofocus are
// derived from the activation policy. The listeners live on this same node, so
// it is also the painted and hit-tested node.
pub(crate) fn interactive(
    defaults: DomProps,
    caller: &DomProps,
    activation: Activation,
    children: Vec<Node>,
) -> Node {
    let disabled = activation.disabled;
    let mut dom = defaults.with_overrides(caller);
    dom.focusable = Attr::Set(!disabled);
    dom.autofocus = activation.autofocus && !disabled;

    let callback: SharedActivate =
        std::sync::Arc::new(std::sync::Mutex::new(activation.on_activate));

    // The caller's own listeners are composed rather than replaced, so an
    // application can observe the same click it made interactive.
    let caller_click = dom.events.click.as_ref().cloned();
    let caller_key = dom.events.key_down.as_ref().cloned();

    let click = {
        let callback = callback.clone();
        EventListener::compose(
            move |event: PointerEvent| {
                if disabled {
                    return;
                }
                // Only a primary-button click activates, once per press.
                if event.kind == PointerEventKind::Click && event.is_primary_button() {
                    fire(&callback);
                }
            },
            caller_click,
        )
    };
    let key = {
        let callback = callback.clone();
        EventListener::compose(
            move |event: KeyboardEvent| {
                if disabled {
                    return;
                }
                let activate = matches!(event.key.code, KeyCode::Enter | KeyCode::Char(' '))
                    && !event
                        .key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
                if activate {
                    event.stop_propagation();
                    fire(&callback);
                }
            },
            caller_key,
        )
    };

    // Assign the composed listeners directly. Rebuilding the node through the
    // macro would re-merge the caller props and double-apply their style.
    dom.events.click = Attr::Set(click);
    dom.events.key_down = Attr::Set(key);

    let children = children.into_iter().collect::<Node>();
    ui! {
        <view dom={dom}>{children}</view>
    }
}

// Shared activation callback: boxed so a control can own an arbitrary handler
// while remaining `Send`.
type SharedActivate = std::sync::Arc<std::sync::Mutex<Option<Box<dyn FnMut() + Send>>>>;

fn fire(callback: &SharedActivate) {
    if let Ok(mut guard) = callback.lock()
        && let Some(callback) = guard.as_mut()
    {
        callback();
    }
}
