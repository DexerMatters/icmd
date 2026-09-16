//! Event payloads, listener registration, and propagation control: pointer,
//! wheel, scroll, keyboard, resize, focus, and paste events, the per-region
//! [`EventHandlers`] table, and the stop/prevent controls shared by all of them.

use std::{
    cell::RefCell,
    fmt,
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

/// The erased, thread-safe callback stored behind a listener's mutex.
type ListenerCallback<E> = Box<dyn FnMut(E) + Send + 'static>;

thread_local! {
    static CALLBACK_FAULT: RefCell<Option<CallbackFault>> = const { RefCell::new(None) };
}

static FAULT_OBSERVED: AtomicU8 = AtomicU8::new(0);

/// How an event listener failed, recorded so the runtime can surface it.
///
/// A callback must not leave the runtime running in an unknown partially-mutated
/// state: the first fault is recorded and later faults are ignored so the cause
/// is preserved. The slot is per-thread because an application callback only
/// ever runs on the thread that dispatches events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallbackFault {
    /// A listener re-entered itself, which would deadlock a non-reentrant lock;
    /// the nested delivery is rejected instead.
    Reentrant,
    /// A listener panicked.
    Panicked,
}

impl CallbackFault {
    /// The message shown for this fault through the runtime error channel.
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Reentrant => "an event listener dispatched an event into itself",
            Self::Panicked => "an event listener panicked",
        }
    }
}

/// Record `fault` as the first fault on this thread, and count every recorded
/// fault so a non-dispatching thread can still observe that one occurred.
pub(crate) fn record_callback_fault(fault: CallbackFault) {
    CALLBACK_FAULT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(fault);
        }
    });
    FAULT_OBSERVED.fetch_add(1, Ordering::SeqCst);
}

/// Take the fault recorded on this thread, if any, clearing the slot.
pub(crate) fn take_callback_fault() -> Option<CallbackFault> {
    CALLBACK_FAULT.with(|slot| slot.borrow_mut().take())
}

/// A registered callback for events of type `E`.
///
/// Clone and comparison share the callback slot: clones fire together, and two
/// listeners are equal when they are the same registration.
pub struct EventListener<E> {
    pub(crate) callback: Arc<Mutex<ListenerCallback<E>>>,
}

/// Opaque by design: the callback closure is not inspectable, and printing an
/// address would make debug output unstable and leak implementation detail.
impl<E> fmt::Debug for EventListener<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventListener").finish_non_exhaustive()
    }
}

impl<E> EventListener<E> {
    /// Wrap `callback` as a listener; it runs on the thread that dispatches the
    /// event.
    pub fn new(callback: impl FnMut(E) + Send + 'static) -> Self {
        Self {
            callback: Arc::new(Mutex::new(Box::new(callback))),
        }
    }

    /// Deliver one event to the callback, recording a fault instead of
    /// panicking or blocking.
    ///
    /// Uses `try_lock` rather than `lock` because a listener may dispatch an
    /// event that routes back to itself, where blocking would deadlock; the
    /// nested delivery is rejected. A lock poisoned by an earlier panic is
    /// recovered so later independent events still run, with the panic recorded
    /// once.
    pub(crate) fn call(&self, event: E) {
        let mut callback = match self.callback.try_lock() {
            Ok(callback) => callback,
            Err(std::sync::TryLockError::WouldBlock) => {
                record_callback_fault(CallbackFault::Reentrant);
                return;
            }
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
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
    /// Build a listener that runs `internal` and then forwards a clone of the
    /// event to `caller`, when one is present.
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

/// Two listeners are equal when they share one callback slot.
impl<E> PartialEq for EventListener<E> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.callback, &other.callback)
    }
}

impl<E> Eq for EventListener<E> {}

/// DOM focus ownership change on the focused target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusEvent {
    /// The region gained DOM focus.
    Gained,
    /// The region lost DOM focus.
    Lost,
}

/// Terminal window activation, a different state machine from DOM focus
/// ownership.
///
/// It says the operating-system window has or has not got input focus, not
/// which widget owns focus. Keeping it a distinct type makes it impossible to
/// route terminal activation into widget focus callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminalFocusEvent {
    /// The terminal window gained input focus.
    Gained,
    /// The terminal window lost input focus.
    Lost,
}

