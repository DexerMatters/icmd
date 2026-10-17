//! Component context, hook state, shared-state handles, and context keys.
//! Owns the per-render `ComponentContext` and the `ContextKey`, `StateSetter`,
//! `StateRef`, and `Ref` shared-state types.

use std::{
    any::Any,
    collections::HashMap,
    marker::PhantomData,
    ops::{self},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use super::hooks::{EffectCallback, FiberId, HookSlot, StateUpdate, UpdateQueue};
use super::lifetime::{AppHandle, FrameClock, FrameClockState};
use crossbeam_channel::Sender;

use super::common::Node;

static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A typed, process-unique key naming a context value and its default.
#[derive(Clone)]
pub struct ContextKey<T> {
    id: u64,
    default: Arc<T>,
}

impl<T> ContextKey<T> {
    /// Creates a key with a process-unique id and a default value.
    pub fn new(default: T) -> Self {
        Self {
            id: NEXT_CONTEXT_ID.fetch_add(1, Ordering::Relaxed),
            default: Arc::new(default),
        }
    }

    /// Builds a provider node that publishes `value` under this key to `children`.
    pub fn provider(&self, value: T, children: impl IntoIterator<Item = Node>) -> Node
    where
        T: Send + Sync + 'static,
    {
        Node::provider(self, value, children)
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Returns the default value as an `Arc`, a refcount bump rather than a deep clone.
    pub(crate) fn default_arc(&self) -> Arc<T> {
        self.default.clone()
    }
}

/// Creates a context key whose default is seen when no provider is above the consumer.
pub fn create_context<T>(default: T) -> ContextKey<T> {
    ContextKey::new(default)
}

#[derive(Clone, Default)]
pub(crate) struct ContextValues {
    values: HashMap<u64, Arc<dyn Any + Send + Sync>>,
}

impl ContextValues {
    /// Reads the stored value as an `Arc`, without cloning `T`.
    pub(crate) fn get_arc<T>(&self, key: &ContextKey<T>) -> Option<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.values
            .get(&key.id)
            .and_then(|value| value.clone().downcast::<T>().ok())
    }

    pub(crate) fn insert<T>(&mut self, key: &ContextKey<T>, value: T)
    where
        T: Send + Sync + 'static,
    {
        self.values.insert(key.id, Arc::new(value));
    }

    pub(crate) fn insert_erased(&mut self, id: u64, value: Arc<dyn Any + Send + Sync>) {
        self.values.insert(id, value);
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        self.values
            .extend(other.values.iter().map(|(id, value)| (*id, value.clone())));
    }
}

/// A handle that queues state updates for one hook slot and wakes the runtime.
#[derive(Clone)]
pub struct StateSetter<T> {
    fiber: FiberId,
    hook: usize,
    updates: UpdateQueue,
    marker: PhantomData<fn(T)>,
    wake: Sender<()>,
}

impl<T: Send + 'static> StateSetter<T> {
    /// Replaces the state with `value`.
    pub fn set(&self, value: T) {
        self.update(move |state| *state = value);
    }

    /// Applies `update` to the current state and wakes the runtime.
    pub fn update(&self, update: impl FnOnce(&mut T) + Send + 'static) {
        let apply = Box::new(move |state: &mut (dyn Any + Send)| {
            let state = state
                .downcast_mut::<T>()
                .expect("state setter used with a different hook type");
            update(state);
        });
        self.updates
            .lock()
            .expect("state update queue poisoned")
            .push_back(StateUpdate::Apply {
                fiber: self.fiber,
                hook: self.hook,
                apply,
            });
        let _ = self.wake.try_send(());
    }
}

/// A shared, mutex-protected value; alias for `Arc<Mutex<T>>`.
///
/// Prefer `StateRef`, which encapsulates the lock and reports poisoning.
pub type Ref<T> = Arc<Mutex<T>>;

/// A shared-state failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateError {
    /// The lock was poisoned by a panicking holder.
    Poisoned,
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poisoned => write!(f, "state was poisoned by a panicking holder"),
        }
    }
}

impl std::error::Error for StateError {}

/// The runtime-side wake target attached to a live shared state.
#[derive(Clone)]
struct LiveWake {
    fiber: FiberId,
    updates: UpdateQueue,
    wake: Sender<()>,
}

