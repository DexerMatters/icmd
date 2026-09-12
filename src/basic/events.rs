use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use crossterm::event::{KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::{ScreenPosition, Size};

use super::{
    common::Attr,
    props::{ScrollDelta, ScrollOffset},
};

type ListenerCallback<E> = Box<dyn FnMut(E) + Send + 'static>;

#[derive(fmt_derive::Debug)]
pub struct EventListener<E> {
    pub(crate) callback: Arc<Mutex<ListenerCallback<E>>>,
}

impl<E> EventListener<E> {
    pub fn new(callback: impl FnMut(E) + Send + 'static) -> Self {
        Self {
            callback: Arc::new(Mutex::new(Box::new(callback))),
        }
    }

    pub(crate) fn call(&self, event: E) {
        let mut callback = self.callback.lock().expect("event listener poisoned");
        callback(event);
    }
}

impl<E> EventListener<E> {
    /// Compose a component-internal listener with a caller-supplied observer.
    ///
    /// The internal behavior runs first so it can reduce state before the
    /// caller observes the same event. The caller's observer always runs, even
    /// when the internal handler stopped propagation: `stop_propagation`
    /// controls ancestor delivery, not the other observers on this host.
    pub fn compose(
        internal: impl FnMut(E) + Send + 'static,
        caller: Option<EventListener<E>>,
    ) -> EventListener<E>
    where
        E: Clone + Send + 'static,
    {
        let mut internal = internal;
        EventListener::new(move |event: E| {
            internal(event.clone());
            if let Some(caller) = &caller {
                caller.call(event);
            }
        })
    }
}

impl<E> Clone for EventListener<E> {
    fn clone(&self) -> Self {
        Self {
            callback: self.callback.clone(),
        }
    }
}

impl<E> PartialEq for EventListener<E> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.callback, &other.callback)
    }
}

