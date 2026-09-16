//! Text leaves: spans, text content, wrap/align/overflow keywords, and styles.
//! Owns the `Text` node payload and `Span`, plus the `TextWrap`, `TextAlign`,
//! and `TextOverflow` keyword enums.

use std::{fmt, sync::Arc};

use crossterm::style::Color;

use super::editor_surface::{EditorSurface, LayoutProbe};
use super::props::{Style, TextStyle};

/// How overflowing text is wrapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextWrap {
    /// Do not wrap; keep text on one line.
    #[default]
    NoWrap,
    /// Wrap at word boundaries.
    Soft,
    /// Wrap at the exact column, splitting words.
    Hard,
}

/// Horizontal alignment of text within its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    /// Align to the start edge.
    #[default]
    Start,
    /// Align to the center.
    Center,
    /// Align to the end edge.
    End,
}

/// How text that still overflows is truncated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextOverflow {
    /// Clip at the edge.
    #[default]
    Clip,
    /// Truncate with an ellipsis.
    Ellipsis,
}

/// A styled run of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub(crate) content: String,
    pub(crate) style: TextStyle,
}

macro_rules! text_style_builders {
    ($($name:ident),+ $(,)?) => {
        $(
            /// Enables (and overrides the inherited value of) this text attribute.
            pub fn $name(mut self) -> Self {
                self.style = self.style.$name();
                self
            }
        )+
    };
}

macro_rules! text_color_builders {
    ($($name:ident),+ $(,)?) => {
        $(
            /// Overrides this text color.
            pub fn $name(mut self, color: Color) -> Self {
                self.style = self.style.$name(color);
                self
            }
        )+
    };
}

impl Span {
    /// Creates a span with default styling.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: TextStyle::default(),
        }
    }

    /// Replaces the span's text style.
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }
    text_color_builders!(foreground, background);

    text_style_builders!(
        bold,
        dim,
        italic,
        underlined,
        slow_blink,
        rapid_blink,
        reverse,
        hidden,
        crossed_out,
        fraktur,
        framed,
        encircled,
        overlined,
    );
}

/// A text node: styled spans plus wrap, align, overflow, and layout styling.
#[derive(Clone)]
pub struct Text {
    pub(crate) spans: Vec<Span>,
    pub(crate) style: TextStyle,
    pub(crate) layout_style: Style,
    pub(crate) wrap: TextWrap,
    pub(crate) align: TextAlign,
    pub(crate) overflow: TextOverflow,
    pub(crate) editor: Option<Arc<EditorSurface>>,
    pub(crate) probe: Option<LayoutProbe>,
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Text")
            .field("spans", &self.spans)
            .field("wrap", &self.wrap)
            .field("align", &self.align)
            .field("overflow", &self.overflow)
            .finish_non_exhaustive()
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.spans == other.spans
            && self.style == other.style
            && self.layout_style == other.layout_style
            && self.wrap == other.wrap
            && self.align == other.align
            && self.overflow == other.overflow
            && self.editor == other.editor
            && self.probe == other.probe
    }
}

impl Eq for Text {}

impl Text {
    /// Creates a single-span text node.
    pub fn new(content: impl Into<String>) -> Self {
        Self::from_spans([Span::new(content)])
    }

    /// Creates a text node from styled spans.
    pub fn from_spans(spans: impl IntoIterator<Item = Span>) -> Self {
        Self {
            spans: spans.into_iter().collect(),
            style: TextStyle::default(),
            layout_style: Style::default(),
            wrap: TextWrap::default(),
            align: TextAlign::default(),
            overflow: TextOverflow::default(),
            editor: None,
            probe: None,
        }
    }

    pub(crate) fn editor_surface(mut self, surface: EditorSurface, probe: LayoutProbe) -> Self {
        self.editor = Some(Arc::new(surface));
        self.probe = Some(probe);
        self
    }

    /// Replaces the text style; the two style domains stay explicitly named.
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces the layout style.
    pub fn layout_style(mut self, style: Style) -> Self {
        self.layout_style = style;
        self
    }

    text_color_builders!(foreground, background);

    text_style_builders!(
        bold,
        dim,
        italic,
        underlined,
        slow_blink,
        rapid_blink,
        reverse,
        hidden,
        crossed_out,
        fraktur,
        framed,
        encircled,
        overlined,
    );
    /// Sets the wrap mode.
    pub fn wrap(mut self, wrap: TextWrap) -> Self {
        self.wrap = wrap;
        self
    }
    /// Sets the horizontal alignment.
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }
    /// Sets the overflow truncation mode.
    pub fn overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }
}

impl From<&str> for Text {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}
impl From<String> for Text {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}
