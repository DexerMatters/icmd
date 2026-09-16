//! A retained, terminal-native UI framework.
//!
//! An application builds one tree of [`Node`]s from ordinary components and
//! hands it to [`render`]. The runtime lowers the tree into a DOM, commits it to
//! a frame, and paints only the cells that changed; terminal input is routed
//! back through the tree as [`events`].
//!
//! ```no_run
//! # use icmd::{Component, ComponentContext, Node, Props};
//! fn app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
//!     icmd::text("Hello, terminal")
//! }
//!
//! # fn main() -> Result<(), icmd::RenderError> {
//! icmd::render(app.apply(()), icmd::RuntimeConfig::default())?;
//! # Ok(())
//! # }
//! ```
//!
//! The example is marked to compile but not run: a session needs a real
//! terminal.
//!
//! # Where things live
//!
//! - The crate root holds the vocabulary an application names directly:
//!   components, [`Node`], [`Props`], style, layout, text, and the event types.
//! - [`widgets`] names the built-in widgets.
//! - [`events`] adds the live event-stream types on top of the root event types.
//! - [`theme`] owns palettes and providers.
//! - [`advanced`] exposes the renderer protocol and runtime ownership; a
//!   high-level application never needs it.
#![deny(missing_docs)]

mod app;
mod basic;
mod data;
mod elements;
mod frame_builder;
mod glyph;
pub mod lifecycle;
mod raster;
mod runtime;
pub mod theme;
mod ui;

pub use app::{RenderError, RuntimeConfig, render, render_with};
pub use lifecycle::{AppLifecycle, AppPhase, AppSession, ExitReason};
pub use theme::{ThemeBuilder, ThemeProviderProps, theme_context, theme_provider};

pub use elements::{
    AlertProps, AlertVariant, BadgeProps, BadgeVariant, ButtonProps, ButtonVariant, CanvasContext,
    CanvasDraw, CanvasError, CanvasProps, CheckboxProps, ImageProps, InputProps, LinkProps,
    ProgressBarProps, RadioProps, RawInputAppearance, RawInputMode, RawInputProps, ScrollAreaProps,
    SelectionAreaProps, SkeletonProps, SpinnerProps, SwitchProps, TextClipboardAction,
    TextClipboardEvent, TextSelectionEvent, TextValueEvent, TextareaProps, alert, badge,
    blockquote, button, canvas, card, center, checkbox, code, column, container, divider, footer,
    heading, input, kbd, label, link, muted, paragraph, progress_bar, radio, raw_input, row,
    scroll_area, section, selection_area, skeleton, spacer, spinner, switch, textarea,
};