impl<E> Eq for EventListener<E> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusEvent {
    Gained,
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PointerId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerType {
    #[default]
    Mouse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerButton {
    #[default]
    None,
    Primary,
    Auxiliary,
    Secondary,
}

impl PointerButton {
    pub(crate) const fn bit(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Primary => 1,
            Self::Auxiliary => 4,
            Self::Secondary => 2,
        }
    }

    pub(crate) const fn from_crossterm(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => Self::Primary,
            MouseButton::Middle => Self::Auxiliary,
            MouseButton::Right => Self::Secondary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerEventKind {
    Down,
    Up,
    Move,
    Cancel,
    Over,
    Out,
    Enter,
    Leave,
    GotCapture,
    LostCapture,
    Click,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerEvent {
    pub pointer_id: PointerId,
    pub pointer_type: PointerType,
    pub kind: PointerEventKind,
    pub position: ScreenPosition,
    /// Position relative to the event target's visible rectangle.
    pub local_position: ScreenPosition,
    pub button: PointerButton,
    pub buttons: u16,
    pub modifiers: KeyModifiers,
}

impl PointerEvent {
    pub(crate) const fn new(
        kind: PointerEventKind,
        position: ScreenPosition,
        button: PointerButton,
        buttons: u16,
        modifiers: KeyModifiers,
    ) -> Self {
        Self {
            pointer_id: PointerId(0),
            pointer_type: PointerType::Mouse,
            kind,
            position,
            local_position: position,
            button,
            buttons,
            modifiers,
        }
    }

    pub(crate) fn with_local_position(mut self, local_position: ScreenPosition) -> Self {
        self.local_position = local_position;
        self
    }

    pub fn is_primary(&self) -> bool {
        // Crossterm exposes one mouse pointer, so it is always the primary
        // pointer in the browser sense (independent of which button changed).
        self.pointer_id == PointerId(0)
    }

    pub fn is_primary_button(&self) -> bool {
        self.button == PointerButton::Primary
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelEvent {
    pub position: ScreenPosition,
    pub delta_x: i16,
    pub delta_y: i16,
    pub modifiers: KeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollEvent {
    pub offset: ScrollOffset,
    pub max_offset: ScrollOffset,
    pub delta: ScrollDelta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyboardEvent {
    pub key: KeyEvent,
}

thread_local! {
    static KEYBOARD_PROPAGATION: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
}

impl KeyboardEvent {
    /// Prevent this keyboard event from reaching ancestor keyboard listeners.
    ///
    /// The runtime checks this flag between focused-target and ancestor
    /// listeners. It is deliberately scoped to the current dispatch, so
    /// nested event dispatches do not consume their parent event.
    pub fn stop_propagation(&self) {
        KEYBOARD_PROPAGATION.with(|stack| {
            if let Some(stopped) = stack.borrow_mut().last_mut() {
                *stopped = true;
            }
        });
    }

    pub(crate) fn begin_dispatch() {
        KEYBOARD_PROPAGATION.with(|stack| stack.borrow_mut().push(false));
    }

    pub(crate) fn propagation_stopped() -> bool {
        KEYBOARD_PROPAGATION.with(|stack| stack.borrow().last().copied().unwrap_or(false))
    }

    pub(crate) fn end_dispatch() {
        KEYBOARD_PROPAGATION.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeEvent {
    pub size: Size,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PasteEvent {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventHandlers {
    pub pointer_down: Attr<EventListener<PointerEvent>>,
    pub pointer_up: Attr<EventListener<PointerEvent>>,
    pub pointer_move: Attr<EventListener<PointerEvent>>,
    pub pointer_cancel: Attr<EventListener<PointerEvent>>,
    pub pointer_over: Attr<EventListener<PointerEvent>>,
    pub pointer_out: Attr<EventListener<PointerEvent>>,
    pub pointer_enter: Attr<EventListener<PointerEvent>>,
    pub pointer_leave: Attr<EventListener<PointerEvent>>,
    pub got_pointer_capture: Attr<EventListener<PointerEvent>>,
    pub lost_pointer_capture: Attr<EventListener<PointerEvent>>,
    pub click: Attr<EventListener<PointerEvent>>,
    pub wheel: Attr<EventListener<WheelEvent>>,
    pub scroll: Attr<EventListener<ScrollEvent>>,
    pub key_down: Attr<EventListener<KeyboardEvent>>,
    pub key_up: Attr<EventListener<KeyboardEvent>>,
    pub keyboard_event: Attr<EventListener<KeyboardEvent>>,
    pub resize_event: Attr<EventListener<ResizeEvent>>,
    pub focus_event: Attr<EventListener<FocusEvent>>,
    pub paste_event: Attr<EventListener<PasteEvent>>,
}

impl EventHandlers {
    /// Merge caller-provided handlers over component defaults.
    pub fn merge(&mut self, overrides: &Self) {
        macro_rules! merge {
            ($($field:ident),+ $(,)?) => {
                $(self.$field.overlay(&overrides.$field);)+
            };
        }
        merge!(
            pointer_down,
            pointer_up,
            pointer_move,
            pointer_cancel,
            pointer_over,
            pointer_out,
            pointer_enter,
            pointer_leave,
            got_pointer_capture,
            lost_pointer_capture,
            click,
            wheel,
            scroll,
            key_down,
            key_up,
            keyboard_event,
            resize_event,
            focus_event,
            paste_event,
        );
    }
}

pub(crate) fn mouse_details(event: MouseEvent) -> (PointerEventKind, PointerButton) {
    match event.kind {
        MouseEventKind::Down(button) => (
            PointerEventKind::Down,
            PointerButton::from_crossterm(button),
        ),
        MouseEventKind::Up(button) => (PointerEventKind::Up, PointerButton::from_crossterm(button)),
        MouseEventKind::Drag(button) => (
            PointerEventKind::Move,
            PointerButton::from_crossterm(button),
        ),
        MouseEventKind::Moved => (PointerEventKind::Move, PointerButton::None),
        MouseEventKind::ScrollDown
        | MouseEventKind::ScrollUp
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => (PointerEventKind::Move, PointerButton::None),
    }
}
