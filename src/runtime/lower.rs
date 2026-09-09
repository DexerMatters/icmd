use std::{
    any::{Any, TypeId},
    collections::{HashSet, VecDeque},
    mem,
    sync::{Arc, Mutex},
};

use crossbeam_channel::{Receiver, Sender, bounded};
use slotmap::SlotMap;

use super::hooks::{FiberId, HookSlot, UpdateQueue};
use crate::basic::{
    DomId, DomNode, DomProps, Key, Node,
    common::{NodeKind, RenderFn},
    context::{Context, ContextValues},
};

type FiberArena = SlotMap<FiberId, Fiber>;

enum FiberKind {
    Root,
    Image {
        id: DomId,
        image: crate::Image,
    },
    Text {
        id: DomId,
        text: Box<crate::Text>,
    },
    Element {
        id: DomId,
        props: Box<DomProps>,
    },
    Provider {
        context: u64,
    },
    Function {
        type_id: TypeId,
        props_type_id: TypeId,
        render_fn: RenderFn,
    },
    Fragment,
}

struct Fiber {
    parent: Option<FiberId>,
    key: Option<Key>,
    kind: FiberKind,
    children: Vec<FiberId>,
    props: Option<Arc<dyn Any + Send + Sync>>,
    hooks: Vec<HookSlot>,
    provided_context: ContextValues,
}

impl Fiber {
    fn root() -> Self {
        Self {
            parent: None,
            key: None,
            kind: FiberKind::Root,
            children: Vec::new(),
            props: None,
            hooks: Vec::new(),
            provided_context: ContextValues::default(),
        }
    }
}