/// Identifier of the pointer that produced a pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PointerId(
    /// Numeric pointer identifier; `0` is the primary pointer.
    pub u64,
);

/// Kind of pointing device that produced a pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerType {
    /// A mouse; the default, and the only pointer crossterm exposes.
    #[default]
    Mouse,
}

/// Mouse button associated with a pointer event. The numeric bit of each
/// variant matches its bit in [`PointerEvent::buttons`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerButton {
    /// No button changed state, as for a plain hover.
    #[default]
    None,
    /// Left button, mask bit 1.
    Primary,
    /// Middle button, mask bit 4.
    Auxiliary,
    /// Right button, mask bit 2.
    Secondary,
}

impl PointerButton {
    /// This button's bit in the pressed-button mask: `0` for
    /// [`PointerButton::None`].
    pub(crate) const fn bit(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Primary => 1,
            Self::Auxiliary => 4,
            Self::Secondary => 2,
        }
    }

    /// Map a crossterm mouse button to this type.
    pub(crate) const fn from_crossterm(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => Self::Primary,
            MouseButton::Middle => Self::Auxiliary,
            MouseButton::Right => Self::Secondary,
        }
    }
}

/// Kind of pointer interaction carried by a [`PointerEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerEventKind {
    /// A button was pressed.
    Down,
    /// A button was released.
    Up,
    /// The pointer moved, whether hovering or dragging.
    Move,
    /// The pointer interaction was cancelled.
    Cancel,
    /// The pointer entered the region's bounds; bubbles.
    Over,
    /// The pointer left the region's bounds; bubbles.
    Out,
    /// The pointer entered the region or one of its descendants; does not
    /// bubble.
    Enter,
    /// The pointer left the region and all descendants; does not bubble.
    Leave,
    /// The region received pointer capture.
    GotCapture,
    /// The region lost pointer capture.
    LostCapture,
    /// A press and release completed on the same region.
    Click,
}

/// A pointer interaction with the button state and modifiers that accompanied
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerEvent {
    /// Identifier of the pointer that produced the event.
    pub pointer_id: PointerId,
    /// Kind of pointing device.
    pub pointer_type: PointerType,
    /// Which interaction occurred.
    pub kind: PointerEventKind,
    /// Position in screen cells.
    pub position: ScreenPosition,
    /// Position relative to the receiving region, in cells.
    pub local_position: ScreenPosition,
    /// Button whose state changed, if any.
    pub button: PointerButton,
    /// Bit mask of buttons currently pressed.
    pub buttons: u16,
    /// Keyboard modifiers held during the event.
    pub modifiers: KeyModifiers,
}

impl PointerEvent {
    /// Build a mouse pointer event at `position`, with `local_position` equal to
    /// it until a receiving region overwrites it.
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

    /// Set the region-relative position, returning the updated event.
    pub(crate) fn with_local_position(mut self, local_position: ScreenPosition) -> Self {
        self.local_position = local_position;
        self
    }

    /// Whether this is the primary pointer. Crossterm exposes one mouse pointer,
    /// so it is always the primary pointer in the browser sense, independent of
    /// which button changed.
    pub fn is_primary(&self) -> bool {
        self.pointer_id == PointerId(0)
    }

    /// Whether the button that changed state is the primary button.
    pub fn is_primary_button(&self) -> bool {
        self.button == PointerButton::Primary
    }
}

/// A wheel or trackpad scroll at a screen position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelEvent {
    /// Position of the pointer in screen cells.
    pub position: ScreenPosition,
    /// Horizontal scroll amount in cells; positive scrolls right, and one wheel
    /// notch is one unit.
    pub delta_x: i16,
    /// Vertical scroll amount in rows; positive scrolls down, and one wheel
    /// notch is one unit.
    pub delta_y: i16,
    /// Keyboard modifiers held during the event.
    pub modifiers: KeyModifiers,
}

/// A scroll-offset change on a scrollable region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollEvent {
    /// Offset after applying `delta`.
    pub offset: ScrollOffset,
    /// Largest offset the region allows.
    pub max_offset: ScrollOffset,
    /// Change that produced `offset`.
    pub delta: ScrollDelta,
}

