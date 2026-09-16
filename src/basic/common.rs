//! Core node vocabulary: keys, nodes, components, and tri-state attributes.
//! Owns the `Node` tree payloads, the `Component`/`PropsTransform` traits, and
//! the `Attr` tri-state value shared with style and DOM props.

use std::path::{Path, PathBuf};
use std::{
    any::{Any, TypeId},
    fmt, ops,
    sync::Arc,
};

use crate::{
    Image, ImageSource, RasterImage, RasterPlacement,
    basic::context::{ComponentContext, ContextKey},
};

use super::{
    events::{EventHandlers, EventListener},
    props::{DomProps, Props, Style},
    text::Text,
};

/// A stable, cloneable node identity used to match nodes across renders.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(Arc<str>);

impl Key {
    /// Builds a key without the intermediate `String` that an `Into<String>`
    /// bound would force for `&str` and `Arc<str>` inputs.
    pub fn new(value: impl Into<Key>) -> Self {
        value.into()
    }

    /// Returns the key as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Key").field(&self.as_str()).finish()
    }
}

impl From<&str> for Key {
    fn from(value: &str) -> Self {
        Self(Arc::from(value))
    }
}

impl From<Arc<str>> for Key {
    fn from(value: Arc<str>) -> Self {
        Self(value)
    }
}

impl From<&String> for Key {
    fn from(value: &String) -> Self {
        Self(Arc::from(value.as_str()))
    }
}

impl From<String> for Key {
    fn from(value: String) -> Self {
        Self(Arc::from(value))
    }
}

impl From<u64> for Key {
    fn from(value: u64) -> Self {
        Self(Arc::from(value.to_string()))
    }
}

/// A single node in the retained tree: a component, element, provider, or leaf.
#[derive(Clone)]
pub struct Node {
    pub(crate) kind: NodeKind,
    key: Option<Key>,
}

#[derive(Clone)]
pub(crate) enum NodeKind {
    Component {
        type_id: TypeId,
        props_type_id: TypeId,
        render_fn: RenderFn,
        props: Arc<dyn Any + Send + Sync>,
    },
    Element {
        dom: DomProps,
        children: Vec<Node>,
    },
    Provider {
        context: u64,
        value: Arc<dyn Any + Send + Sync>,
        children: Vec<Node>,
    },
    Text(Text),
    Image(Image),
    Raster(RasterPlacement),
    Fragment(Vec<Node>),
}

impl Node {
    fn from_kind(kind: NodeKind) -> Self {
        Self { kind, key: None }
    }

    pub(crate) fn provider<T>(
        context: &ContextKey<T>,
        value: T,
        children: impl IntoIterator<Item = Node>,
    ) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::from_kind(NodeKind::Provider {
            context: context.id(),
            value: Arc::new(value),
            children: children.into_iter().collect(),
        })
    }

    /// Builds an element node from DOM props and children.
    pub fn element(dom: DomProps, children: impl IntoIterator<Item = Node>) -> Self {
        Self::from_kind(NodeKind::Element {
            dom,
            children: children.into_iter().collect(),
        })
    }

    /// Attaches a reconciliation key and returns the node.
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Builds a raster-placement leaf node.
    pub fn raster(raster: RasterPlacement) -> Self {
        Self::from_kind(NodeKind::Raster(raster))
    }

    pub(crate) fn node_key(&self) -> Option<&Key> {
        self.key.as_ref()
    }
}

impl From<Image> for Node {
    fn from(image: Image) -> Self {
        Self::from_kind(NodeKind::Image(image))
    }
}

impl From<Text> for Node {
    fn from(text: Text) -> Self {
        Self::from_kind(NodeKind::Text(text))
    }
}

impl From<&str> for Node {
    fn from(text: &str) -> Self {
        Self::from_kind(NodeKind::Text(text.into()))
    }
}

impl From<String> for Node {
    fn from(text: String) -> Self {
        Self::from_kind(NodeKind::Text(text.into()))
    }
}

impl<T> FromIterator<T> for Node
where
    T: Into<Node>,
{
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_kind(NodeKind::Fragment(
            iter.into_iter().map(Into::into).collect(),
        ))
    }
}

/// Builds a text node from any string-like content.
pub fn text(content: impl Into<String>) -> Node {
    Text::new(content).into()
}

/// Renders a bare element from `props`; the default function-component shape.
pub fn view(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    Node::element(props.dom.clone(), props.children.clone())
}

/// Collects children into a single fragment node.
pub fn fragment<I, V>(children: I) -> Node
where
    I: IntoIterator<Item = V>,
    V: Into<Node>,
{
    children.into_iter().collect()
}

/// Builds a fragment node with no children.
pub fn empty() -> Node {
    fragment(std::iter::empty::<Node>())
}

