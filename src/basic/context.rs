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
};

use super::hooks::{EffectCallback, FiberId, HookSlot, StateUpdate, UpdateQueue};
use super::lifetime::AppHandle;
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
            .push_back(StateUpdate {
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

/// Shared state with an encapsulated mutex.
///
/// The lock strategy stays a private detail, so it can change without a public
/// compatibility event, and poison handling is centralized here.
pub struct StateRef<T> {
    inner: Arc<Mutex<T>>,
}

impl<T> StateRef<T> {
    /// Creates shared state holding `value` under a new mutex.
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(Mutex::new(value)),
        }
    }

    pub(crate) fn from_ref(inner: Ref<T>) -> Self {
        Self { inner }
    }

    /// Wraps an existing `Ref<T>` allocation, sharing the same value.
    ///
    /// Used by callers migrating from `Ref<T>` and by tests that observe poisoning.
    pub fn from_shared(inner: Ref<T>) -> Self {
        Self { inner }
    }

    /// Runs `f` on a shared borrow, or reports `StateError::Poisoned`.
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R, StateError> {
        let guard = self.inner.lock().map_err(|_| StateError::Poisoned)?;
        Ok(f(&guard))
    }

    /// Runs `f` on a mutable borrow, or reports `StateError::Poisoned`.
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, StateError> {
        let mut guard = self.inner.lock().map_err(|_| StateError::Poisoned)?;
        Ok(f(&mut guard))
    }

    /// Runs `f` on a mutable borrow without blocking; returns `Ok(None)` when the lock is held.
    pub fn try_update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<Option<R>, StateError> {
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

    /// Returns a `StateRef` sharing the same allocation as `use_ref`.
    pub fn use_state_ref<T: Send + 'static>(&mut self, initial: impl FnOnce() -> T) -> StateRef<T> {
        StateRef::from_ref(self.use_ref(initial))
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
