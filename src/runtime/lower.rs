use std::{
    any::{Any, TypeId},
    collections::{HashSet, VecDeque},
    mem,
    sync::{Arc, Mutex},
};

use crossbeam_channel::{Receiver, Sender, bounded};
use slotmap::SlotMap;

use crate::basic::{DomId, DomNode, DomProps, Key, Node, common::NodeKind, context::Context};

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

type FiberArena = SlotMap<FiberId, Fiber>;

enum FiberKind {
    Root,
    Image(crate::Image),
    Function {
        type_id: TypeId,
        render_fn: fn(&mut Context, &dyn Any) -> Node,
    },
    Fragment,
    Empty,
}

struct Fiber {
    key: Option<Key>,
    kind: FiberKind,
    dom_id: Option<DomId>,
    dom_props: Option<DomProps>,
    children: Vec<FiberId>,
    props: Option<Arc<dyn Any + Send + Sync>>,
    hooks: Vec<HookSlot>,
}

impl Fiber {
    fn root() -> Self {
        Self {
            key: None,
            kind: FiberKind::Root,
            dom_id: None,
            dom_props: None,
            children: Vec::new(),
            props: None,
            hooks: Vec::new(),
        }
    }
}

/// Stateful reactive lowering from function components to a host-only DOM tree.
///
/// `Lower` owns fiber identity and hooks. It deliberately knows nothing about
/// renderer operations; a later DOM commit stage can consume its output.
pub struct Lower {
    fibers: FiberArena,
    root: FiberId,
    root_node: Option<Node>,
    next_dom_id: u64,
    updates: UpdateQueue,
    wake_tx: Sender<()>,
    wake_rx: Receiver<()>,
}

impl Default for Lower {
    fn default() -> Self {
        Self::new()
    }
}

impl Lower {
    fn new() -> Self {
        let mut fibers = FiberArena::with_key();
        let root = fibers.insert(Fiber::root());
        let (wake_tx, wake_rx) = bounded(1);
        Self {
            fibers,
            root,
            root_node: None,
            next_dom_id: 1,
            updates: Arc::new(Mutex::new(VecDeque::new())),
            wake_tx,
            wake_rx,
        }
    }

    fn lower(&mut self, node: Node) -> DomNode {
        assert!(
            matches!(&node.kind, NodeKind::Component { .. }),
            "Lower requires a component as its root input"
        );
        self.apply_updates();
        self.root_node = Some(node.clone());
        self.reconcile_children(self.root, vec![node]);
        self.commit_effects();
        self.root_dom()
    }

    fn unmount(&mut self) {
        let children = mem::take(&mut self.fibers[self.root].children);
        for child in children {
            self.remove_subtree(child);
        }
    }

    fn root_dom(&self) -> DomNode {
        let root = self.fibers[self.root]
            .children
            .first()
            .copied()
            .expect("root component was not mounted");
        self.build_dom(root)
            .into_iter()
            .next()
            .expect("a root component must lower to one DOM element")
    }

    fn build_dom(&self, fiber: FiberId) -> Vec<DomNode> {
        let current = &self.fibers[fiber];
        match &current.kind {
            FiberKind::Function { .. } => vec![DomNode::Element {
                id: current.dom_id.expect("component DOM id missing"),
                props: current
                    .dom_props
                    .clone()
                    .expect("component DOM props missing"),
                children: current
                    .children
                    .iter()
                    .flat_map(|child| self.build_dom(*child))
                    .collect(),
            }],
            FiberKind::Image(image) => vec![DomNode::Image {
                id: current.dom_id.expect("image DOM id missing"),
                image: image.clone(),
            }],
            FiberKind::Fragment => current
                .children
                .iter()
                .flat_map(|child| self.build_dom(*child))
                .collect(),
            FiberKind::Empty | FiberKind::Root => Vec::new(),
        }
    }

    fn apply_updates(&mut self) -> bool {
        while self.wake_rx.try_recv().is_ok() {}
        let updates = {
            let mut queue = self.updates.lock().expect("state update queue poisoned");
            queue.drain(..).collect::<Vec<_>>()
        };
        let mut applied = false;
        for update in updates {
            let Some(fiber) = self.fibers.get_mut(update.fiber) else {
                continue;
            };
            let Some(HookSlot::State(state)) = fiber.hooks.get_mut(update.hook) else {
                continue;
            };
            (update.apply)(state.as_mut());
            applied = true;
        }
        applied
    }

    fn reconcile_children(&mut self, parent: FiberId, nodes: Vec<Node>) {
        let old = self.fibers[parent].children.clone();
        let mut used = HashSet::new();
        let mut children = Vec::with_capacity(nodes.len());
        let mut seen_keys = HashSet::new();

        for (index, node) in nodes.into_iter().enumerate() {
            if let Some(key) = node.node_key()
                && !seen_keys.insert(key.clone())
            {
                panic!("duplicate sibling key {:?}", key);
            }
            let matched = if let Some(key) = node.node_key() {
                old.iter().copied().find(|candidate| {
                    !used.contains(candidate)
                        && self.fibers[*candidate].key.as_ref() == Some(key)
                        && self.compatible(*candidate, &node)
                })
            } else {
                old.get(index).copied().filter(|candidate| {
                    !used.contains(candidate)
                        && self.fibers[*candidate].key.is_none()
                        && self.compatible(*candidate, &node)
                })
            };

            let child = if let Some(fiber) = matched {
                used.insert(fiber);
                self.reconcile_node(fiber, node);
                fiber
            } else {
                self.mount_node(node)
            };
            children.push(child);
        }
        for child in old {
            if !used.contains(&child) {
                self.remove_subtree(child);
            }
        }
        self.fibers[parent].children = children;
    }

