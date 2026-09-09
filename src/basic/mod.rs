pub(crate) mod common;
pub(crate) mod context;
mod dom;
pub(crate) mod events;
pub(crate) mod props;
mod text;

pub use common::Component as ElementComponent;
pub use common::{
    __ui_apply, __ui_events, __ui_tag_names_equal, Attr, Component, Forward, Key, Node,
    PropsTransform, empty, fragment, text, view,
};
pub use context::{
    ComponentContext, Context, ContextKey, EffectResult, ElementContext, ProviderProps, Ref,
    StateSetter, create_context, provider,
};
pub use dom::{DomId, DomNode};
pub use events::{
    EventHandlers, EventListener, FocusEvent, KeyboardEvent, PasteEvent, PointerButton,
    PointerEvent, PointerEventKind, PointerId, PointerType, ResizeEvent, WheelEvent,
};
pub use props::{
    Align, Attributes, AutoProps, AxisPosition, BorderKind, BorderStyle, Dimension, DomProps,
    Edges, Fill, FillError, Justify, Layout, Overflow, OverflowScrollbarStyle, Percent,
    PercentBasis, Point, Props, ScrollAxes, ScrollProps, Style, StylePatch, TextStyle, Visibility,
    style, style_patch,
};
pub use text::{Span, Text, TextAlign, TextOverflow, TextWrap};