/// A key press, repeat, or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyboardEvent {
    /// The key, its modifiers, and which edge it reports.
    pub key: KeyEvent,
}

/// Propagation state for one dispatch, shared by every event family: every event
/// type can stop propagation and prevent its default action, not just keyboard
/// events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DispatchFrame {
    /// Whether later listeners on the route should be skipped.
    pub(crate) stopped: bool,
    /// Whether the framework's built-in default action is disabled.
    pub(crate) default_prevented: bool,
}

thread_local! {
    static PROPAGATION: RefCell<Vec<DispatchFrame>> = const { RefCell::new(Vec::new()) };
}

/// Apply `update` to the innermost propagation frame, if a dispatch is active.
fn update_frame(update: impl FnOnce(&mut DispatchFrame)) {
    PROPAGATION.with(|stack| {
        if let Some(frame) = stack.borrow_mut().last_mut() {
            update(frame);
        }
    });
}

/// Whether the innermost dispatch frame has stopped propagation.
pub(crate) fn propagation_stopped() -> bool {
    PROPAGATION.with(|stack| stack.borrow().last().is_some_and(|frame| frame.stopped))
}

/// Whether the innermost dispatch frame has prevented its default action.
pub(crate) fn default_prevented() -> bool {
    PROPAGATION.with(|stack| {
        stack
            .borrow()
            .last()
            .is_some_and(|frame| frame.default_prevented)
    })
}

/// Push a fresh propagation frame for one dispatch and return a guard that pops
/// it on drop.
///
/// Because restoration is tied to `Drop`, an unwinding listener cannot leave
/// stale "stopped" state behind for the next, unrelated dispatch.
pub(crate) fn begin_dispatch() -> PropagationGuard {
    PROPAGATION.with(|stack| stack.borrow_mut().push(DispatchFrame::default()));
    PropagationGuard { _private: () }
}

/// Pops the propagation frame pushed by [`begin_dispatch`] when dropped.
pub(crate) struct PropagationGuard {
    _private: (),
}

impl Drop for PropagationGuard {
    fn drop(&mut self) {
        PROPAGATION.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

/// Give every listed event type the same propagation controls.
macro_rules! propagation_controls {
    ($($event:ty),+ $(,)?) => {
        $(
            impl $event {
                /// Stop the event from reaching further ancestors.
                pub fn stop_propagation(&self) {
                    update_frame(|frame| frame.stopped = true);
                }

                /// Prevent the framework's built-in default action for this
                /// event (focus-on-press, wheel scrolling, text editing).
                pub fn prevent_default(&self) {
                    update_frame(|frame| frame.default_prevented = true);
                }
            }
        )+
    };
}

propagation_controls!(PointerEvent, WheelEvent, ScrollEvent);

impl KeyboardEvent {
    /// Stop the event from reaching further ancestors.
    pub fn stop_propagation(&self) {
        update_frame(|frame| frame.stopped = true);
    }

    /// Prevent the framework's built-in default action for this event (text
    /// editing and key scrolling).
    pub fn prevent_default(&self) {
        update_frame(|frame| frame.default_prevented = true);
    }
}

/// The terminal viewport changed size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeEvent {
    /// New viewport size in cells.
    pub size: Size,
}

/// Text pasted into the application.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PasteEvent {
    /// Pasted text. Shared so routing the event to every ancestor on the focused
    /// target's route copies a refcount rather than the whole pasted payload.
    pub text: Arc<str>,
}

/// Which pass of DOM-style event propagation a listener runs in. Capture runs
/// root-to-target before the target, and the target and its ancestors then
/// bubble.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EventPhase {
    /// Runs root-to-target before the target's listeners.
    Capture,
    /// Runs on the target itself.
    #[default]
    Target,
    /// Runs target-to-root after the target.
    Bubble,
}

