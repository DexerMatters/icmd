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

use crate::runtime::hooks::{EffectCallback, FiberId, HookSlot, StateUpdate, UpdateQueue};
use crossbeam_channel::Sender;

use super::{
    common::{Attr, Node, fragment},
    props::Props,
};

static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct ContextKey<T> {
    id: u64,
    default: Arc<T>,
}

impl<T> ContextKey<T> {
    pub fn new(default: T) -> Self {
        Self {
            id: NEXT_CONTEXT_ID.fetch_add(1, Ordering::Relaxed),
            default: Arc::new(default),
        }
    }

    pub fn provider(&self, value: T, children: impl IntoIterator<Item = Node>) -> Node
    where
        T: Send + Sync + 'static,
    {
        Node::provider(self, value, children)
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    // The default is stored once as an `Arc`, so returning it to a consumer is
    // a refcount bump rather than a deep clone of the value.
    pub(crate) fn default_arc(&self) -> Arc<T> {
        self.default.clone()
    }
}

pub fn create_context<T>(default: T) -> ContextKey<T> {
    ContextKey::new(default)
}

#[derive(Clone)]
pub struct ProviderProps<T: 'static> {
    pub context_key: Attr<&'static ContextKey<T>>,
    pub value: Attr<T>,
}

impl<T> Default for ProviderProps<T> {
    fn default() -> Self {
        Self {
            context_key: Attr::Unset,
            value: Attr::Unset,
        }
    }
}

// Legacy provider component retained for macro compatibility. Missing required
// fields are a defined no-op: the children render with the inherited context
// rather than panicking the worker. `ContextKey::provider(value, child)` is the
// canonical, type-safe construction path.
pub fn provider<T>(_cx: &mut ComponentContext, props: &Props<ProviderProps<T>>) -> Node
where
    T: Clone + Send + Sync + 'static,
{
    let (Some(key), Some(value)) = (
        props.context_key.as_ref().copied(),
        props.value.as_ref().cloned(),
    ) else {
        return fragment(props.children.clone());
    };
    key.provider(value, props.children.clone())
}

#[derive(Clone, Default)]
pub(crate) struct ContextValues {
    values: HashMap<u64, Arc<dyn Any + Send + Sync>>,
}

impl ContextValues {
    // Shared read: the stored value is an `Arc`, so this does not clone `T`.
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

#[derive(Clone)]
pub struct StateSetter<T> {
    fiber: FiberId,
    hook: usize,
    updates: UpdateQueue,
    marker: PhantomData<fn(T)>,
    wake: Sender<()>,
}

impl<T: Send + 'static> StateSetter<T> {
    pub fn set(&self, value: T) {
        self.update(move |state| *state = value);
    }

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

pub type Ref<T> = Arc<Mutex<T>>;

pub trait EffectResult: Send + 'static {
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

pub struct ComponentContext<CustomHook = ()> {
    pub(crate) fiber: FiberId,
    pub(crate) hook_cursor: usize,
    pub(crate) hooks: Vec<HookSlot>,
    pub(crate) updates: UpdateQueue,
    pub(crate) wake: Sender<()>,
    pub(crate) inherited_context: ContextValues,
    pub(crate) provided_context: ContextValues,
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
    ) -> Self {
        Self {
            fiber,
            hook_cursor: 0,
            hooks,
            updates,
            wake,
            inherited_context,
            provided_context: ContextValues::default(),
            custom_hook: CustomHook::default(),
        }
    }

    pub fn use_context<'a, T>(&self, context: impl FnOnce() -> &'a ContextKey<T>) -> T
    where
        T: Clone + Send + Sync + 'static,
    {
        (*self.use_context_arc(context)).clone()
    }

    // Canonical shared read. Consumers that only inspect the value should use
    // this and dereference, which avoids cloning the value on every render.
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

    pub fn provide<T>(&mut self, context: &ContextKey<T>, value: T)
    where
        T: Send + Sync + 'static,
    {
        self.provided_context.insert(context, value);
    }

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

    pub fn use_effect<D, F, R>(&mut self, dependencies: D, effect: F)
    where
        D: PartialEq + Send + 'static,
        F: FnOnce() -> R + Send + 'static,
        R: EffectResult,
    {
        self.use_optional_effect(dependencies, move || effect().into_cleanup());
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