/// Reconciles logical fibers and emits a host-only DOM tree.
///
/// Function components, providers, and fragments are logical fibers and are
/// flattened during lowering. Empty and multi-host roots receive an internal
/// wrapper element so the commit stage always receives one host root.
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
        self.apply_updates();
        self.root_node = Some(node.clone());
        self.reconcile_children(self.root, vec![node]);
        self.commit_effects();
        self.root_dom()
    }

    fn rerender(&mut self) -> DomNode {
        let node = self
            .root_node
            .clone()
            .expect("root component is not mounted");
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
            .expect("root node was not mounted");
        let mut roots = self.build_dom(root);
        match roots.len() {
            1 => roots.pop().expect("root DOM node disappeared"),
            _ => DomNode::Element {
                id: DomId(0),
                props: DomProps::default(),
                children: roots,
            },
        }
    }

    fn build_dom(&self, fiber: FiberId) -> Vec<DomNode> {
        let current = &self.fibers[fiber];
        match &current.kind {
            FiberKind::Element { id, props } => vec![DomNode::Element {
                id: *id,
                props: (**props).clone(),
                children: current
                    .children
                    .iter()
                    .flat_map(|child| self.build_dom(*child))
                    .collect(),
            }],
            FiberKind::Function { .. } => current
                .children
                .iter()
                .flat_map(|child| self.build_dom(*child))
                .collect(),
            FiberKind::Provider { .. } => current
                .children
                .iter()
                .flat_map(|child| self.build_dom(*child))
                .collect(),
            FiberKind::Image { id, image } => vec![DomNode::Image {
                id: *id,
                image: image.clone(),
            }],
            FiberKind::Text { id, text } => vec![DomNode::Text {
                id: *id,
                text: text.as_ref().clone(),
            }],
            FiberKind::Fragment => current
                .children
                .iter()
                .flat_map(|child| self.build_dom(*child))
                .collect(),
            FiberKind::Root => Vec::new(),
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
                self.mount_node(parent, node)
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
            (FiberKind::Image { .. }, NodeKind::Image(_))
            | (FiberKind::Text { .. }, NodeKind::Text(_))
            | (FiberKind::Element { .. }, NodeKind::Element { .. })
            | (FiberKind::Fragment, NodeKind::Fragment(_)) => true,
            (
                FiberKind::Provider { context: current },
                NodeKind::Provider { context: next, .. },
            ) => current == next,
            (
                FiberKind::Function {
                    type_id,
                    props_type_id,
                    ..
                },
                NodeKind::Component {
                    type_id: next,
                    props_type_id: next_props,
                    ..
                },
            ) => type_id == next && props_type_id == next_props,
            _ => false,
        }
    }

    fn mount_node(&mut self, parent: FiberId, node: Node) -> FiberId {
        let key = node.node_key().cloned();
        let (kind, props, provided_context) = match &node.kind {
            NodeKind::Image(image) => (
                FiberKind::Image {
                    id: self.allocate_dom_id(),
                    image: image.clone(),
                },
                None,
                ContextValues::default(),
            ),
            NodeKind::Text(text) => (
                FiberKind::Text {
                    id: self.allocate_dom_id(),
                    text: Box::new(text.clone()),
                },
                None,
                ContextValues::default(),
            ),
            NodeKind::Element { dom, .. } => (
                FiberKind::Element {
                    id: self.allocate_dom_id(),
                    props: Box::new(dom.clone()),
                },
                None,
                ContextValues::default(),
            ),
            NodeKind::Provider { context, value, .. } => {
                let mut provided_context = ContextValues::default();
                provided_context.insert_erased(*context, value.clone());
                (
                    FiberKind::Provider { context: *context },
                    None,
                    provided_context,
                )
            }
            NodeKind::Component {
                type_id,
                props_type_id,
                render_fn,
                props,
            } => (
                FiberKind::Function {
                    type_id: *type_id,
                    props_type_id: *props_type_id,
                    render_fn: render_fn.clone(),
                },
                Some(props.clone()),
                ContextValues::default(),
            ),
            NodeKind::Fragment(_) => (FiberKind::Fragment, None, ContextValues::default()),
        };
        let fiber = self.fibers.insert(Fiber {
            parent: Some(parent),
            key,
            kind,
            children: Vec::new(),
            props,
            hooks: Vec::new(),
            provided_context,
        });
        match node.kind {
            NodeKind::Component { .. } => self.render_component(fiber),
            NodeKind::Element { children, .. } => self.reconcile_children(fiber, children),
            NodeKind::Provider { children, .. } => self.reconcile_children(fiber, children),
            NodeKind::Fragment(children) => self.reconcile_children(fiber, children),
            NodeKind::Image(_) | NodeKind::Text(_) => {}
        }
        fiber
    }

    fn reconcile_node(&mut self, fiber: FiberId, node: Node) {
        self.fibers[fiber].key = node.node_key().cloned();
        match node.kind {
            NodeKind::Image(image) => {
                let FiberKind::Image { image: current, .. } = &mut self.fibers[fiber].kind else {
                    unreachable!()
                };
                *current = image;
            }
            NodeKind::Text(text) => {
                let FiberKind::Text { text: current, .. } = &mut self.fibers[fiber].kind else {
                    unreachable!()
                };
                **current = text;
            }
            NodeKind::Element { dom, children } => {
                let FiberKind::Element { props, .. } = &mut self.fibers[fiber].kind else {
                    unreachable!()
                };
                **props = dom;
                self.reconcile_children(fiber, children);
            }
            NodeKind::Component {
                type_id,
                props_type_id,
                render_fn,
                props,
            } => {
                self.fibers[fiber].kind = FiberKind::Function {
                    type_id,
                    props_type_id,
                    render_fn,
                };
                self.fibers[fiber].props = Some(props);
                self.render_component(fiber);
            }
            NodeKind::Provider {
                context,
                value,
                children,
            } => {
                self.fibers[fiber].kind = FiberKind::Provider { context };
                let mut provided_context = ContextValues::default();
                provided_context.insert_erased(context, value);
                self.fibers[fiber].provided_context = provided_context;
                self.reconcile_children(fiber, children);
            }
            NodeKind::Fragment(children) => self.reconcile_children(fiber, children),
        }
    }

    fn inherited_context(&self, fiber: FiberId) -> ContextValues {
        let mut ancestors = Vec::new();
        let mut current = self.fibers[fiber].parent;
        while let Some(parent) = current {
            ancestors.push(parent);
            current = self.fibers[parent].parent;
        }

        let mut inherited = ContextValues::default();
        for ancestor in ancestors.into_iter().rev() {
            inherited.extend(&self.fibers[ancestor].provided_context);
        }
        inherited
    }

    fn render_component(&mut self, fiber: FiberId) {
        let (render, props, hooks) = {
            let current = &mut self.fibers[fiber];
            let FiberKind::Function { ref render_fn, .. } = current.kind else {
                unreachable!()
            };
            (
                render_fn.clone(),
                current
                    .props
                    .as_ref()
                    .expect("component props missing")
                    .clone(),
                mem::take(&mut current.hooks),
            )
        };
        let inherited_context = self.inherited_context(fiber);
        let mut context = Context::new(
            fiber,
            hooks,
            self.updates.clone(),
            self.wake_tx.clone(),
            inherited_context,
        );
        let child = render(&mut context, props.as_ref());
        let (hooks, provided_context) = context.finish();
        self.fibers[fiber].hooks = hooks;
        self.fibers[fiber].provided_context = provided_context;
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

impl super::pipeline::PipelineComponent for Lower {
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
                    if self.apply_updates() && self.root_node.is_some()
                        && output.send(self.rerender()).is_err()
                    {
                        break;
                    }
                }
            }
        }
    }
}
