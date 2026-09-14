use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet, VecDeque},
    error::Error,
    fmt, mem,
    sync::{Arc, Mutex},
};

use crossbeam_channel::{Receiver, Sender, bounded};
use slotmap::SlotMap;

use super::limits::{ConfigError, ResourceLimits};
use super::pipeline::RuntimeError;
use crate::basic::hooks::{FiberId, HookSlot, UpdateQueue};
use crate::basic::{
    DomId, DomNode, DomProps, Key, Node,
    common::{NodeKind, RenderFn},
    context::{ComponentContext, ContextValues},
};

type FiberArena = SlotMap<FiberId, Fiber>;

// Lowering is iterative over the logical tree, but a public tree can still be
// arbitrarily deep or large. Both ceilings are checked before the recursive
// work they bound, and reported as typed errors instead of aborting a worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    TreeTooDeep {
        limit: usize,
        observed_at_least: usize,
    },
    TreeTooLarge {
        limit: usize,
        observed: usize,
    },
    // Two siblings shared one key. This is a caller error, not an internal
    // invariant, so it is reported instead of panicking the worker.
    DuplicateKey {
        key: String,
    },
    // The configured resource policy was rejected. Reporting it as a typed
    // stage error means an invalid limit cannot be applied silently or panic
    // the worker.
    Config {
        detail: String,
    },
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TreeTooDeep {
                limit,
                observed_at_least,
            } => write!(
                f,
                "logical tree depth exceeds the limit of {limit} (at least {observed_at_least})"
            ),
            Self::TreeTooLarge { limit, observed } => write!(
                f,
                "logical tree node count exceeds the limit of {limit} (at least {observed})"
            ),
            Self::DuplicateKey { key } => {
                write!(f, "duplicate sibling key {key:?}")
            }
            Self::Config { detail } => {
                write!(f, "invalid resource policy: {detail}")
            }
        }
    }
}

impl Error for LowerError {}

enum FiberKind {
    Root,
    Image {
        id: DomId,
        image: crate::Image,
    },
    Raster {
        id: DomId,
        raster: crate::RasterPlacement,
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
    // Logical depth from the root, kept on the fiber so the depth limit is a
    // constant-time check instead of an ancestor walk per mount.
    depth: usize,
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
            depth: 0,
            key: None,
            kind: FiberKind::Root,
            children: Vec::new(),
            props: None,
            hooks: Vec::new(),
            provided_context: ContextValues::default(),
        }
    }
}

pub struct Lower {
    fibers: FiberArena,
    root: FiberId,
    // Only whether a root is mounted matters; retaining the whole logical root
    // duplicated the caller's tree for no observable behavior.
    mounted: bool,
    live_nodes: usize,
    limits: ResourceLimits,
    // Set when a limit rejects a mount. Checked by `lower`/`rerender` so the
    // caller receives one typed error instead of a silently truncated tree.
    pending_error: Option<LowerError>,
    next_dom_id: u64,
    updates: UpdateQueue,
    wake_tx: Sender<()>,
    wake_rx: Receiver<()>,
    effect_fibers: Vec<FiberId>,
}

impl Default for Lower {
    fn default() -> Self {
        Self::new()
    }
}