#[doc(hidden)]
/// Macro support: builds a component node, applying `build` to its props to derive a key.
pub fn __ui_apply<P, C>(component: C, build: impl FnOnce(&mut Props<P>) -> Option<Key>) -> Node
where
    C: Component<P> + Send + Sync + 'static,
    P: Default + Send + Sync + 'static,
{
    let mut props = Props::default();
    let key = build(&mut props);
    let mut node = component.apply(props);
    if let Some(key) = key {
        node = node.key(key);
    }
    node
}

#[doc(hidden)]
/// Macro support: builds an event handler set by applying `apply`.
pub fn __ui_events(apply: impl FnOnce(&mut EventHandlers)) -> EventHandlers {
    let mut events = EventHandlers::default();
    apply(&mut events);
    events
}

#[doc(hidden)]
/// Macro support: compares two tag names byte-for-byte at compile time.
pub const fn __ui_tag_names_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// A node-producing type parameterized by its props type `P`.
pub trait Component<P>: 'static {
    /// Mutates props before rendering, once per construction.
    fn prepare(&self, _props: &mut Props<P>) {}

    /// Builds this component's node for `props`.
    fn render(&self, cx: &mut ComponentContext, props: &Props<P>) -> Node;

    /// Prepares and erases `props` into a component node.
    fn apply(self, props: impl Into<Props<P>>) -> Node
    where
        Self: Sized + Send + Sync + 'static,
        P: Send + Sync + 'static,
    {
        let mut props = props.into();
        self.prepare(&mut props);
        let render_fn: RenderFn = Arc::new(move |cx, erased_props| {
            let props = erased_props
                .downcast_ref::<Props<P>>()
                .expect("component props changed type during reconciliation");
            self.render(cx, props)
        });
        Node {
            kind: NodeKind::Component {
                type_id: TypeId::of::<Self>(),
                props_type_id: TypeId::of::<P>(),
                render_fn,
                props: Arc::new(props),
            },
            key: None,
        }
    }

    /// Returns a forwarder that sets fields of `P` from `extra` on each construction.
    fn props(self, extra: impl Into<P>) -> Forward<Self, impl PropsTransform<P>>
    where
        Self: Sized,
        P: Clone + Send + Sync + 'static,
    {
        let extra = extra.into();
        Forward::new(self, move |props: &mut Props<P>| {
            props.set_data(extra.clone())
        })
    }

    /// Returns a forwarder that mutates `P` through `apply` on each construction.
    fn extra(
        self,
        apply: impl Fn(&mut P) + Send + Sync + 'static,
    ) -> Forward<Self, impl PropsTransform<P>>
    where
        Self: Sized,
        P: Send + Sync + 'static,
    {
        Forward::new(self, move |props: &mut Props<P>| apply(props.data_mut()))
    }

    /// Returns a forwarder that replaces the node style.
    fn style(self, apply: impl FnOnce(&mut Style)) -> Forward<Self, impl PropsTransform<P>>
    where
        Self: Sized,
        P: Send + Sync + 'static,
    {
        let style = crate::style(apply);
        Forward::new(self, move |props: &mut Props<P>| {
            props.dom.style = style.clone()
        })
    }

    /// Returns a forwarder that registers event handlers.
    fn events(
        self,
        events: impl FnOnce(&mut EventHandlers) + Send + 'static,
    ) -> Forward<Self, impl PropsTransform<P>>
    where
        Self: Sized,
        P: Send + Sync + 'static,
    {
        Forward::new(self, EventSetter(std::sync::Mutex::new(Some(events))))
    }

    /// Applies `children` as the full child list and returns the node.
    fn children(self, children: impl IntoIterator<Item = Node>) -> Node
    where
        Self: Sized + Send + Sync + 'static,
        P: Send + Sync + Default + 'static,
    {
        apply_props(self, move |props| {
            props.children = children.into_iter().collect()
        })
    }

    /// Appends one child and returns the node.
    fn child(self, child: impl Into<Node>) -> Node
    where
        Self: Sized + Send + Sync + 'static,
        P: Send + Sync + Default + 'static,
    {
        let child = child.into();
        apply_props(self, move |props| props.children.push(child))
    }

    /// Builds the node with default props.
    fn node(self) -> Node
    where
        Self: Sized + Send + Sync + 'static,
        P: Default + Send + Sync + 'static,
    {
        self.apply(Props::default())
    }

    /// Returns a forwarder that applies `transform` to props.
    fn mapped<F>(self, transform: F) -> Forward<Self, F>
    where
        Self: Sized,
        F: Fn(&mut Props<P>) + Send + Sync + 'static,
    {
        Forward::new(self, transform)
    }
}

