//! Hook storage: the per-fiber hook slots a component's render fills.
//!
//! Slots are positional, so a component must call its hooks in the same order
//! on every render.

use std::{
    any::Any,
    collections::VecDeque,
    sync::{Arc, Mutex},
};

slotmap::new_key_type! {
    pub(crate) struct FiberId;
}

pub(crate) type Cleanup = Box<dyn FnOnce() + Send>;
pub(crate) type EffectCallback = Box<dyn FnOnce() -> Option<Cleanup> + Send>;
pub(crate) type StateUpdateCallback = Box<dyn FnOnce(&mut (dyn Any + Send)) + Send>;
pub(crate) type UpdateQueue = Arc<Mutex<VecDeque<StateUpdate>>>;

pub(crate) enum HookSlot {
    State(Box<dyn Any + Send>),
    Ref(Box<dyn Any + Send>),
    Memo {
        dependencies: Box<dyn Any + Send>,
        value: Box<dyn Any + Send>,
    },
    Effect {
        dependencies: Box<dyn Any + Send>,
        cleanup: Option<Cleanup>,
        pending: Option<EffectCallback>,
    },
}

impl HookSlot {
    pub(crate) fn cleanup(self) {
        if let Self::Effect {
            cleanup: Some(cleanup),
            ..
        } = self
        {
            cleanup();
        }
    }
}

pub(crate) struct StateUpdate {
    pub fiber: FiberId,
    pub hook: usize,
    pub apply: StateUpdateCallback,
}
