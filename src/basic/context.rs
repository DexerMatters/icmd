use std::{
    any::Any,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use crate::runtime::lower::{EffectCallback, FiberId, HookSlot, StateUpdate, UpdateQueue};
use crossbeam_channel::Sender;

/// A clonable handle for scheduling state updates.
///
/// Updates are queued and applied during the next synchronous pipeline pass, so
/// calling a setter never renders recursively.
pub struct StateSetter<T> {
    fiber: FiberId,
    hook: usize,
    updates: UpdateQueue,
    marker: PhantomData<fn(T)>,
    wake: Sender<()>,
}

impl<T> Clone for StateSetter<T> {
    fn clone(&self) -> Self {
        Self {
            fiber: self.fiber,
            hook: self.hook,
            updates: self.updates.clone(),
            marker: PhantomData,
            wake: self.wake.clone(),
        }
    }
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

/// A persistent, thread-safe value which does not itself schedule a render.
pub type Ref<T> = Arc<Mutex<T>>;

/// The return value of an effect: either `()` or a cleanup closure.
pub trait EffectResult: Send + 'static {
    #[doc(hidden)]
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

/// Hook context passed to function components while they render.
pub struct Context {
    pub(crate) fiber: FiberId,
    pub(crate) hook_cursor: usize,
    pub(crate) hooks: Vec<HookSlot>,
    pub(crate) updates: UpdateQueue,
    pub(crate) wake: Sender<()>,
}

impl Context {
    pub(crate) fn new(
        fiber: FiberId,
        hooks: Vec<HookSlot>,
        updates: UpdateQueue,
        wake: Sender<()>,
    ) -> Self {
        Self {
            fiber,
            hook_cursor: 0,
            hooks,
            updates,
            wake,
        }
    }

    /// Returns a snapshot of state and a stable setter for future renders.
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

    /// Keeps a mutable value for the component lifetime without causing rerenders.
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

    /// Recomputes a value only when its dependencies change.
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

    /// Runs an effect after the render is committed and cleans it up before its
    /// dependencies change or the component unmounts.
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

    pub(crate) fn finish(mut self) -> Vec<HookSlot> {
        if self.hook_cursor < self.hooks.len() {
            for hook in self.hooks.drain(self.hook_cursor..) {
                hook.cleanup();
            }
        }
        self.hooks
    }
}