fn apply_props<P, C>(component: C, apply: impl FnOnce(&mut Props<P>)) -> Node
where
    C: Component<P> + Send + Sync + 'static,
    P: Send + Sync + Default + 'static,
{
    let mut props = Props::default();
    apply(&mut props);
    component.apply(props)
}

/// A component paired with a props transform applied before rendering.
#[derive(Clone)]
pub struct Forward<F, T> {
    component: F,
    transform: T,
}

impl<F, T> Forward<F, T> {
    /// Pairs a component with a props transform.
    pub fn new(component: F, transform: T) -> Self {
        Self {
            component,
            transform,
        }
    }
}

impl<P, F, T> Component<P> for Forward<F, T>
where
    P: Send + Sync + 'static,
    F: Component<P> + Send + Sync + 'static,
    T: PropsTransform<P>,
{
    fn prepare(&self, props: &mut Props<P>) {
        self.component.prepare(props);
        self.transform.apply(props);
    }

    fn render(&self, cx: &mut ComponentContext, props: &Props<P>) -> Node {
        self.component.render(cx, props)
    }
}

/// A reusable in-place mutation applied to props before rendering.
pub trait PropsTransform<P>: Send + Sync + 'static {
    /// Mutates `props` in place.
    fn apply(&self, props: &mut Props<P>);
}

impl<P, F> PropsTransform<P> for F
where
    P: Send + Sync + 'static,
    F: Fn(&mut Props<P>) + Send + Sync + 'static,
{
    fn apply(&self, props: &mut Props<P>) {
        self(props)
    }
}

struct EventSetter<F>(std::sync::Mutex<Option<F>>);

impl<P, F> PropsTransform<P> for EventSetter<F>
where
    P: Send + Sync + 'static,
    F: FnOnce(&mut EventHandlers) + Send + 'static,
{
    fn apply(&self, props: &mut Props<P>) {
        let callback = self
            .0
            .lock()
            .expect("event setter poisoned")
            .take()
            .expect("event setter applied more than once");
        callback(&mut props.dom.events);
    }
}

impl<P, F> Component<P> for F
where
    P: Send + Sync + 'static,
    F: Fn(&mut ComponentContext, &Props<P>) -> Node + Send + Sync + 'static,
{
    fn render(&self, cx: &mut ComponentContext, props: &Props<P>) -> Node {
        self(cx, props)
    }
}

/// A tri-state value: unset, or explicitly set to `T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Attr<T> {
    /// No explicit value; consumers fall back to their own default.
    #[default]
    Unset,
    /// An explicit value that overrides any default.
    Set(T),
}

impl<T> Attr<T> {
    /// Returns `true` when a value is explicitly set.
    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    pub(crate) fn overlay(&mut self, override_value: &Self)
    where
        T: Clone,
    {
        if let Self::Set(value) = override_value {
            *self = Self::Set(value.clone());
        }
    }

    /// Sets an explicit value; the `/=`, `|`, and `|=` operators desugar to this call.
    pub fn set(&mut self, value: T) {
        *self = Self::Set(value);
    }

    /// Sets `value` only when currently unset; the `|` operator's default behaviour.
    pub fn set_default(&mut self, value: T) {
        if matches!(self, Self::Unset) {
            *self = Self::Set(value);
        }
    }

    /// Returns to the unset state, which differs from an explicit value.
    pub fn clear(&mut self) {
        *self = Self::Unset;
    }

    /// Returns `true` for an explicitly set value; the inverse of `Unset`.
    pub fn is_explicit(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    /// Returns a copy, preserving the unset state.
    pub fn cloned(&self) -> Attr<T>
    where
        T: Clone,
    {
        match self {
            Self::Unset => Self::Unset,
            Self::Set(value) => Self::Set(value.clone()),
        }
    }

    /// Returns the set value, or `None` when unset.
    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Unset => None,
            Self::Set(value) => Some(value),
        }
    }

    /// Returns the set value mutably, or `None` when unset.
    pub fn as_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Unset => None,
            Self::Set(value) => Some(value),
        }
    }

    /// Returns the set value, or `default` when unset.
    pub fn resolve(&self, default: T) -> T
    where
        T: Clone,
    {
        self.as_ref().cloned().unwrap_or(default)
    }

    /// Returns the set value, or the result of `default` when unset.
    pub fn resolve_with(&self, default: impl FnOnce() -> T) -> T
    where
        T: Clone,
    {
        self.as_ref().cloned().unwrap_or_else(default)
    }

    /// Returns the set value, or `T::default()` when unset.
    pub fn unwrap_or_default(self) -> T
    where
        T: Default,
    {
        match self {
            Self::Unset => T::default(),
            Self::Set(value) => value,
        }
    }

    /// Returns the set value, or `value` when unset.
    pub fn unwrap_or(self, value: T) -> T {
        match self {
            Self::Unset => value,
            Self::Set(value) => value,
        }
    }

    /// Maps the set value with `f`, leaving `Unset` unchanged.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Attr<U> {
        match self {
            Self::Unset => Attr::Unset,
            Self::Set(value) => Attr::Set(f(value)),
        }
    }

    /// Chains `f` on the set value, leaving `Unset` unchanged.
    pub fn and_then<U>(self, f: impl FnOnce(T) -> Attr<U>) -> Attr<U> {
        match self {
            Self::Unset => Attr::Unset,
            Self::Set(value) => f(value),
        }
    }
}

