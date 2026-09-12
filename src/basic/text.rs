use std::{fmt, sync::Arc};

use crossterm::style::Color;

use super::editor_surface::{EditorSurface, LayoutProbe};
use super::props::{Style, TextStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextWrap {
    #[default]
    /// Only explicit newline characters create visual rows.
    NoWrap,
    /// Prefer Unicode line-break opportunities, falling back to extended
    /// grapheme boundaries for an overlong unbreakable fragment.
    Soft,
    /// Wrap strictly at the available cell width, splitting only at extended
    /// grapheme boundaries.
    Hard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub(crate) content: String,
    pub(crate) style: TextStyle,
}

macro_rules! text_style_builders {
    ($($name:ident),+ $(,)?) => {
        $(
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
            pub fn $name(mut self, color: Color) -> Self {
                self.style = self.style.$name(color);
                self
            }
        )+
    };
}

impl Span {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: TextStyle::default(),
        }
    }

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

#[derive(Clone)]
pub struct Text {
    pub(crate) spans: Vec<Span>,
    pub(crate) style: TextStyle,
    pub(crate) layout_style: Style,
    pub(crate) wrap: TextWrap,
    pub(crate) align: TextAlign,
    pub(crate) overflow: TextOverflow,
    /// Editor-only decorations (selection, caret, placeholder) painted from the
    /// canonical layout at the committed content width. `None` for ordinary
    /// text.
    pub(crate) editor: Option<Arc<EditorSurface>>,
    /// Passive channel the commit pass publishes its layout into.
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

/// Equality describes the rendered content only.
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
    pub fn new(content: impl Into<String>) -> Self {
        Self::from_spans([Span::new(content)])
    }

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

    /// Attach editor-only paint instructions and the probe the commit pass
    /// publishes its layout into.
    pub(crate) fn editor_surface(mut self, surface: EditorSurface, probe: LayoutProbe) -> Self {
        self.editor = Some(Arc::new(surface));
        self.probe = Some(probe);
        self
    }

    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }
    pub fn with_style(mut self, style: Style) -> Self {
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
    pub fn wrap(mut self, wrap: TextWrap) -> Self {
        self.wrap = wrap;
        self
    }
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }
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
