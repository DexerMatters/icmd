mod app;
pub mod basic;
pub mod data;
pub mod elements;
mod raster;
mod runtime;
pub mod theme;
mod ui;

pub use app::{RenderError, RuntimeConfig, render};
pub use theme::{ThemeProviderProps, theme_context, theme_provider};

pub use elements::{
    AlertProps, AlertVariant, BadgeProps, BadgeVariant, CanvasContext, CanvasDraw, CanvasError,
    CanvasProps, CheckboxProps, ImageProps, InputProps, ProgressBarProps, RadioProps,
    ScrollAreaProps, SkeletonProps, SpinnerProps, SwitchProps, TextAreaProps, TextClipboardAction,
    TextClipboardEvent, TextEditHandler, TextValueEvent, alert, badge, blockquote, button, canvas,
    card, center, checkbox, code, column, container, divider, footer, heading, image, input, kbd,
    label, muted, paragraph, progress_bar, radio, row, scroll_area, section, skeleton, spacer,
    spinner, switch, text_area,
};

pub use basic::{
    __ui_apply, __ui_events, __ui_tag_names_equal, Align, Attr, Attributes, AxisPosition,
    BorderKind, BorderStyle, Component, ComponentContext, ContextKey, Dimension, DomId, DomNode,
    DomProps, Edges, EffectResult, EventHandlers, EventListener, Fill, FillError, FocusEvent,
    Forward, Justify, Key, KeyboardEvent, Layout, Node, Overflow, PasteEvent, Percent,
    PercentBasis, Point, PointerButton, PointerEvent, PointerEventKind, PointerId, PointerType,
    Props, PropsTransform, ProviderProps, Ref, ResizeEvent, ScrollAxes, ScrollDelta, ScrollEvent,
    ScrollOffset, ScrollbarGlyph, ScrollbarStyle, ScrollbarVisibility, Span, StateSetter, Style,
    StylePatch, Text, TextAlign, TextOverflow, TextStyle, TextWrap, Visibility, WheelEvent,
    create_context, empty, fragment, provider, style, style_patch, text, view,
};
pub use data::{
    Cell, CellEdit, CellError, Frame, Image, ImageError, ImageId, ImagePosition, MAX_GLYPH_BYTES,
    MAX_SURFACE_CELLS, Operation, Rect, ScreenPosition, Size,
};
pub use raster::{
    ImageAlign, ImageFit, ImageLoading, ImageMode, ImageProtocol, ImageRenderOptions, ImageSource,
    ImageUpdatePolicy, RasterImage, RasterImageError, RasterPlacement,
};
pub use runtime::{
    Commit, EventDispatcher, FrameError, Lower, PipelineComponent, Renderer, RendererConfig,
    Runtime, ViewportSetter,
};

pub mod prelude {
    pub use crate::{
        Attr, Component, ComponentContext, Dimension, DomProps, Edges, ImageLoading, ImageSource,
        InputProps, Layout, Node, Props, Ref, ScrollAreaProps, ScrollAxes, ScrollDelta,
        ScrollEvent, ScrollOffset, ScrollbarGlyph, ScrollbarStyle, ScrollbarVisibility, Style,
        StylePatch, TextAreaProps, TextClipboardAction, TextClipboardEvent, TextEditHandler,
        TextValueEvent, TextWrap, ThemeProviderProps, column, heading, input, progress_bar,
        provider, row, scroll_area, style, style_patch, text, text_area, theme_context,
        theme_provider, ui, view,
    };
}