    fn compatible(&self, fiber: FiberId, node: &Node) -> bool {
        match (&self.fibers[fiber].kind, &node.kind) {
            (FiberKind::Image(_), NodeKind::Image(_))
            | (FiberKind::Fragment, NodeKind::Fragment(_))
            | (FiberKind::Empty, NodeKind::Empty) => true,
            (FiberKind::Function { type_id, .. }, NodeKind::Component { type_id: next, .. }) => {
                type_id == next
            }
            _ => false,
        }
    }

    fn mount_node(&mut self, node: Node) -> FiberId {
        let key = node.node_key().cloned();
        let (kind, dom_id, dom_props, props) = match &node.kind {
            NodeKind::Image(image) => (
                FiberKind::Image(image.clone()),
                Some(self.allocate_dom_id()),
                None,
                None,
            ),
            NodeKind::Component {
                type_id,
                render_fn,
                dom,
                props,
            } => (
                FiberKind::Function {
                    type_id: *type_id,
                    render_fn: *render_fn,
                },
                Some(self.allocate_dom_id()),
                Some(dom.clone()),
                Some(props.clone()),
            ),
            NodeKind::Fragment(_) => (FiberKind::Fragment, None, None, None),
            NodeKind::Empty => (FiberKind::Empty, None, None, None),
        };
        let fiber = self.fibers.insert(Fiber {
            key,
            kind,
            dom_id,
            dom_props,
            children: Vec::new(),
            props,
            hooks: Vec::new(),
        });
        match node.kind {
            NodeKind::Component { .. } => self.render_component(fiber),
            NodeKind::Fragment(children) => self.reconcile_children(fiber, children),
            NodeKind::Image(_) | NodeKind::Empty => {}
        }
        fiber
    }

    fn reconcile_node(&mut self, fiber: FiberId, node: Node) {
        self.fibers[fiber].key = node.node_key().cloned();
        match node.kind {
            NodeKind::Image(image) => self.fibers[fiber].kind = FiberKind::Image(image),
            NodeKind::Component { dom, props, .. } => {
                self.fibers[fiber].dom_props = Some(dom);
                self.fibers[fiber].props = Some(props);
                self.render_component(fiber);
            }
            NodeKind::Fragment(children) => self.reconcile_children(fiber, children),
            NodeKind::Empty => {}
        }
    }

    fn render_component(&mut self, fiber: FiberId) {
        let (render, props, hooks) = {
            let current = &mut self.fibers[fiber];
            let FiberKind::Function { render_fn, .. } = current.kind else {
                unreachable!()
            };
            (
                render_fn,
                current
                    .props
                    .as_ref()
                    .expect("component props missing")
                    .clone(),
                mem::take(&mut current.hooks),
            )
        };
        let mut context = Context::new(fiber, hooks, self.updates.clone(), self.wake_tx.clone());
        let child = render(&mut context, props.as_ref());
        self.fibers[fiber].hooks = context.finish();
        self.reconcile_children(fiber, vec![child]);
    }

    fn remove_subtree(&mut self, fiber: FiberId) {
        let children = self.fibers[fiber].children.clone();
        for child in children {
            self.remove_subtree(child);
        }
        let removed = self.fibers.remove(fiber).expect("fiber already removed");
        for hook in removed.hooks {
            hook.cleanup();
        }
    }

    fn commit_effects(&mut self) {
        let fibers = self.fibers.keys().collect::<Vec<_>>();
        for fiber in fibers {
            let hook_count = self.fibers[fiber].hooks.len();
            for index in 0..hook_count {
                let (cleanup, pending) = match &mut self.fibers[fiber].hooks[index] {
                    HookSlot::Effect {
                        cleanup, pending, ..
                    } if pending.is_some() => (cleanup.take(), pending.take()),
                    _ => continue,
                };
                if let Some(cleanup) = cleanup {
                    cleanup();
                }
                let next_cleanup = pending.expect("pending effect disappeared")();
                if let HookSlot::Effect { cleanup, .. } = &mut self.fibers[fiber].hooks[index] {
                    *cleanup = next_cleanup;
                }
            }
        }
    }

    fn allocate_dom_id(&mut self) -> DomId {
        let id = DomId(self.next_dom_id);
        self.next_dom_id = self
            .next_dom_id
            .checked_add(1)
            .expect("lowerer exhausted DOM identifiers");
        id
    }
}

impl Drop for Lower {
    fn drop(&mut self) {
        self.unmount();
    }
}

impl super::pipeline::Component for Lower {
    type Input = Node;
    type Output = DomNode;

    fn run(mut self, input: Receiver<Self::Input>, output: Sender<Self::Output>) {
        let wake = self.wake_rx.clone();
        loop {
            crossbeam_channel::select! {
                recv(input) -> message => {
                    let Ok(node) = message else { break; };
                    if output.send(self.lower(node)).is_err() { break; }
                }
                recv(wake) -> _ => {
                    if self.apply_updates() {
                        let Some(node) = self.root_node.clone() else { continue; };
                        self.reconcile_children(self.root, vec![node]);
                        self.commit_effects();
                        if output.send(self.root_dom()).is_err() { break; }
                    }
                }
            }
        }
    }
}