/// Shared state with an encapsulated mutex.
///
/// The lock strategy stays a private detail, so it can change without a public
/// compatibility event, and poison handling is centralized here.
///
/// A handle created by [`ComponentContext::use_live_state`] (or
/// [`ComponentContext::use_state_ref`]) is *live*: every successful [`update`]
/// or [`set`] wakes the component that owns it, so the next frame reads the new
/// value. A handle created with [`StateRef::new`] or
/// [`StateRef::from_shared`] is detached and never wakes.
///
/// [`update`]: StateRef::update
/// [`set`]: StateRef::set
pub struct StateRef<T> {
    inner: Arc<Mutex<T>>,
    live: Option<LiveWake>,
}

impl<T> Clone for StateRef<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            live: self.live.clone(),
        }
    }
}

impl<T> StateRef<T> {
    /// Creates detached shared state holding `value` under a new mutex.
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(Mutex::new(value)),
            live: None,
        }
    }

    pub(crate) fn from_live(
        inner: Ref<T>,
        fiber: FiberId,
        updates: UpdateQueue,
        wake: Sender<()>,
    ) -> Self {
        Self {
            inner,
            live: Some(LiveWake {
                fiber,
                updates,
                wake,
            }),
        }
    }

    /// Wraps an existing `Ref<T>` allocation, sharing the same value.
    ///
    /// The result is detached: updates are visible to readers but do not wake
    /// the runtime, exactly like [`StateRef::new`].
    pub fn from_shared(inner: Ref<T>) -> Self {
        Self { inner, live: None }
    }

    /// Whether updates from this handle wake the runtime.
    pub fn is_live(&self) -> bool {
        self.live.is_some()
    }

    /// Whether two handles share the same allocation.
    pub fn same_allocation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Signals the owning component that the shared value changed.
    ///
    /// A no-op for a detached handle. The update queue coalesces repeated
    /// touches, so a batched event produces one rerender.
    fn notify(&self) {
        let Some(live) = &self.live else {
            return;
        };
        live.updates
            .lock()
            .expect("state update queue poisoned")
            .push_back(StateUpdate::Touch { fiber: live.fiber });
        let _ = live.wake.try_send(());
    }

    /// Runs `f` on a shared borrow, or reports `StateError::Poisoned`.
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R, StateError> {
        let guard = self.inner.lock().map_err(|_| StateError::Poisoned)?;
        Ok(f(&guard))
    }

    /// Replaces the value and, for a live handle, wakes the runtime.
    pub fn set(&self, value: T) -> Result<(), StateError> {
        self.update(move |state| *state = value)
    }

    /// Runs `f` on a mutable borrow and, for a live handle, wakes the runtime.
    ///
    /// Use [`StateRef::update_quiet`] when several updates belong to one
    /// deliberate batch and the caller will wake separately.
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, StateError> {
        let result = self.update_quiet(f)?;
        self.notify();
        Ok(result)
    }

    /// Runs `f` on a mutable borrow without waking the runtime.
    pub fn update_quiet<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, StateError> {
        let mut guard = self.inner.lock().map_err(|_| StateError::Poisoned)?;
        Ok(f(&mut guard))
    }

    /// Runs `f` on a mutable borrow without blocking; returns `Ok(None)` when the lock is held.
    ///
    /// A successful live update wakes the runtime.
    pub fn try_update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<Option<R>, StateError> {
        let result = self.try_update_quiet(f)?;
        if result.is_some() {
            self.notify();
        }
        Ok(result)
    }

    /// Runs `f` on a mutable borrow without blocking and without waking.
    pub fn try_update_quiet<R>(
        &self,
        f: impl FnOnce(&mut T) -> R,
    ) -> Result<Option<R>, StateError> {
        match self.inner.try_lock() {
            Ok(mut guard) => Ok(Some(f(&mut guard))),
            Err(std::sync::TryLockError::WouldBlock) => Ok(None),
            Err(std::sync::TryLockError::Poisoned(_)) => Err(StateError::Poisoned),
        }
    }
}

/// The value an effect may return: `()` for no cleanup, or a `FnOnce()` cleanup.
pub trait EffectResult: Send + 'static {
    /// Converts this result into an optional cleanup closure.
    fn into_cleanup(self) -> Option<Box<dyn FnOnce() + Send>>;
}

