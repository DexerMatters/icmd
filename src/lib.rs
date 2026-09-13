mod app;
pub mod basic;
pub mod data;
pub mod elements;
mod frame_builder;
mod glyph;
mod raster;
mod runtime;
pub mod theme;
mod ui;

pub use app::{RenderError, RuntimeConfig, render};
#[doc(hidden)]
pub use basic::{TextLayoutForTest, indexed_layout_for_test};
#[doc(hidden)]
pub use elements::normalize_for_test;
pub use theme::{ThemeBuilder, ThemeProviderProps, theme_context, theme_provider};

pub use elements::{
    AlertProps, AlertVariant, BadgeProps, BadgeVariant, ButtonProps, ButtonVariant, CanvasContext,
    CanvasDraw, CanvasError, CanvasProps, CheckboxProps, ImageProps, InputProps, ProgressBarProps,
    RadioProps, RawInputAppearance, RawInputMode, RawInputProps, ScrollAreaProps, SkeletonProps,
    SpinnerProps, SwitchProps, TextClipboardAction, TextClipboardEvent, TextValueEvent,
    TextareaProps, alert, badge, blockquote, button, canvas, card, center, checkbox, code, column,
    container, divider, footer, heading, input, kbd, label, muted, paragraph, progress_bar, radio,
    raw_input, row, scroll_area, section, skeleton, spacer, spinner, switch, textarea,
};

// Canonical raster widget constructor. The historical `image` spelling is kept
// as a deprecated alias below for one transition release, because it collided
// with both the `image` facade module and the decoded-pixel `RasterImage` type.
pub use elements::image::image as raster_image;

pub use basic::{
    Align, Attr, Attributes, AxisPosition, BorderKind, BorderStyle, Component, ComponentContext,
    ContextKey, Dimension, DomId, DomNode, DomProps, Edges, EffectResult, EventHandlers,
    EventListener, Fill, FillError, FocusEvent, Forward, Justify, Key, KeyboardEvent, Layout, Node,
    Overflow, PasteEvent, Percent, PercentBasis, Point, PointerButton, PointerEvent,
    PointerEventKind, PointerId, PointerType, Props, PropsTransform, Ref, ResizeEvent, ScrollAxes,
    ScrollDelta, ScrollEvent, ScrollOffset, ScrollbarGlyph, ScrollbarStyle, ScrollbarVisibility,
    Span, StateError, StateRef, StateSetter, Style, StylePatch, TerminalFocusEvent, Text,
    TextAlign, TextOverflow, TextStyle, TextWrap, Visibility, WheelEvent, create_context, empty,
    fragment, style, style_patch, text, view,
};
pub use data::{
    Cell, CellEdit, CellError, EmojiMerging, Frame, Image, ImageError, ImageId, ImagePosition,
    MAX_GLYPH_BYTES, MAX_SURFACE_CELLS, Operation, Rect, ScreenPosition, Size,
};
pub use frame_builder::{
    BuildError, CellSurfaceHandle, FrameBuilder, RasterSurfaceHandle, SurfaceHandle,
};
pub use raster::{
    ImageAlign, ImageFit, ImageLoading, ImageMode, ImageProtocol, ImageRenderOptions, ImageSource,
    ImageUpdatePolicy, RasterImage, RasterImageError, RasterPlacement,
};

pub mod prelude {
    pub use crate::{
        Attr, Component, ComponentContext, Dimension, DomProps, Edges, EmojiMerging, ImageLoading,
        ImageSource, InputProps, Layout, Node, Props, RawInputAppearance, RawInputMode,
        RawInputProps, Ref, ScrollAreaProps, ScrollAxes, ScrollDelta, ScrollEvent, ScrollOffset,
        ScrollbarGlyph, ScrollbarStyle, ScrollbarVisibility, Style, StylePatch,
        TextClipboardAction, TextClipboardEvent, TextValueEvent, TextWrap, TextareaProps,
        ThemeProviderProps, column, heading, input, progress_bar, raw_input, row, scroll_area,
        style, style_patch, text, textarea, theme_context, theme_provider, ui, view,
    };
}

// Stable high-level facade tiers. The root re-exports above remain for source
// compatibility during the transition; new code should import from these
// modules so the implementation protocol in `advanced` is explicit.
pub mod widgets {
    // Canonical raster widget name, kept beside the other high-level widgets.
    pub use crate::elements::image::image as raster_image;

    pub use crate::{
        alert, badge, blockquote, button, canvas, card, center, checkbox, code, column, container,
        divider, empty, footer, fragment, heading, input, kbd, label, muted, paragraph,
        progress_bar, radio, raw_input, row, scroll_area, section, skeleton, spacer, spinner,
        switch, text, textarea, view,
    };
}

pub mod style {
    pub use crate::{
        Align, Attributes, AxisPosition, BorderKind, BorderStyle, Dimension, Edges, Fill,
        FillError, Justify, Layout, Overflow, Percent, PercentBasis, Point, Style, StylePatch,
        TextAlign, TextOverflow, TextStyle, TextWrap, Visibility, style, style_patch,
    };
}

pub mod events {
    pub use crate::basic::{
        EventHandlers, EventListener, EventPhase, FocusEvent, KeyboardEvent, PasteEvent,
        PointerButton, PointerEvent, PointerEventKind, PointerId, PointerType, ResizeEvent,
        ScrollEvent, TerminalFocusEvent, WheelEvent,
    };
    pub use crate::runtime::{DispatchOutcome, FocusError, FocusOutcome};
    // Raw terminal event types, for applications that read the event stream.
    pub use crossterm::event::{KeyEvent, MouseEvent};
}

pub mod image {
    pub use crate::{
        ImageAlign, ImageFit, ImageLoading, ImageMode, ImageProtocol, ImageRenderOptions,
        ImageSource, ImageUpdatePolicy, RasterImage, RasterImageError, RasterPlacement,
    };

    // Canonical raster widget name. The historical `image` spelling stays at the
    // crate root for one transition release.
    pub use crate::elements::image::image as raster_image;
}

// Explicitly advanced tier: renderer protocol, runtime ownership, and raw
// frame operations. A high-level application should not need to import this.
pub mod advanced {
    pub use crate::basic::DomNode;
    pub use crate::data::{
        Cell, CellEdit, CellError, Frame, ImageId, Operation, Rect, ScreenPosition, Size,
    };
    pub use crate::runtime::{
        ChannelRenderer, Commit, CommitConfig, ConfigError, DispatchOutcome, EventDispatcher,
        FocusError, FocusOutcome, FrameError, ImageMetrics, ImageResource, LayoutInstrument,
        LimitError, Lower, LowerError, PipelineComponent, Renderer, RendererConfig,
        RendererConfigError, ResourceLimits, Runtime, RuntimeError, RuntimeHandle, ShutdownPolicy,
        Stage, SurfaceKind, ViewportSetter, live_worker_count,
    };

    // The terminal-cell surface keeps its historical `Image` name at the root
    // for one transition release; the advanced tier names the layer explicitly.
    pub type CellSurface = crate::Image;
}

// Macro expansion support. Public only so external `ui!` expansions compile;
// nothing here is a stable interface.
#[doc(hidden)]
pub mod __private {
    // Macro expansion helpers. Reachable only through this hidden module so
    // they never pollute the crate root.
    pub use crate::basic::{__ui_apply, __ui_events, __ui_tag_names_equal};
}

// High-level alias for the common entry point.
pub use app::render as run;
