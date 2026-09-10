mod app;
pub mod basic;
pub mod data;
pub mod elements;
mod runtime;
pub mod theme;
mod ui;

pub use app::{RenderError, RuntimeConfig, render};
pub use theme::{ThemeProviderProps, theme_context, theme_provider};

pub use elements::{
    AlertProps, AlertVariant, BadgeProps, BadgeVariant, CanvasContext, CanvasDraw, CanvasError,
    CanvasProps, CheckboxProps, ProgressBarProps, RadioProps, ScrollBarProps, ScrollbarOrientation,
    ScrollbarProps, SkeletonProps, SpinnerProps, SwitchProps, alert, badge, blockquote, button,
    canvas, card, center, checkbox, code, column, container, divider, footer, hbox, header,
    heading, input, kbd, label, muted, paragraph, progress_bar, progressbar, radio, row, scrollbar,
    section, skeleton, spacer, spinner, switch, title, vbox,
};

pub use basic::{
    __ui_apply, __ui_events, __ui_tag_names_equal, Align, Attr, Attributes, AxisPosition,
    BorderKind, BorderStyle, Component, ComponentContext, Context, ContextKey, Dimension, DomId,
    DomNode, DomProps, Edges, EffectResult, ElementComponent, ElementContext, EventHandlers,
    EventListener, Fill, FillError, FocusEvent, Forward, Justify, Key, KeyboardEvent, Layout, Node,
    Overflow, OverflowScrollbarStyle, PasteEvent, Percent, PercentBasis, Point, PointerButton,
    PointerEvent, PointerEventKind, PointerId, PointerType, Props, PropsTransform, ProviderProps,
    Ref as ElementRef, ResizeEvent, ScrollEvent, ScrollStyle, Span, StateSetter, Style, StylePatch,
    Text, TextAlign, TextOverflow, TextStyle, TextWrap, Visibility, WheelEvent, create_context,
    empty, fragment, provider, style, style_patch, text, view,
};
pub use data::{
    Cell, CellEdit, CellError, Frame, Image, ImageError, ImageId, ImagePosition, MAX_GLYPH_BYTES,
    MAX_SURFACE_CELLS, Operation, Rect, ScreenPosition, Size,
};
pub use runtime::PipelineComponent as RuntimeComponent;
pub use runtime::{
    Commit, EventDispatcher, FrameError, Lower, PipelineComponent, Renderer, Runtime,
    ViewportSetter,
};

pub mod prelude {
    pub use crate::{
        Attr, Component, ComponentContext, Dimension, DomProps, Edges, ElementComponent,
        ElementContext, Layout, Node, Props, Style, StylePatch, ThemeProviderProps, provider,
        style, style_patch, text, theme_context, theme_provider, ui, view,
    };
}
