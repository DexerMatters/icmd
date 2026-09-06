use std::{
    any::{Any, TypeId},
    fmt,
    sync::Arc,
};

use crate::{Image, basic::context::Context};

use super::props::{DomProps, Props};

/// A stable identity used when children may be inserted, removed, or reordered.
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

/// A declarative node consumed by the reactive lowerer.
#[derive(Clone)]
pub struct Node {
    pub(crate) kind: NodeKind,
    key: Option<Key>,
}

#[derive(Clone)]
pub(crate) enum NodeKind {
    Component {
        type_id: TypeId,
        render_fn: fn(&mut Context, &dyn Any) -> Node,
        dom: DomProps,
        props: Arc<dyn Any + Send + Sync>,
    },
    Image(Image),
    Fragment(Vec<Node>),
    Empty,
}

impl Node {
    pub fn component<C: Component>(props: impl Into<Props<C::Props>>) -> Self {
        fn render_fn<C: Component>(cx: &mut Context, props: &dyn Any) -> Node {
            let props = props
                .downcast_ref::<Props<C::Props>>()
                .expect("component props changed type during reconciliation");
            C::render(cx, props)
        }

        let props = props.into();
        Self {
            kind: NodeKind::Component {
                type_id: TypeId::of::<C>(),
                render_fn: render_fn::<C>,
                dom: props.dom.clone(),
                props: Arc::new(props),
            },
            key: None,
        }
    }

    pub fn image(image: Image) -> Self {
        Self {
            kind: NodeKind::Image(image),
            key: None,
        }
    }

    pub fn fragment(children: impl IntoIterator<Item = Node>) -> Self {
        Self {
            kind: NodeKind::Fragment(children.into_iter().collect()),
            key: None,
        }
    }

    pub fn empty() -> Self {
        Self {
            kind: NodeKind::Empty,
            key: None,
        }
    }

    /// Assigns an identity to this node. Keys only need to be unique among siblings.
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
        Self::image(image)
    }
}

/// A React-style function component. Persistent values belong in hooks on
/// [`Context`]; shared DOM properties and children are available through props.
pub trait Component: 'static {
    type Props: Send + Sync + 'static;

    fn render(cx: &mut Context, props: &Props<Self::Props>) -> Node;
}