// Iterative, allocation-bounded pre-pass over the logical tree. Component
// children are not materialized here; expansion is rechecked by the live-node
// counter during lowering, so a recursive component cannot escape the ceiling.
fn validate_tree(root: &Node, limits: &ResourceLimits) -> Result<(), LowerError> {
    let mut stack: Vec<(&Node, usize)> = vec![(root, 1)];
    let mut count = 0usize;
    while let Some((node, depth)) = stack.pop() {
        count = count.saturating_add(1);
        if depth > limits.max_tree_depth {
            return Err(LowerError::TreeTooDeep {
                limit: limits.max_tree_depth,
                observed_at_least: depth,
            });
        }
        if count > limits.max_nodes {
            return Err(LowerError::TreeTooLarge {
                limit: limits.max_nodes,
                observed: count,
            });
        }
        match &node.kind {
            NodeKind::Element { children, .. }
            | NodeKind::Provider { children, .. }
            | NodeKind::Fragment(children) => {
                for child in children {
                    stack.push((child, depth.saturating_add(1)));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

impl Lower {
    fn new() -> Self {
        let mut fibers = FiberArena::with_key();
        let root = fibers.insert(Fiber::root());
        let (wake_tx, wake_rx) = bounded(1);
        Self {
            fibers,
            root,
            mounted: false,
            live_nodes: 0,
            limits: ResourceLimits::default(),
            pending_error: None,
            next_dom_id: 1,
            updates: Arc::new(Mutex::new(VecDeque::new())),
            wake_tx,
            wake_rx,
            effect_fibers: Vec::new(),
        }
    }

    pub fn with_limits(limits: ResourceLimits) -> Self {
        let mut lower = Self::new();
        lower.limits = limits;
        lower
    }

    // Construction that rejects an invalid policy up front, for callers that
    // would rather handle the error than have it surface on the first frame.
    pub fn try_with_limits(limits: ResourceLimits) -> Result<Self, ConfigError> {
        limits.validate()?;
        Ok(Self::with_limits(limits))
    }

    fn lower(&mut self, node: Node) -> Result<DomNode, LowerError> {
        // A policy that never validated is refused here, so an out-of-range
        // limit is a typed error rather than a silently unusable bound.
        self.limits.validate().map_err(|error| LowerError::Config {
            detail: error.to_string(),
        })?;
        let _ = self.apply_updates();
        // Validate the incoming root iteratively, before any recursive
        // traversal, so an over-deep tree cannot reach the recursive code.
        validate_tree(&node, &self.limits)?;
        self.pending_error = None;
        self.mounted = true;
        self.reconcile_children(self.root, vec![node]);
        if let Some(error) = self.pending_error.take() {
            return Err(error);
        }
        self.commit_effects();
        Ok(self.root_dom())
    }

    fn rerender(&mut self, dirty: Vec<FiberId>) -> Result<DomNode, LowerError> {
        self.pending_error = None;
        let requested: HashSet<_> = dirty.into_iter().collect();
        let roots: Vec<_> = requested
            .iter()
            .copied()
            .filter(|fiber| {
                let mut parent = self.fibers.get(*fiber).and_then(|value| value.parent);
                while let Some(current) = parent {
                    if requested.contains(&current) {
                        return false;
                    }
                    parent = self.fibers.get(current).and_then(|value| value.parent);
                }
                true
            })
            .collect();
        for fiber in roots {
            if self
                .fibers
                .get(fiber)
                .is_some_and(|value| matches!(value.kind, FiberKind::Function { .. }))
            {
                self.render_component(fiber);
            }
        }
        if let Some(error) = self.pending_error.take() {
            return Err(error);
        }
        self.commit_effects();
        Ok(self.root_dom())
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
                id: DomId::root(),
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
            FiberKind::Raster { id, raster } => vec![DomNode::Raster {
                id: *id,
                raster: raster.clone(),
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

    fn apply_updates(&mut self) -> Vec<FiberId> {
        while self.wake_rx.try_recv().is_ok() {}
        let updates = {
            let mut queue = self.updates.lock().expect("state update queue poisoned");
            queue.drain(..).collect::<Vec<_>>()
        };
        let mut applied = Vec::new();
        for update in updates {
            let Some(fiber) = self.fibers.get_mut(update.fiber) else {
                continue;
            };
            let Some(HookSlot::State(state)) = fiber.hooks.get_mut(update.hook) else {
                continue;
            };
            (update.apply)(state.as_mut());
            applied.push(update.fiber);
        }
        applied
    }

    fn reconcile_children(&mut self, parent: FiberId, nodes: Vec<Node>) {
        let old = self.fibers[parent].children.clone();
        let mut used = HashSet::new();
        let mut children = Vec::with_capacity(nodes.len());
        let mut seen_keys = HashSet::new();

        // Per-parent key index built in one pass, so matching a keyed new child
        // is a lookup rather than a scan of every old sibling. Without this,
        // reordering N keyed children costs O(N^2) comparisons.
        // The key is cloned into the index so reconciliation can mutate the
        // fiber arena while the index is alive.
        let mut keyed_old: HashMap<Key, FiberId> = HashMap::with_capacity(old.len());
        for candidate in &old {
            if let Some(key) = self.fibers[*candidate].key.clone() {
                keyed_old.entry(key).or_insert(*candidate);
            }
        }

        let child_depth = self.fibers[parent].depth.saturating_add(1);
        for (index, node) in nodes.into_iter().enumerate() {
            if self.pending_error.is_some() {
                break;
            }
            if child_depth > self.limits.max_tree_depth {
                self.pending_error = Some(LowerError::TreeTooDeep {
                    limit: self.limits.max_tree_depth,
                    observed_at_least: child_depth,
                });
                break;
            }
            if self.live_nodes >= self.limits.max_nodes {
                self.pending_error = Some(LowerError::TreeTooLarge {
                    limit: self.limits.max_nodes,
                    observed: self.live_nodes.saturating_add(1),
                });
                break;
            }
            if let Some(key) = node.node_key()
                && !seen_keys.insert(key.clone())
            {
                // Deterministic first-wins: report the duplicate and skip it
                // rather than panicking or silently migrating state.
                self.pending_error = Some(LowerError::DuplicateKey {
                    key: key.as_str().to_string(),
                });
                continue;
            }
            let matched = if let Some(key) = node.node_key() {
                keyed_old.get(key).copied().filter(|candidate| {
                    !used.contains(candidate) && self.compatible(*candidate, &node)
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
            | (FiberKind::Raster { .. }, NodeKind::Raster(_))
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
            NodeKind::Raster(raster) => (
                FiberKind::Raster {
                    id: self.allocate_dom_id(),
                    raster: raster.clone(),
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
        self.live_nodes = self.live_nodes.saturating_add(1);
        super::metrics::note_node_lowered();
        let fiber = self.fibers.insert(Fiber {
            parent: Some(parent),
            depth: self.fibers[parent].depth.saturating_add(1),
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
            NodeKind::Image(_) | NodeKind::Raster(_) | NodeKind::Text(_) => {}
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
            NodeKind::Raster(raster) => {
                let FiberKind::Raster {
                    raster: current, ..
                } = &mut self.fibers[fiber].kind
                else {
                    unreachable!()
                };
                *current = raster;
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
        let mut context = ComponentContext::new(
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
        self.effect_fibers.push(fiber);
        self.reconcile_children(fiber, vec![child]);
    }

    fn remove_subtree(&mut self, fiber: FiberId) {
        let children = self.fibers[fiber].children.clone();
        for child in children {
            self.remove_subtree(child);
        }
        let removed = self.fibers.remove(fiber).expect("fiber already removed");
        self.live_nodes = self.live_nodes.saturating_sub(1);
        for hook in removed.hooks {
            hook.cleanup();
        }
    }

    fn commit_effects(&mut self) {
        let fibers = mem::take(&mut self.effect_fibers);
        let mut seen = HashSet::new();
        for fiber in fibers {
            if !seen.insert(fiber) {
                continue;
            }
            let Some(current) = self.fibers.get(fiber) else {
                continue;
            };
            let hook_count = current.hooks.len();
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
        let id = DomId::new(self.next_dom_id);
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

    const STAGE: super::pipeline::Stage = super::pipeline::Stage::Lower;

    fn run(
        mut self,
        input: Receiver<Self::Input>,
        output: Sender<Self::Output>,
        errors: Sender<RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let wake = self.wake_rx.clone();
        loop {
            crossbeam_channel::select! {
                recv(input) -> message => {
                    let Ok(node) = message else { break; };
                    // A rejected tree is a typed, recoverable error: the caller
                    // is told exactly which budget was exceeded and the worker
                    // keeps serving later valid roots.
                    match self.lower(node) {
                        Ok(dom) => {
                            if output.send(dom).is_err() { break; }
                        }
                        Err(error) => {
                            if errors.send(RuntimeError::Lower(error)).is_err() { break; }
                        }
                    }
                }
                recv(wake) -> _ => {
                    let dirty = self.apply_updates();
                    if !dirty.is_empty() && self.mounted {
                        match self.rerender(dirty) {
                            Ok(dom) => {
                                if output.send(dom).is_err() { break; }
                            }
                            Err(error) => {
                                if errors.send(RuntimeError::Lower(error)).is_err() { break; }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
