use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use crossterm::event::{KeyEvent, MouseEvent};

use crate::{Node, ScreenPosition, Size};

/// A clonable, thread-safe callback stored on a DOM property.
///
/// Listener equality is based on callback identity rather than closure
/// contents, since closures do not expose a meaningful value equality.
#[derive(fmt_derive::Debug)]
pub struct EventListener<E> {
    callback: Arc<dyn Fn(E) + Send + Sync + 'static>,
}

impl<E> EventListener<E> {
    pub fn new(callback: impl Fn(E) + Send + Sync + 'static) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    pub fn call(&self, event: E) {
        (self.callback)(event);
    }
}

impl<E> Clone for EventListener<E> {
    fn clone(&self) -> Self {
        Self {
            callback: self.callback.clone(),
        }
    }
}

impl<E> PartialEq for EventListener<E> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.callback, &other.callback)
    }
}

impl<E> Eq for EventListener<E> {}

/// The kind of terminal focus transition delivered to a focus listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusEvent {
    Gained,
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    None,
    Horizontal,
    Vertical,
    Absolute,
}
impl Default for Layout {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Visible,
    Invisible,
    Hidden,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Visible
    }
}

/// Properties which survive component lowering and appear on a DOM element.
/// Layout-specific additions such as margin and padding belong here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DomProps {
    // Layout properties
    pub layout: Layout,
    pub z_index: i32,
    pub visibility: Visibility,
    pub margin: (u16, u16, u16, u16),  // top, right, bottom, left
    pub padding: (u16, u16, u16, u16), // top, right, bottom, left

    // Event listeners
    pub mouse_event: Option<EventListener<MouseEvent>>,
    pub keyboard_event: Option<EventListener<KeyEvent>>,
    pub resize_event: Option<EventListener<Size>>,
    pub focus_event: Option<EventListener<FocusEvent>>,
    pub paste_event: Option<EventListener<String>>,
}

/// The complete input to a component: DOM properties, explicit children, and
/// application-specific data.
#[derive(Clone)]
pub struct Props<T> {
    pub dom: DomProps,
    pub children: Vec<Node>,
    pub user_defined: T,
}

impl<T> Props<T> {
    pub fn new(user_defined: T) -> Self {
        Self {
            dom: DomProps::default(),
            children: Vec::new(),
            user_defined,
        }
    }

    pub fn on_mouse_event(mut self, listener: impl Fn(MouseEvent) + Send + Sync + 'static) -> Self {
        self.dom.mouse_event = Some(EventListener::new(listener));
        self
    }

    pub fn on_keyboard_event(
        mut self,
        listener: impl Fn(KeyEvent) + Send + Sync + 'static,
    ) -> Self {
        self.dom.keyboard_event = Some(EventListener::new(listener));
        self
    }

    pub fn on_resize_event(mut self, listener: impl Fn(Size) + Send + Sync + 'static) -> Self {
        self.dom.resize_event = Some(EventListener::new(listener));
        self
    }

    pub fn on_focus_event(mut self, listener: impl Fn(FocusEvent) + Send + Sync + 'static) -> Self {
        self.dom.focus_event = Some(EventListener::new(listener));
        self
    }

    pub fn on_paste_event(mut self, listener: impl Fn(String) + Send + Sync + 'static) -> Self {
        self.dom.paste_event = Some(EventListener::new(listener));
        self
    }

    pub fn with_children(mut self, children: impl IntoIterator<Item = Node>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    pub fn child(mut self, child: impl Into<Node>) -> Self {
        self.children.push(child.into());
        self
    }

    pub fn extra(&self) -> &T {
        &self.user_defined
    }

    pub fn extra_mut(&mut self) -> &mut T {
        &mut self.user_defined
    }

    pub fn with_extra<U>(self, user_defined: U) -> Props<U> {
        Props {
            dom: self.dom,
            children: self.children,
            user_defined,
        }
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Props<U> {
        let Props {
            dom,
            children,
            user_defined,
        } = self;
        Props {
            dom,
            children,
            user_defined: map(user_defined),
        }
    }

    pub fn into_parts(self) -> (DomProps, Vec<Node>, T) {
        (self.dom, self.children, self.user_defined)
    }
}

impl<T> Deref for Props<T> {
    type Target = DomProps;

    fn deref(&self) -> &Self::Target {
        &self.dom
    }
}

impl<T> DerefMut for Props<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.dom
    }
}

impl<T> From<T> for Props<T> {
    fn from(user_defined: T) -> Self {
        Self::new(user_defined)
    }
}

impl<T: Default> Default for Props<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