impl<T> From<Option<T>> for Attr<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::Set(value),
            None => Self::Unset,
        }
    }
}

impl<T> From<Attr<T>> for Option<T> {
    fn from(value: Attr<T>) -> Self {
        match value {
            Attr::Unset => None,
            Attr::Set(value) => Some(value),
        }
    }
}

impl<E, F> ops::DivAssign<F> for Attr<EventListener<E>>
where
    F: FnMut(E) + Send + 'static,
{
    fn div_assign(&mut self, rhs: F) {
        match self {
            Self::Unset => *self = Self::Set(EventListener::new(rhs)),
            Self::Set(value) => {
                value.callback = Arc::new(std::sync::Mutex::new(Box::new(rhs)));
            }
        }
    }
}

impl<T> ops::DivAssign<T> for Attr<T> {
    fn div_assign(&mut self, rhs: T) {
        match self {
            Self::Unset => *self = Self::Set(rhs),
            Self::Set(value) => *value = rhs,
        }
    }
}

impl ops::DivAssign<RasterImage> for Attr<ImageSource> {
    fn div_assign(&mut self, rhs: RasterImage) {
        *self = Self::Set(ImageSource::from(rhs));
    }
}

impl From<ImageSource> for Attr<ImageSource> {
    fn from(value: ImageSource) -> Self {
        Self::Set(value)
    }
}

impl From<RasterImage> for Attr<ImageSource> {
    fn from(value: RasterImage) -> Self {
        Self::Set(value.into())
    }
}

impl From<PathBuf> for Attr<ImageSource> {
    fn from(value: PathBuf) -> Self {
        Self::Set(value.into())
    }
}

impl From<String> for Attr<ImageSource> {
    fn from(value: String) -> Self {
        Self::Set(value.into())
    }
}

impl From<&str> for Attr<ImageSource> {
    fn from(value: &str) -> Self {
        Self::Set(value.into())
    }
}

impl From<&Path> for Attr<ImageSource> {
    fn from(value: &Path) -> Self {
        Self::Set(value.into())
    }
}

impl ops::DivAssign<PathBuf> for Attr<ImageSource> {
    fn div_assign(&mut self, rhs: PathBuf) {
        *self = Self::Set(ImageSource::file(rhs));
    }
}

impl ops::DivAssign<&Path> for Attr<ImageSource> {
    fn div_assign(&mut self, rhs: &Path) {
        *self = Self::Set(ImageSource::file(rhs));
    }
}

impl ops::DivAssign<String> for Attr<ImageSource> {
    fn div_assign(&mut self, rhs: String) {
        *self = Self::Set(ImageSource::file(rhs));
    }
}

impl ops::DivAssign<&str> for Attr<ImageSource> {
    fn div_assign(&mut self, rhs: &str) {
        *self = Self::Set(ImageSource::file(rhs));
    }
}

impl ops::DivAssign<&str> for Attr<String> {
    fn div_assign(&mut self, rhs: &str) {
        *self /= rhs.to_owned();
    }
}

impl<T> ops::BitOrAssign<T> for Attr<T> {
    fn bitor_assign(&mut self, rhs: T) {
        if let Self::Unset = self {
            *self = Self::Set(rhs)
        }
    }
}

impl<T: Clone> ops::BitOr<T> for Attr<T> {
    type Output = T;

    fn bitor(self, rhs: T) -> Self::Output {
        match self {
            Self::Unset => rhs,
            Self::Set(value) => value,
        }
    }
}

impl<T: Clone> ops::BitOr<T> for &Attr<T> {
    type Output = T;

    fn bitor(self, rhs: T) -> Self::Output {
        self.as_ref().cloned().unwrap_or(rhs)
    }
}

impl ops::BitOr<&str> for Attr<String> {
    type Output = String;

    fn bitor(self, rhs: &str) -> Self::Output {
        self | rhs.to_owned()
    }
}

impl ops::BitOr<&str> for &Attr<String> {
    type Output = String;

    fn bitor(self, rhs: &str) -> Self::Output {
        self.as_ref().cloned().unwrap_or_else(|| rhs.to_owned())
    }
}

pub(crate) type RenderFn = Arc<dyn Fn(&mut ComponentContext, &dyn Any) -> Node + Send + Sync>;
