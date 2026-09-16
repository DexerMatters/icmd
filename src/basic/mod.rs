//! The component layer: node and prop types, the rendering context, hooks,
//! DOM bookkeeping, the text layout engine, and the selection engine.
//!
//! Nothing here is public on its own; the crate root re-exports the curated
//! subset.

pub(crate) mod common;
pub(crate) mod context;
mod dom;
pub(crate) mod editor_surface;
pub(crate) mod element_ref;
pub(crate) mod events;
pub(crate) mod hooks;
pub(crate) mod lifetime;
pub(crate) mod props;
pub(crate) mod selection;
mod text;
pub(crate) mod text_layout;

pub use common::{
    __ui_apply, __ui_events, __ui_tag_names_equal, Attr, Component, Forward, Key, Node,
    PropsTransform, empty, fragment, text, view,
};
pub use context::{
    ComponentContext, ContextKey, EffectResult, Ref, StateError, StateRef, StateSetter,
    create_context,
};
pub use dom::{DomId, DomNode};
pub use element_ref::{
    ElementRect, ElementRef, ElementScrollState, ElementSnapshot, ResolvedBorderStyle,
    ResolvedElementStyle, ResolvedTextStyle,
};
pub use events::{
    EventHandlers, EventListener, EventPhase, FocusEvent, KeyboardEvent, PasteEvent, PointerButton,
    PointerEvent, PointerEventKind, PointerId, PointerType, ResizeEvent, ScrollEvent,
    TerminalFocusEvent, WheelEvent,
};
pub use lifetime::AppHandle;
pub use props::{
    Align, Attributes, AxisPosition, BorderKind, BorderStyle, Dimension, DomProps, Edges, Fill,
    FillError, Justify, Layout, Overflow, Percent, PercentBasis, Point, Props, ScrollAxes,
    ScrollDelta, ScrollOffset, ScrollbarGlyph, ScrollbarStyle, ScrollbarVisibility, Style,
    StylePatch, TextStyle, Visibility, style, style_patch,
};
pub use text::{Span, Text, TextAlign, TextOverflow, TextWrap};
#[doc(hidden)]
pub use text_layout::{TextLayoutForTest, indexed_layout_for_test};
