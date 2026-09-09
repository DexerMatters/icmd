use std::{
    any::{Any, TypeId},
    fmt, ops,
    sync::Arc,
};

use crate::{
    Image,
    basic::context::{ComponentContext, ContextKey},
};

use super::{
    events::{EventHandlers, EventListener},
    props::{DomProps, Props, Style},
    text::Text,
};

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(Arc<str>);

impl Key {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Arc::from(value.into()))
    }

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

    /// Construct an explicit host element with forwarded DOM props.
    pub fn element(dom: DomProps, children: impl IntoIterator<Item = Node>) -> Self {
        Self::from_kind(NodeKind::Element {
            dom,
            children: children.into_iter().collect(),
        })
    }

    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.key = Some(key.into());
        self
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

pub fn text(content: impl Into<String>) -> Node {
    Text::new(content).into()
}

/// The explicit host component. Unlike logical function components, `view`
/// creates the renderer element that receives forwarded DOM props.
pub fn view(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    Node::element(props.dom.clone(), props.children.clone())
}

pub fn fragment<I, V>(children: I) -> Node
where
    I: IntoIterator<Item = V>,
    V: Into<Node>,
{
    children.into_iter().collect()
}

pub fn empty() -> Node {
    fragment(std::iter::empty::<Node>())
}

/// Implementation hook used by the public `ui!` macro.
#[doc(hidden)]
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

/// Implementation hook used by `ui!` to type-infer grouped event closures.
#[doc(hidden)]
pub fn __ui_events(apply: impl FnOnce(&mut EventHandlers)) -> EventHandlers {
    let mut events = EventHandlers::default();
    apply(&mut events);
    events
}

/// Implementation hook used by `ui!` for compile-time closing-tag checks.
#[doc(hidden)]
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

/// A logical function component. Implementations reconcile their returned
/// nodes; only an explicit [`view`] creates a host element.
pub trait Component<P>: 'static {
    fn prepare(&self, _props: &mut Props<P>) {}

    fn render(&self, cx: &mut ComponentContext, props: &Props<P>) -> Node;

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

    fn props(self, extra: impl Into<P>) -> Forward<Self, impl PropsTransform<P>>
    where
        Self: Sized,
        P: Clone + Send + Sync + 'static,
    {
        let extra = extra.into();
        Forward::new(self, move |props: &mut Props<P>| {
            props.user_defined = extra.clone()
        })
    }

    fn extra(
        self,
        apply: impl Fn(&mut P) + Send + Sync + 'static,
    ) -> Forward<Self, impl PropsTransform<P>>
    where
        Self: Sized,
        P: Send + Sync + 'static,
    {
        Forward::new(self, move |props: &mut Props<P>| {
            apply(&mut props.user_defined)
        })
    }

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

    fn children(self, children: impl IntoIterator<Item = Node>) -> Node
    where
        Self: Sized + Send + Sync + 'static,
        P: Send + Sync + Default + 'static,
    {
        apply_props(self, move |props| {
            props.children = children.into_iter().collect()
        })
    }

    fn child(self, child: impl Into<Node>) -> Node
    where
        Self: Sized + Send + Sync + 'static,
        P: Send + Sync + Default + 'static,
    {
        let child = child.into();
        apply_props(self, move |props| props.children.push(child))
    }

    /// Build a logical component node with default props.
    fn node(self) -> Node
    where
        Self: Sized + Send + Sync + 'static,
        P: Default + Send + Sync + 'static,
    {
        self.apply(Props::default())
    }

    /// Compatibility spelling for [`Component::node`].
    #[deprecated(note = "use `node()`; components are logical nodes")]
    fn element(self) -> Node
    where
        Self: Sized + Send + Sync + 'static,
        P: Default + Send + Sync + 'static,
    {
        self.node()
    }

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

#[derive(Clone)]
pub struct Forward<F, T> {
    component: F,
    transform: T,
}

impl<F, T> Forward<F, T> {
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

pub trait PropsTransform<P>: Send + Sync + 'static {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Attr<T> {
    #[default]
    Unset,
    Set(T),
}

impl<T> Attr<T> {
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

    pub fn set(&mut self, value: T) {
        *self = Self::Set(value);
    }

    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Unset => None,
            Self::Set(value) => Some(value),
        }
    }

    pub fn as_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Unset => None,
            Self::Set(value) => Some(value),
        }
    }

    pub fn resolve(&self, default: T) -> T
    where
        T: Clone,
    {
        self.as_ref().cloned().unwrap_or(default)
    }

    pub fn resolve_with(&self, default: impl FnOnce() -> T) -> T
    where
        T: Clone,
    {
        self.as_ref().cloned().unwrap_or_else(default)
    }

    pub fn unwrap_or_default(self) -> T
    where
        T: Default,
    {
        match self {
            Self::Unset => T::default(),
            Self::Set(value) => value,
        }
    }

    pub fn unwrap_or(self, value: T) -> T {
        match self {
            Self::Unset => value,
            Self::Set(value) => value,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Attr<U> {
        match self {
            Self::Unset => Attr::Unset,
            Self::Set(value) => Attr::Set(f(value)),
        }
    }

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
