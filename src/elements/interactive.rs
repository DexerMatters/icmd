//! Shared activation mechanics for the interactive controls: focus policy,
//! disabled gate, and the mapping from physical input to one semantic
//! activation.
use crossterm::event::{KeyCode, KeyModifiers};

use crate::{
    Attr, DomProps, EventListener, KeyboardEvent, Node, PointerEvent,
    basic::events::PointerEventKind, ui, view,
};

/// Private activation state a control shares: its disabled gate, autofocus
/// request, and the optional callback fired once per activation. What
/// activation means stays the control's own policy.
pub(crate) struct Activation {
    /// Blocks activation and clears focusability when set.
    pub(crate) disabled: bool,
    /// Requests focus on mount when the control is enabled.
    pub(crate) autofocus: bool,
    /// Fired on each accepted activation.
    pub(crate) on_activate: Option<Box<dyn FnMut() + Send + 'static>>,
}

impl Activation {
    /// Creates an activation with no callback.
    pub(crate) fn new(disabled: bool, autofocus: bool) -> Self {
        Self {
            disabled,
            autofocus,
            on_activate: None,
        }
    }

    /// Installs the callback fired on each accepted activation.
    pub(crate) fn on_activate(mut self, callback: impl FnMut() + Send + 'static) -> Self {
        self.on_activate = Some(Box::new(callback));
        self
    }
}

/// Builds the interactive host node: merges the control's defaults with the
/// caller's DOM props exactly once, derives focusability and autofocus from
/// `activation`, and composes the caller's click and key listeners rather than
/// replacing them. The listeners live on this same node, so it is also the
/// painted and hit-tested node.
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

    let caller_click = dom.events.click.as_ref().cloned();
    let caller_key = dom.events.key_down.as_ref().cloned();

    let click = {
        let callback = callback.clone();
        EventListener::compose(
            move |event: PointerEvent| {
                if disabled {
                    return;
                }
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

    dom.events.click = Attr::Set(click);
    dom.events.key_down = Attr::Set(key);

    let children = children.into_iter().collect::<Node>();
    ui! {
        <view dom={dom}>{children}</view>
    }
}

/// Shared activation callback: boxed so a control can own an arbitrary handler
/// while remaining `Send`.
type SharedActivate = std::sync::Arc<std::sync::Mutex<Option<Box<dyn FnMut() + Send>>>>;

/// Invokes the installed callback once, if any.
fn fire(callback: &SharedActivate) {
    if let Ok(mut guard) = callback.lock()
        && let Some(callback) = guard.as_mut()
    {
        callback();
    }
}
