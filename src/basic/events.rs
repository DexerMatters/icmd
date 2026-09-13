use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
};

use crossterm::event::{KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::{ScreenPosition, Size};

use super::{
    common::Attr,
    props::{ScrollDelta, ScrollOffset},
};

type ListenerCallback<E> = Box<dyn FnMut(E) + Send + 'static>;

// A callback that panics must not leave the runtime running in an unknown
// partially-mutated state. The first fault is recorded and surfaced through the
// runtime error channel; later faults are ignored so the cause is preserved.
//
// The slot is per-thread: an application callback only ever runs on the thread
// that dispatches events, so per-thread state is both correct for the runtime
// and free of the cross-test leakage a process-global slot would create.
thread_local! {
    static CALLBACK_FAULT: RefCell<Option<CallbackFault>> = const { RefCell::new(None) };
}

// Reentrancy is detected by the listener that is being re-entered, while a
// panic is caught after unwinding; both funnel through this counter so a
// non-dispatching thread can still be observed.
static FAULT_OBSERVED: AtomicU8 = AtomicU8::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallbackFault {
    // A listener re-entered itself, which would deadlock a non-reentrant lock.
    // Delivery is rejected instead.
    Reentrant,
    Panicked,
}

impl CallbackFault {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Reentrant => "an event listener dispatched an event into itself",
            Self::Panicked => "an event listener panicked",
        }
    }
}

pub(crate) fn record_callback_fault(fault: CallbackFault) {
    CALLBACK_FAULT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(fault);
        }
    });
    FAULT_OBSERVED.fetch_add(1, Ordering::SeqCst);
}

pub(crate) fn take_callback_fault() -> Option<CallbackFault> {
    CALLBACK_FAULT.with(|slot| slot.borrow_mut().take())
}

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
        // `try_lock` (not `lock`) because a listener may dispatch an event that
        // routes back to itself. Blocking there would deadlock; rejecting the
        // nested delivery is deterministic and preserves the outer callback.
        let mut callback = match self.callback.try_lock() {
            Ok(callback) => callback,
            Err(std::sync::TryLockError::WouldBlock) => {
                record_callback_fault(CallbackFault::Reentrant);
                return;
            }
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                // A previous panic poisoned the lock. Recover the callback so
                // subsequent independent events still work, and record the
                // panic once.
                record_callback_fault(CallbackFault::Panicked);
                poisoned.into_inner()
            }
        };
        if catch_unwind(AssertUnwindSafe(|| callback(event))).is_err() {
            record_callback_fault(CallbackFault::Panicked);
        }
    }
}

impl<E> EventListener<E> {
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

// Terminal activation is a different state machine from DOM focus ownership:
// it says the operating-system window has or has not got input focus, not which
// widget owns focus. Keeping it a distinct type makes it impossible to route
// terminal activation into widget focus callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminalFocusEvent {
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
    pub fn stop_propagation(&self) {
        KEYBOARD_PROPAGATION.with(|stack| {
            if let Some(stopped) = stack.borrow_mut().last_mut() {
                *stopped = true;
            }
        });
    }

    pub(crate) fn propagation_stopped() -> bool {
        KEYBOARD_PROPAGATION.with(|stack| stack.borrow().last().copied().unwrap_or(false))
    }

    // The guard owns the thread-local propagation frame. Because restoration is
    // tied to `Drop`, an unwinding listener cannot leave stale "stopped" state
    // behind for the next, unrelated dispatch.
    pub(crate) fn begin_dispatch() -> PropagationGuard {
        KEYBOARD_PROPAGATION.with(|stack| stack.borrow_mut().push(false));
        PropagationGuard { _private: () }
    }
}

pub(crate) struct PropagationGuard {
    _private: (),
}

impl Drop for PropagationGuard {
    fn drop(&mut self) {
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
    // Targeted, bubbling keyboard hook. It only ever receives a key when this
    // region is on the focused target's route.
    pub keyboard_event: Attr<EventListener<KeyboardEvent>>,
    // Application-global keyboard hook. Unlike `keyboard_event` it is not a
    // focused target: it receives a key that no focused widget consumed, which
    // is what application shortcuts need.
    pub app_key: Attr<EventListener<KeyboardEvent>>,
    pub resize_event: Attr<EventListener<ResizeEvent>>,
    pub focus_event: Attr<EventListener<FocusEvent>>,
    pub terminal_focus: Attr<EventListener<TerminalFocusEvent>>,
    pub paste_event: Attr<EventListener<PasteEvent>>,
}

impl EventHandlers {
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
            app_key,
            resize_event,
            focus_event,
            terminal_focus,
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