pub use basic::{
    Align, AppHandle, Attr, Attributes, AxisPosition, BorderKind, BorderStyle, Component,
    ComponentContext, ContextKey, Dimension, DomId, DomNode, DomProps, Edges, EffectResult,
    EventHandlers, EventListener, Fill, FillError, FocusEvent, Forward, Justify, Key,
    KeyboardEvent, Layout, Node, Overflow, PasteEvent, Percent, PercentBasis, Point, PointerButton,
    PointerEvent, PointerEventKind, PointerId, PointerType, Props, PropsTransform, Ref,
    ResizeEvent, ScrollAxes, ScrollDelta, ScrollEvent, ScrollOffset, ScrollbarGlyph,
    ScrollbarStyle, ScrollbarVisibility, Span, StateError, StateRef, StateSetter, Style,
    StylePatch, TerminalFocusEvent, Text, TextAlign, TextOverflow, TextStyle, TextWrap, Visibility,
    WheelEvent, create_context, empty, fragment, style, style_patch, text, view,
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

/// Common imports for an application: components, layout keywords, and the
/// widgets most programs name.
pub mod prelude {
    pub use crate::{
        AppHandle, AppLifecycle, AppPhase, Attr, Component, ComponentContext, Dimension, DomProps,
        Edges, EmojiMerging, ExitReason, ImageLoading, ImageSource, InputProps, Layout, Node,
        Props, RawInputAppearance, RawInputMode, RawInputProps, Ref, ScrollAreaProps, ScrollAxes,
        ScrollDelta, ScrollEvent, ScrollOffset, ScrollbarGlyph, ScrollbarStyle,
        ScrollbarVisibility, Style, StylePatch, TextClipboardAction, TextClipboardEvent,
        TextValueEvent, TextWrap, TextareaProps, ThemeProviderProps, column, heading, input,
        progress_bar, raw_input, row, scroll_area, style, style_patch, text, textarea,
        theme_context, theme_provider, ui, view,
    };
}

/// The built-in widgets, each an ordinary component constructor.
pub mod widgets {
    pub use crate::elements::image::image as raster_image;

    pub use crate::{
        alert, badge, blockquote, button, canvas, card, center, checkbox, code, column, container,
        divider, empty, footer, fragment, heading, input, kbd, label, link, muted, paragraph,
        progress_bar, radio, raw_input, row, scroll_area, section, selection_area, skeleton,
        spacer, spinner, switch, text, textarea, view,
    };
}

/// Live event types, plus the raw terminal events an application can read.
pub mod events {
    pub use crate::basic::{
        EventHandlers, EventListener, EventPhase, FocusEvent, KeyboardEvent, PasteEvent,
        PointerButton, PointerEvent, PointerEventKind, PointerId, PointerType, ResizeEvent,
        ScrollEvent, TerminalFocusEvent, WheelEvent,
    };
    pub use crate::runtime::{DispatchOutcome, FocusError, FocusOutcome};
    pub use crossterm::event::{KeyEvent, MouseEvent};
}

/// The pipeline protocol, runtime ownership, and raw frame operations.
///
/// A high-level application never needs this tier; it is for code that builds
/// its own pipeline or frame.
pub mod advanced {
    pub use crate::basic::DomNode;
    pub use crate::data::{
        Cell, CellEdit, CellError, Frame, ImageId, Operation, Rect, ScreenPosition, Size,
    };
    pub use crate::frame_builder::{
        BuildError, CellSurfaceHandle, FrameBuilder, RasterSurfaceHandle, SurfaceHandle,
    };
    pub use crate::runtime::{
        ChannelRenderer, Commit, CommitConfig, ConfigError, DispatchOutcome, EventDispatcher,
        FocusError, FocusOutcome, FrameError, ImageMetrics, ImageResource, LayoutInstrument,
        LimitError, Lower, LowerError, PipelineComponent, Renderer, RendererConfig,
        RendererConfigError, ResourceLimits, Runtime, RuntimeError, RuntimeHandle, RuntimeMetrics,
        ShutdownPolicy, Stage, SurfaceKind, ViewportSetter, live_worker_count, runtime_metrics,
    };
}

#[doc(hidden)]
pub mod __private {
    pub use crate::app::{
        MAX_RENDER_WAIT, coalesce_event, drive_session, drive_session_with, run_session, teardown,
        terminal_clipboard_sequence,
    };
    pub use crate::basic::editor_surface::{CommittedLayout, EditorSurface};
    pub use crate::basic::selection::{
        CaretPoint, ClipboardIntent, CommittedSelection, DocPoint, DocumentBuilder, Run, Segment,
        Selection, SelectionConfig, SelectionDocument, SelectionMotion, SelectionOverlay,
        SelectionProbe, SelectionStyles, block_separator, clipboard_clear, clipboard_intent,
        clipboard_load, clipboard_store, extends_selection, motion_for, set_system_writer,
        value_layout,
    };
    pub use crate::basic::text_layout::{
        ComputedText, HitBias, Item, ItemKind, Row, ShapedGlyph, TAB_WIDTH, TextLayout, layout_text,
    };
    pub use crate::basic::{
        __ui_apply, __ui_events, __ui_tag_names_equal, TextLayoutForTest, indexed_layout_for_test,
    };
    pub use crate::data::CellSlot;
    pub use crate::elements::input::model::{
        Caret, EditAction, EditIntent, EditModel, EditOutcome, EditPolicy, Outcome, ValueOwnership,
        normalize,
    };
    pub use crate::elements::normalize_for_test;
    pub use crate::glyph::{
        AllowedGlyphWidth, GlyphError, ValidatedTerminalGlyph, validate_terminal_glyph,
    };
    pub use crate::lifecycle::PhaseRunner;
    pub use crate::raster::{ImageSourceKey, render_rgba_with_cell_size};
    pub use crate::runtime::commit::geometry::RectI;
    pub use crate::runtime::commit::text::{
        cell_symbol, editor_raster, layout, layout_for, surface_layout, text_measure,
    };
    pub use crate::runtime::image::manager::{
        ByteBudget, ImageLoader, ImageManager, SourceCacheEntry, SourceRequest, SourceState,
    };
    pub use crate::runtime::metrics::{
        note_event_dispatched, note_frame_presented, note_node_lowered, note_output_bytes,
        note_text_shaping,
    };
    pub use crate::runtime::renderer::{Surface, encode_diff};
}
