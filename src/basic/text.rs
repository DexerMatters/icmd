//! Text leaves: spans, text content, wrap/align/overflow keywords, and styles.
//! Owns the `Text` node payload and `Span`, plus the `TextWrap`, `TextAlign`,
//! and `TextOverflow` keyword enums.

use std::{borrow::Cow, fmt, sync::Arc};

use crossterm::style::Color;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::editor_surface::{EditorSurface, LayoutProbe};
use super::props::{Style, TextStyle};

/// The display width of `text` in terminal cells, using the default merged
/// emoji policy.
///
/// Newlines and tabs are layout directives rather than painted cells and count
/// as zero here; control graphemes count as zero as well.
pub fn display_width(text: &str) -> usize {
    display_width_with(text, crate::EmojiMerging::Merge)
}

/// The display width of `text` under an explicit emoji-merging policy.
pub fn display_width_with(text: &str, merging: crate::EmojiMerging) -> usize {
    if merging.merges() {
        return text
            .graphemes(true)
            .filter(|grapheme| {
                *grapheme != "\n" && *grapheme != "\t" && !grapheme.chars().any(char::is_control)
            })
            .map(UnicodeWidthStr::width)
            .sum();
    }
    crate::data::display_units(text, merging)
        .into_iter()
        .map(|range| UnicodeWidthStr::width(&text[range]))
        .sum()
}

/// Truncates `text` to at most `width` cells, keeping the head and adding an
/// ellipsis when anything was dropped.
///
/// The returned borrowed slice is unchanged when it already fits. A wide
/// grapheme that would cross the budget is dropped rather than split.
pub fn fit(text: &str, width: usize) -> Cow<'_, str> {
    let budget = width;
    if display_width(text) <= budget {
        return Cow::Borrowed(text);
    }
    if budget == 0 {
        return Cow::Borrowed("");
    }
    if budget == 1 {
        return Cow::Borrowed("…");
    }
    let mut used = 0usize;
    let mut end = 0usize;
    for (index, grapheme) in text.grapheme_indices(true) {
        let cell_width = grapheme_width(grapheme);
        if used.saturating_add(cell_width) > budget.saturating_sub(1) {
            break;
        }
        used = used.saturating_add(cell_width);
        end = index + grapheme.len();
    }
    let mut out = String::with_capacity(end + 3);
    out.push_str(&text[..end]);
    out.push('…');
    Cow::Owned(out)
}

/// Truncates `text` from the start, keeping the tail and prefixing an ellipsis.
///
/// This is the right-aligned counterpart to [`fit`], useful for numeric or
/// path columns.
pub fn fit_right(text: &str, width: usize) -> Cow<'_, str> {
    let budget = width;
    if display_width(text) <= budget {
        return Cow::Borrowed(text);
    }
    if budget == 0 {
        return Cow::Borrowed("");
    }
    if budget == 1 {
        return Cow::Borrowed("…");
    }
    let mut used = 0usize;
    let mut start = text.len();
    for (index, grapheme) in text.grapheme_indices(true).rev() {
        let cell_width = grapheme_width(grapheme);
        if used.saturating_add(cell_width) > budget.saturating_sub(1) {
            break;
        }
        used = used.saturating_add(cell_width);
        start = index;
    }
    let mut out = String::with_capacity(text.len().saturating_sub(start) + 3);
    out.push('…');
    out.push_str(&text[start..]);
    Cow::Owned(out)
}

fn grapheme_width(grapheme: &str) -> usize {
    if grapheme == "\n" || grapheme == "\t" || grapheme.chars().any(char::is_control) {
        0
    } else {
        UnicodeWidthStr::width(grapheme)
    }
}

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