/// The listeners registered on one region, keyed by event kind and phase.
///
/// Each field is an optional [`Attr`] slot, so a region carries only the
/// listeners it registered.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventHandlers {
    /// Capture-phase pointer press; runs before the target and bubble listeners
    /// and can stop the rest of the dispatch.
    pub pointer_down_capture: Attr<EventListener<PointerEvent>>,
    /// Capture-phase pointer release.
    pub pointer_up_capture: Attr<EventListener<PointerEvent>>,
    /// Capture-phase click.
    pub click_capture: Attr<EventListener<PointerEvent>>,
    /// Capture-phase key press.
    pub key_down_capture: Attr<EventListener<KeyboardEvent>>,
    /// Capture-phase key release.
    pub key_up_capture: Attr<EventListener<KeyboardEvent>>,
    /// Capture-phase wheel scroll.
    pub wheel_capture: Attr<EventListener<WheelEvent>>,
    /// Bubbling pointer press.
    pub pointer_down: Attr<EventListener<PointerEvent>>,
    /// Bubbling pointer release.
    pub pointer_up: Attr<EventListener<PointerEvent>>,
    /// Bubbling pointer motion, hover or drag.
    pub pointer_move: Attr<EventListener<PointerEvent>>,
    /// Bubbling pointer cancellation.
    pub pointer_cancel: Attr<EventListener<PointerEvent>>,
    /// Bubbling pointer bounds entry; see [`PointerEventKind::Over`].
    pub pointer_over: Attr<EventListener<PointerEvent>>,
    /// Bubbling pointer bounds exit; see [`PointerEventKind::Out`].
    pub pointer_out: Attr<EventListener<PointerEvent>>,
    /// Direct pointer entry, delivered only to the entered region.
    pub pointer_enter: Attr<EventListener<PointerEvent>>,
    /// Direct pointer exit, delivered only to the left region.
    pub pointer_leave: Attr<EventListener<PointerEvent>>,
    /// The region received pointer capture.
    pub got_pointer_capture: Attr<EventListener<PointerEvent>>,
    /// The region lost pointer capture.
    pub lost_pointer_capture: Attr<EventListener<PointerEvent>>,
    /// Completed press and release on the region.
    pub click: Attr<EventListener<PointerEvent>>,
    /// Wheel scroll over the region.
    pub wheel: Attr<EventListener<WheelEvent>>,
    /// Scroll-offset change on the region.
    pub scroll: Attr<EventListener<ScrollEvent>>,
    /// Key press on the focused route.
    pub key_down: Attr<EventListener<KeyboardEvent>>,
    /// Key release on the focused route.
    pub key_up: Attr<EventListener<KeyboardEvent>>,
    /// Targeted, bubbling keyboard hook. It only receives a key when this region
    /// is on the focused target's route.
    pub keyboard_event: Attr<EventListener<KeyboardEvent>>,
    /// Application-global keyboard hook. Unlike [`EventHandlers::keyboard_event`]
    /// it is not a focused target: it receives a key that no focused widget
    /// consumed, which is what application shortcuts need. It is a press hook:
    /// only press and repeat events reach it, never a release, so one physical
    /// press fires a shortcut exactly once even when the terminal reports both
    /// edges.
    pub app_key: Attr<EventListener<KeyboardEvent>>,
    /// Terminal viewport resize.
    pub resize_event: Attr<EventListener<ResizeEvent>>,
    /// DOM focus ownership change on the region.
    pub focus_event: Attr<EventListener<FocusEvent>>,
    /// Terminal window activation change.
    pub terminal_focus: Attr<EventListener<TerminalFocusEvent>>,
    /// Text pasted while the region is on the focused route.
    pub paste_event: Attr<EventListener<PasteEvent>>,
}

impl EventHandlers {
    /// Overlay every listener slot set in `overrides` onto the matching slot
    /// here, leaving unset slots unchanged.
    pub fn merge(&mut self, overrides: &Self) {
        macro_rules! merge {
            ($($field:ident),+ $(,)?) => {
                $(self.$field.overlay(&overrides.$field);)+
            };
        }
        merge!(
            pointer_down_capture,
            pointer_up_capture,
            click_capture,
            key_down_capture,
            key_up_capture,
            wheel_capture,
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

/// Map a crossterm mouse event to a pointer kind and changed button. Drag and
/// bare motion both map to [`PointerEventKind::Move`], and wheel motion maps to
/// `Move` with [`PointerButton::None`].
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