impl EffectResult for () {
    fn into_cleanup(self) -> Option<Box<dyn FnOnce() + Send>> {
        None
    }
}

impl<F> EffectResult for F
where
    F: FnOnce() + Send + 'static,
{
    fn into_cleanup(self) -> Option<Box<dyn FnOnce() + Send>> {
        Some(Box::new(self))
    }
}

/// The per-render hook and context environment passed to a component.
pub struct ComponentContext<CustomHook = ()> {
    pub(crate) fiber: FiberId,
    pub(crate) hook_cursor: usize,
    pub(crate) hooks: Vec<HookSlot>,
    pub(crate) updates: UpdateQueue,
    pub(crate) wake: Sender<()>,
    pub(crate) inherited_context: ContextValues,
    pub(crate) provided_context: ContextValues,
    pub(crate) handle: AppHandle,
    pub(crate) custom_hook: CustomHook,
}

impl<CustomHook> ComponentContext<CustomHook>
where
    CustomHook: Default,
{
    pub(crate) fn new(
        fiber: FiberId,
        hooks: Vec<HookSlot>,
        updates: UpdateQueue,
        wake: Sender<()>,
        inherited_context: ContextValues,
        handle: AppHandle,
    ) -> Self {
        Self {
            fiber,
            hook_cursor: 0,
            hooks,
            updates,
            wake,
            inherited_context,
            provided_context: ContextValues::default(),
            handle,
            custom_hook: CustomHook::default(),
        }
    }

    /// Returns a cloneable session control handle.
    ///
    /// Its `request_exit` stops the application like the exit key does, running
    /// the Unmount and Exit phases and restoring the terminal.
    pub fn use_handle(&self) -> AppHandle {
        self.handle.clone()
    }

    /// The committed terminal viewport, in cells.
    ///
    /// A detached pipeline reports the conventional 80x24 default. The value is
    /// refreshed by a resize event before the next render.
    pub fn use_viewport(&self) -> crate::Size {
        self.handle.viewport()
    }

    /// Asks the session for one more full frame.
    ///
    /// Best-effort and safe from any thread; the loop observes the request
    /// within one render wait.
    pub fn request_frame(&self) {
        self.handle.request_frame();
    }

    /// Returns a clone of the value published for `context`, or its default.
    pub fn use_context<'a, T>(&self, context: impl FnOnce() -> &'a ContextKey<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        (*self.use_context_arc(context)).clone()
    }

    /// Returns the `Arc` published for `context`, or its default.
    ///
    /// Consumers that only inspect the value should use this and dereference,
    /// which avoids cloning the value on every render.
    pub fn use_context_arc<'a, T>(
        &self,
        context: impl FnOnce() -> &'a ContextKey<T>,
    ) -> std::sync::Arc<T>
    where
        T: Send + Sync + 'static,
    {
        let context = context();
        self.inherited_context
            .get_arc(context)
            .unwrap_or_else(|| context.default_arc())
    }

    /// Publishes `value` under `context` to this subtree.
    pub fn provide<T>(&mut self, context: &ContextKey<T>, value: T)
    where
        T: Send + Sync + 'static,
    {
        self.provided_context.insert(context, value);
    }

    /// Returns the current state and a setter, calling `initial` only on first render.
    ///
    /// The returned `T` is a snapshot captured during this render. [`StateSetter`]
    /// writes are queued and applied before the next render, so a frame that is
    /// already being composed still sees the old value: reading the hook again in
    /// the same render does not observe a `set` that was just issued.
    ///
    /// For state an event handler mutates and reads back immediately, use
    /// [`ComponentContext::use_live_state`] instead.
    pub fn use_state<T: Clone + Send + 'static>(
        &mut self,
        initial: impl FnOnce() -> T,
    ) -> (T, StateSetter<T>) {
        let index = self.next_hook();
        if index == self.hooks.len() {
            self.hooks.push(HookSlot::State(Box::new(initial())));
        }
        let value = match &self.hooks[index] {
            HookSlot::State(value) => value
                .downcast_ref::<T>()
                .expect("hook order or state type changed between renders")
                .clone(),
            _ => panic!("hook order changed between renders: expected state hook"),
        };
        (
            value,
            StateSetter {
                fiber: self.fiber,
                hook: index,
                updates: self.updates.clone(),
                wake: self.wake.clone(),
                marker: PhantomData,
            },
        )
    }

    /// Returns the persistent `Ref<T>` for this hook slot, calling `initial` on first render.
    pub fn use_ref<T: Send + 'static>(&mut self, initial: impl FnOnce() -> T) -> Ref<T> {
        let index = self.next_hook();
        if index == self.hooks.len() {
            self.hooks
                .push(HookSlot::Ref(Box::new(Arc::new(Mutex::new(initial())))));
        }
        match &self.hooks[index] {
            HookSlot::Ref(value) => value
                .downcast_ref::<Ref<T>>()
                .expect("hook order or ref type changed between renders")
                .clone(),
            _ => panic!("hook order changed between renders: expected ref hook"),
        }
    }

    /// Returns a stable ref to a concrete host element's latest committed
    /// geometry and resolved properties.
    pub fn use_element_ref(&mut self) -> super::element_ref::ElementRef {
        let cell = self.use_ref(super::element_ref::ElementRef::new);
        cell.lock().expect("element ref hook poisoned").clone()
    }

    /// Returns a live `StateRef` sharing the same allocation as `use_ref`.
    ///
    /// Reads observe writes immediately, and a successful update wakes this
    /// component, so a value mutated by an event handler reaches the screen
    /// without a caller-managed redraw tick. Use [`StateRef::update_quiet`]
    /// for a batch that should not wake until the caller is ready.
    ///
    /// This is the historical name for [`ComponentContext::use_live_state`].
    pub fn use_state_ref<T: Send + 'static>(&mut self, initial: impl FnOnce() -> T) -> StateRef<T> {
        self.use_live_state(initial)
    }

    /// Returns a shared, live value whose updates wake this component.
    ///
    /// The value is stored behind the hook's shared allocation, so reads from
    /// an event handler and the next render see the write synchronously. Each
    /// successful [`StateRef::update`] or [`StateRef::set`] wakes the lowerer;
    /// repeated updates before the next pass coalesce into one rerender.
    ///
    /// A handle created outside a component ([`StateRef::new`] or
    /// [`StateRef::from_shared`]) is detached and quiet.
    pub fn use_live_state<T: Send + 'static>(
        &mut self,
        initial: impl FnOnce() -> T,
    ) -> StateRef<T> {
        let index = self.next_hook();
        if index == self.hooks.len() {
            self.hooks
                .push(HookSlot::Ref(Box::new(Arc::new(Mutex::new(initial())))));
        }
        let inner = match &self.hooks[index] {
            HookSlot::Ref(value) => value
                .downcast_ref::<Ref<T>>()
                .expect("hook order or ref type changed between renders")
                .clone(),
            _ => panic!("hook order changed between renders: expected ref hook"),
        };
        StateRef::from_live(inner, self.fiber, self.updates.clone(), self.wake.clone())
    }

    /// Registers a callback that runs after every presented frame.
    ///
    /// The callback receives the elapsed time since its previous run, clamped
    /// to four intervals so a pause does not produce a huge delta. The clock is
    /// active on mount; [`FrameClock::pause`] stops it and
    /// [`FrameClock::resume`] restarts it. The owning component also rerenders
    /// on each tick even when the callback does not touch a state setter.
    ///
    /// Callbacks run on the event-loop thread and must not block.
    pub fn use_frame_clock(&mut self, tick: impl FnMut(Duration) + Send + 'static) -> FrameClock {
        let signal = self.use_live_state(|| 0_u64);
        let shared_cell = self.use_ref({
            let signal = signal.clone();
            || Arc::new(Mutex::new(FrameClockState::new(signal)))
        });
        let shared = shared_cell.lock().expect("frame clock poisoned").clone();
        let sender = self.handle.clone();
        {
            let mut state = shared.lock().expect("frame clock poisoned");
            state.callback = Some(Arc::new(Mutex::new(Box::new(tick))));
        }
        let registered = self.use_ref(|| None::<u64>);
        let needs_registration = registered
            .lock()
            .expect("frame clock registration poisoned")
            .is_none();
        if needs_registration {
            let id = sender.register_frame_clock(shared.clone());
            *registered
                .lock()
                .expect("frame clock registration poisoned") = Some(id);
        }
        let cleanup_handle = sender.clone();
        let cleanup_registered = registered.clone();
        self.use_mount_effect(move || {
            let id = *cleanup_registered
                .lock()
                .expect("frame clock registration poisoned");
            move || {
                if let Some(id) = id {
                    cleanup_handle.unregister_frame_clock(id);
                }
            }
        });
        FrameClock { state: shared }
    }

    /// Returns the memoized value, recomputing only when `dependencies` change.
    pub fn use_memo<T, D>(&mut self, dependencies: D, compute: impl FnOnce() -> T) -> T
    where
        T: Clone + Send + 'static,
        D: PartialEq + Send + 'static,
    {
        let index = self.next_hook();
        if index == self.hooks.len() {
            let value = compute();
            self.hooks.push(HookSlot::Memo {
                dependencies: Box::new(dependencies),
                value: Box::new(value.clone()),
            });
            return value;
        }
        match &mut self.hooks[index] {
            HookSlot::Memo {
                dependencies: old,
                value,
            } => {
                let unchanged = old
                    .downcast_ref::<D>()
                    .expect("memo dependency type changed between renders")
                    == &dependencies;
                if unchanged {
                    value
                        .downcast_ref::<T>()
                        .expect("memo value type changed between renders")
                        .clone()
                } else {
                    let next = compute();
                    *old = Box::new(dependencies);
                    *value = Box::new(next.clone());
                    next
                }
            }
            _ => panic!("hook order changed between renders: expected memo hook"),
        }
    }

    /// Runs `effect` after render whenever `dependencies` change; its result is the cleanup.
    pub fn use_effect<D, F, R>(&mut self, dependencies: D, effect: F)
    where
        D: PartialEq + Send + 'static,
        F: FnOnce() -> R + Send + 'static,
        R: EffectResult,
    {
        self.use_optional_effect(dependencies, move || effect().into_cleanup());
    }

    /// Runs `effect` once at mount; its result is the unmount cleanup.
    pub fn use_mount_effect<R: EffectResult>(
        &mut self,
        effect: impl FnOnce() -> R + Send + 'static,
    ) {
        self.use_effect((), effect);
    }

    /// Registers `cleanup` to run once when this component unmounts.
    ///
    /// The body never runs at mount. For app-scoped exit behavior that must also
    /// run when the tree never mounted, use `AppLifecycle::on_unmount`/`on_exit`.
    pub fn use_unmount(&mut self, cleanup: impl FnOnce() + Send + 'static) {
        self.use_effect((), move || cleanup);
    }

    fn use_optional_effect<D, F>(&mut self, dependencies: D, effect: F)
    where
        D: PartialEq + Send + 'static,
        F: FnOnce() -> Option<Box<dyn FnOnce() + Send>> + Send + 'static,
    {
        let index = self.next_hook();
        let callback: EffectCallback = Box::new(effect);
        if index == self.hooks.len() {
            self.hooks.push(HookSlot::Effect {
                dependencies: Box::new(dependencies),
                cleanup: None,
                pending: Some(callback),
            });
            return;
        }
        match &mut self.hooks[index] {
            HookSlot::Effect {
                dependencies: old,
                pending,
                ..
            } => {
                let unchanged = old
                    .downcast_ref::<D>()
                    .expect("effect dependency type changed between renders")
                    == &dependencies;
                if !unchanged {
                    *old = Box::new(dependencies);
                    *pending = Some(callback);
                }
            }
            _ => panic!("hook order changed between renders: expected effect hook"),
        }
    }

    fn next_hook(&mut self) -> usize {
        let index = self.hook_cursor;
        self.hook_cursor += 1;
        index
    }

    pub(crate) fn finish(mut self) -> (Vec<HookSlot>, ContextValues) {
        if self.hook_cursor < self.hooks.len() {
            for hook in self.hooks.drain(self.hook_cursor..) {
                hook.cleanup();
            }
        }
        (self.hooks, self.provided_context)
    }
}

impl<CustomHook> ops::Deref for ComponentContext<CustomHook> {
    type Target = CustomHook;
    fn deref(&self) -> &Self::Target {
        &self.custom_hook
    }
}

impl<CustomHook> ops::DerefMut for ComponentContext<CustomHook> {
    fn deref_mut(&mut self) -> &mut CustomHook {
        &mut self.custom_hook
    }
}
