//! Style, geometry keywords, and typed DOM props.
//! Owns the `Style` tree, the geometry keyword types it references, and the
//! `Props`/`DomProps` node payloads.

use std::{
    error::Error,
    fmt,
    ops::{self, Deref, DerefMut},
    sync::Arc,
};

use crossterm::style::{Attribute, Attributes as CrosstermAttributes, Color};

use crate::{Node, data::MAX_GLYPH_BYTES};

use super::{common::Attr, events::EventHandlers, text::Text};

/// What a percentage value is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum PercentBasis {
    /// Percentage of the space left after siblings, margins, and gaps.
    #[default]
    Available,
    /// Percentage of the full terminal viewport.
    Viewport,
}

/// A percentage together with its basis, stored as basis points (1/100 percent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Percent {
    basis_points: i32,
    basis: PercentBasis,
}

impl Percent {
    /// Zero percent of the available space.
    pub const ZERO: Self = Self::available(0);
    /// One hundred percent of the available space.
    pub const FULL: Self = Self::available(100);
    /// One hundred percent of the viewport.
    pub const VIEWPORT_FULL: Self = Self::viewport(100);

    /// Builds an available-space percentage from whole percent units.
    pub const fn available(value: i32) -> Self {
        Self::available_basis_points(value.saturating_mul(100))
    }

    /// Builds a viewport percentage from whole percent units.
    pub const fn viewport(value: i32) -> Self {
        Self::viewport_basis_points(value.saturating_mul(100))
    }

    /// Builds an available-space percentage from basis points (1/100 percent).
    pub const fn available_basis_points(value: i32) -> Self {
        Self {
            basis_points: value,
            basis: PercentBasis::Available,
        }
    }

    /// Builds a viewport percentage from basis points (1/100 percent).
    pub const fn viewport_basis_points(value: i32) -> Self {
        Self {
            basis_points: value,
            basis: PercentBasis::Viewport,
        }
    }

    /// Returns the raw basis points (1/100 percent).
    pub const fn basis_points(self) -> i32 {
        self.basis_points
    }

    /// Returns what the percentage is measured against.
    pub const fn basis(self) -> PercentBasis {
        self.basis
    }

    /// Resolves against `reference` cells, truncating toward zero and clamping to `i32`.
    pub fn resolve(self, reference: i32) -> i32 {
        ((reference as i64 * self.basis_points as i64) / 10_000)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
}

/// A size request along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Dimension {
    /// Size to content.
    #[default]
    Auto,
    /// Fixed size in terminal cells.
    Cells(u16),
    /// Fraction of the available space or viewport.
    Percent(Percent),
    /// Expand to fill the remaining space.
    Max,
}

/// A position request along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AxisPosition {
    /// Place at the start of the available space.
    #[default]
    Start,
    /// Offset from the start, in terminal cells.
    Cells(i32),
    /// Fraction of the available space or viewport.
    Percent(Percent),
    /// Center within the available space.
    Center,
    /// Place at the end of the available space.
    End,
}

/// How a box lays out its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Layout {
    /// Take the box out of flow and position it with `line` and `column`.
    Absolute,
    /// Stack children top to bottom.
    #[default]
    Vertical,
    /// Place children left to right.
    Horizontal,
}

/// Whether a box and its subtree occupy layout space and paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Visibility {
    /// Paint normally and take layout space.
    #[default]
    Visible,
    /// Take layout space but paint nothing.
    Invisible,
    /// Take no layout space and paint nothing.
    Hidden,
}

/// Distribution of children along the main axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Justify {
    /// Pack children at the start.
    #[default]
    Start,
    /// Pack children at the center.
    Center,
    /// Pack children at the end.
    End,
    /// Spread children with equal space between them.
    SpaceBetween,
}

/// Alignment of children along the cross axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Align {
    /// Align to the start.
    #[default]
    Start,
    /// Align to the center.
    Center,
    /// Align to the end.
    End,
    /// Stretch to fill the cross axis.
    Stretch,
}

/// Whether content may paint outside the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Overflow {
    /// Clip content to the box.
    #[default]
    Clip,
    /// Let content paint outside the box.
    Visible,
}

/// Which axes a region can scroll along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScrollAxes {
    /// Scroll vertically only.
    #[default]
    Vertical,
    /// Scroll horizontally only.
    Horizontal,
    /// Scroll on both axes.
    Both,
}

/// When a scrollbar is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScrollbarVisibility {
    /// Show only while the region is scrollable.
    #[default]
    Auto,
    /// Always show.
    Always,
    /// Never show.
    Hidden,
}

/// A scroll position, in terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ScrollOffset {
    /// Horizontal offset, in cells.
    pub x: u32,
    /// Vertical offset, in cells.
    pub y: u32,
}

impl ScrollOffset {
    /// Creates an offset from horizontal and vertical cell counts.
    pub const fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }
}

/// A scroll movement, in terminal cells; negative values move up or left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ScrollDelta {
    /// Horizontal movement, in cells.
    pub x: i32,
    /// Vertical movement, in cells.
    pub y: i32,
}

impl ScrollDelta {
    /// Creates a delta from horizontal and vertical cell counts.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A validated one- or two-column glyph used to fill a region's background.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fill {
    symbol: Arc<str>,
    width: usize,
}

/// Translates the shared glyph error into the fill-specific public error.
pub(crate) fn fill_error_from_glyph(error: crate::glyph::GlyphError) -> FillError {
    match error {
        crate::glyph::GlyphError::Empty => FillError::Empty,
        crate::glyph::GlyphError::SymbolTooLong(bytes) => FillError::SymbolTooLong(bytes),
        crate::glyph::GlyphError::MultipleGraphemes => FillError::MultipleGraphemes,
        crate::glyph::GlyphError::ControlCharacter => FillError::ControlCharacter,
        crate::glyph::GlyphError::UnsupportedWidth(width) => FillError::UnsupportedWidth(width),
    }
}

/// Why a fill or scrollbar glyph was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillError {
    /// The symbol was empty.
    Empty,
    /// The symbol contained more than one grapheme.
    MultipleGraphemes,
    /// The symbol contained a control character.
    ControlCharacter,
    /// The symbol's UTF-8 encoding exceeds the maximum glyph length, in bytes.
    SymbolTooLong(usize),
    /// The symbol's display width, in columns, is not permitted.
    UnsupportedWidth(usize),
}

impl fmt::Display for FillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "fill is empty"),
            Self::MultipleGraphemes => write!(f, "fill must contain exactly one grapheme"),
            Self::ControlCharacter => write!(f, "fill contains a control character"),
            Self::SymbolTooLong(bytes) => {
                write!(f, "fill is {bytes} bytes; maximum is {MAX_GLYPH_BYTES}")
            }
            Self::UnsupportedWidth(width) => {
                write!(f, "fill display width {width} is not supported")
            }
        }
    }
}

impl Error for FillError {}

impl Fill {
    /// Validates `symbol` as a one- or two-column terminal glyph, rejecting with a `FillError`.
    ///
    /// The shared validator owns the terminal-glyph invariant; the validated
    /// display width is cached so width is never recomputed.
    pub fn new(symbol: impl Into<String>) -> Result<Self, FillError> {
        let glyph = crate::glyph::validate_terminal_glyph(
            symbol.into().as_str(),
            crate::glyph::AllowedGlyphWidth::OneOrTwo,
        )
        .map_err(fill_error_from_glyph)?;
        Ok(Self {
            width: glyph.width(),
            symbol: glyph.into_text(),
        })
    }

    /// Returns the fill symbol.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Returns the cached display width, in terminal columns.
    pub fn width(&self) -> usize {
        self.width
    }
}

impl TryFrom<&str> for Fill {
    type Error = FillError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for Fill {
    type Error = FillError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// A validated one-column glyph used to draw a scrollbar track or thumb.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScrollbarGlyph(Fill);

impl ScrollbarGlyph {
    /// Validates `symbol` as a one-column terminal glyph.
    pub fn new(symbol: impl Into<String>) -> Result<Self, FillError> {
        let glyph = crate::glyph::validate_terminal_glyph(
            symbol.into().as_str(),
            crate::glyph::AllowedGlyphWidth::One,
        )
        .map_err(fill_error_from_glyph)?;
        Ok(Self(Fill {
            width: glyph.width(),
            symbol: glyph.into_text(),
        }))
    }

    /// Returns the glyph symbol.
    pub fn symbol(&self) -> &str {
        self.0.symbol()
    }
}

impl TryFrom<&str> for ScrollbarGlyph {
    type Error = FillError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for ScrollbarGlyph {
    type Error = FillError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Four per-side values, ordered top, right, bottom, left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Edges<T> {
    /// Top side.
    pub top: T,
    /// Right side.
    pub right: T,
    /// Bottom side.
    pub bottom: T,
    /// Left side.
    pub left: T,
}

impl<T: Copy> Edges<T> {
    /// Sets all four sides to `value`.
    pub const fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
    /// Sets `vertical` for top and bottom and `horizontal` for left and right.
    pub const fn symmetric(vertical: T, horizontal: T) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }
}

impl<T: Default + Copy> Default for Edges<T> {
    fn default() -> Self {
        Self::all(T::default())
    }
}

/// Which box-drawing character set a border uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BorderKind {
    /// Single-line box-drawing characters.
    #[default]
    Single,
    /// Single-line characters with rounded corners.
    Rounded,
    /// Double-line box-drawing characters.
    Double,
    /// Heavy single-line box-drawing characters.
    Heavy,
}

/// Tri-state terminal text attributes; each `Attr` overrides the inherited value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Attributes {
    /// Bold intensity.
    pub bold: Attr<bool>,
    /// Faint (dim) intensity.
    pub dim: Attr<bool>,
    /// Italic slant.
    pub italic: Attr<bool>,
    /// Underline.
    pub underlined: Attr<bool>,
    /// Slow blink.
    pub slow_blink: Attr<bool>,
    /// Rapid blink.
    pub rapid_blink: Attr<bool>,
    /// Swap foreground and background.
    pub reverse: Attr<bool>,
    /// Conceal the text.
    pub hidden: Attr<bool>,
    /// Strikethrough.
    pub crossed_out: Attr<bool>,
    /// Fraktur (Gothic) lettering.
    pub fraktur: Attr<bool>,
    /// Surround the text with a frame.
    pub framed: Attr<bool>,
    /// Surround the text with a circle.
    pub encircled: Attr<bool>,
    /// Overline.
    pub overlined: Attr<bool>,
}

impl Attributes {
    pub(crate) fn resolve(&self, inherited: CrosstermAttributes) -> CrosstermAttributes {
        let mut attributes = inherited;
        macro_rules! resolve {
            ($($field:ident => $attribute:expr),+ $(,)?) => {
                $(
                    if let Some(enabled) = self.$field.clone().into() {
                        if enabled {
                            attributes.set($attribute);
                        } else {
                            attributes.unset($attribute);
                        }
                    }
                )+
            };
        }
        resolve!(
            bold => Attribute::Bold,
            dim => Attribute::Dim,
            italic => Attribute::Italic,
            underlined => Attribute::Underlined,
            slow_blink => Attribute::SlowBlink,
            rapid_blink => Attribute::RapidBlink,
            reverse => Attribute::Reverse,
            hidden => Attribute::Hidden,
            crossed_out => Attribute::CrossedOut,
            fraktur => Attribute::Fraktur,
            framed => Attribute::Framed,
            encircled => Attribute::Encircled,
            overlined => Attribute::OverLined,
        );
        attributes
    }
}

macro_rules! attribute_builders {
    ($($name:ident),+ $(,)?) => {
        $(
            /// Enables (and overrides the inherited value of) this attribute.
            pub fn $name(mut self) -> Self {
                self.$name /= true;
                self
            }
        )+
    };
}

impl Attributes {
    attribute_builders!(
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

macro_rules! color_builders {
    ($($field:ident),+ $(,)?) => {
        $(
            /// Overrides this color.
            pub fn $field(mut self, color: Color) -> Self {
                self.$field /= color;
                self
            }
        )+
    };
}

/// Border configuration: character set, drawn edges, colors, and text attributes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BorderStyle {
    /// Border character set.
    pub kind: Attr<BorderKind>,
    /// Which edges are drawn.
    pub edges: Attr<Edges<bool>>,
    /// Border foreground color.
    pub foreground: Attr<Color>,
    /// Border background color.
    pub background: Attr<Color>,
    /// Text attributes applied to the border.
    pub attr: Attributes,
}

impl BorderStyle {
    /// Overrides the border character set.
    pub fn kind(mut self, kind: BorderKind) -> Self {
        self.kind /= kind;
        self
    }

    /// Overrides which edges are drawn.
    pub fn edges(mut self, edges: Edges<bool>) -> Self {
        self.edges /= edges;
        self
    }

    color_builders!(foreground, background);
}

/// A line/column position, in terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point<T> {
    /// Row, in cells.
    pub line: T,
    /// Column, in cells.
    pub column: T,
}

impl<T: Default> Default for Point<T> {
    fn default() -> Self {
        Self {
            line: T::default(),
            column: T::default(),
        }
    }
}

/// Tri-state text styling: foreground and background colors plus attributes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextStyle {
    /// Text foreground color.
    pub foreground: Attr<Color>,
    /// Text background color.
    pub background: Attr<Color>,
    /// Text attributes.
    pub attr: Attributes,
}

impl TextStyle {
    color_builders!(foreground, background);
}

macro_rules! text_attribute_builders {
    ($($name:ident),+ $(,)?) => {
        $(
            /// Enables (and overrides the inherited value of) this text attribute.
            pub fn $name(mut self) -> Self {
                self.attr = self.attr.$name();
                self
            }
        )+
    };
}

impl TextStyle {
    text_attribute_builders!(
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

/// Glyphs and text styles for a scrollbar's track and thumb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollbarStyle {
    /// Vertical track glyph.
    pub vertical_track: ScrollbarGlyph,
    /// Vertical thumb glyph.
    pub vertical_thumb: ScrollbarGlyph,
    /// Horizontal track glyph.
    pub horizontal_track: ScrollbarGlyph,
    /// Horizontal thumb glyph.
    pub horizontal_thumb: ScrollbarGlyph,
    /// Text style for the track.
    pub track: TextStyle,
    /// Text style for the thumb.
    pub thumb: TextStyle,
}

impl Default for ScrollbarStyle {
    fn default() -> Self {
        Self {
            vertical_track: ScrollbarGlyph::new("│").expect("default scrollbar glyph is valid"),
            vertical_thumb: ScrollbarGlyph::new("┃").expect("default scrollbar glyph is valid"),
            horizontal_track: ScrollbarGlyph::new("─").expect("default scrollbar glyph is valid"),
            horizontal_thumb: ScrollbarGlyph::new("━").expect("default scrollbar glyph is valid"),
            track: TextStyle::default(),
            thumb: TextStyle::default(),
        }
    }
}

/// The style tree applied to a node: layout, spacing, border, and text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    /// How the box lays out its children.
    pub layout: Attr<Layout>,
    /// Width request (`Auto`, cells, `Percent`, or `Max`).
    pub width: Attr<Dimension>,
    /// Height request (`Auto`, cells, `Percent`, or `Max`).
    pub height: Attr<Dimension>,
    /// Vertical position of the box.
    pub line: Attr<AxisPosition>,
    /// Horizontal position of the box.
    pub column: Attr<AxisPosition>,
    /// Space outside the box, in cells, per edge.
    pub margin: Attr<Edges<u16>>,
    /// Space inside the box, in cells, per edge.
    pub padding: Attr<Edges<u16>>,
    /// Space between children in a flex layout, in cells.
    pub gap: Attr<u16>,
    /// Distribution of children along the main axis.
    pub justify: Attr<Justify>,
    /// Alignment of children along the cross axis.
    pub align: Attr<Align>,
    /// Whether content may paint outside the box on either axis.
    pub overflow: Attr<Overflow>,
    /// Horizontal overflow, overriding `overflow`.
    pub overflow_x: Attr<Overflow>,
    /// Vertical overflow, overriding `overflow`.
    pub overflow_y: Attr<Overflow>,
    /// Whether the box occupies layout space and paints.
    pub visibility: Attr<Visibility>,
    /// Paint order offset; higher values paint later.
    pub z_index: Attr<i32>,
    /// Background color.
    pub background: Attr<Color>,
    /// Glyph used to fill background cells.
    pub fill: Attr<Fill>,
    /// Border configuration.
    pub border: BorderStyle,
    /// Text styling.
    pub text: TextStyle,
}

/// A callable that mutates a `Style` in place; cheap to clone and share.
pub type StylePatch = Arc<dyn Fn(&mut Style) + Send + Sync + 'static>;

/// Wraps `apply` into a shareable `StylePatch`.
pub fn style_patch(apply: impl Fn(&mut Style) + Send + Sync + 'static) -> StylePatch {
    Arc::new(apply)
}

impl Attributes {
    /// Returns these attributes with `overrides` layered on top.
    pub fn with_overrides(mut self, overrides: &Self) -> Self {
        macro_rules! merge {
            ($($field:ident),+ $(,)?) => {
                $(self.$field.overlay(&overrides.$field);)+
            };
        }
        merge!(
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
        self
    }
}

impl BorderStyle {
    /// Returns this border style with `overrides` layered on top.
    pub fn with_overrides(mut self, overrides: &Self) -> Self {
        self.kind.overlay(&overrides.kind);
        self.edges.overlay(&overrides.edges);
        self.foreground.overlay(&overrides.foreground);
        self.background.overlay(&overrides.background);
        self.attr = self.attr.with_overrides(&overrides.attr);
        self
    }
}

impl TextStyle {
    /// Returns this text style with `overrides` layered on top.
    pub fn with_overrides(mut self, overrides: &Self) -> Self {
        self.foreground.overlay(&overrides.foreground);
        self.background.overlay(&overrides.background);
        self.attr = self.attr.with_overrides(&overrides.attr);
        self
    }
}

impl Style {
    /// Applies `patch` to this style in place.
    pub fn patch(&mut self, patch: &StylePatch) {
        patch(self);
    }

    /// Returns this style with `overrides` layered on top.
    pub fn with_overrides(mut self, overrides: &Self) -> Self {
        macro_rules! merge {
            ($($field:ident),+ $(,)?) => {
                $(self.$field.overlay(&overrides.$field);)+
            };
        }
        merge!(
            layout, width, height, line, column, margin, padding, gap, justify, align, overflow,
            overflow_x, overflow_y, visibility, z_index, background, fill,
        );
        self.border = self.border.with_overrides(&overrides.border);
        self.text = self.text.with_overrides(&overrides.text);
        self
    }
}

/// Operator assignment ignores an invalid glyph instead of panicking; use `set_fill` for a fallible path.
impl ops::DivAssign<&str> for Attr<Fill> {
    fn div_assign(&mut self, rhs: &str) {
        if let Ok(fill) = Fill::new(rhs) {
            self.overlay(&Attr::Set(fill));
        }
    }
}

impl ops::DivAssign<char> for Attr<Fill> {
    fn div_assign(&mut self, rhs: char) {
        if let Ok(fill) = Fill::new(rhs.to_string()) {
            self.overlay(&Attr::Set(fill));
        }
    }
}

impl Attr<Fill> {
    /// Sets the fill from an unvalidated symbol, the canonical fallible path for non-literals.
    pub fn set_fill(&mut self, symbol: impl Into<String>) -> Result<(), FillError> {
        let fill = Fill::new(symbol)?;
        self.overlay(&Attr::Set(fill));
        Ok(())
    }

    /// Returns the fill when one is explicitly set.
    pub fn fill(&self) -> Option<&Fill> {
        match self {
            Attr::Set(fill) => Some(fill),
            Attr::Unset => None,
        }
    }
}

/// Builds a `Style` by mutating a default one with `f`.
pub fn style(f: impl FnOnce(&mut Style)) -> Style {
    let mut style = Style::default();
    f(&mut style);
    style
}

impl ops::DivAssign<fn(&mut Self)> for Style {
    fn div_assign(&mut self, rhs: fn(&mut Self)) {
        rhs(self);
    }
}

impl ops::DivAssign<StylePatch> for Style {
    fn div_assign(&mut self, rhs: StylePatch) {
        self.patch(&rhs);
    }
}

impl ops::DivAssign<&StylePatch> for Style {
    fn div_assign(&mut self, rhs: &StylePatch) {
        self.patch(rhs);
    }
}

/// Properties attached to a node: style, events, focus, and scroll state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DomProps {
    /// Styling applied to the node.
    pub style: Style,
    /// Event handlers registered on the node.
    pub events: EventHandlers,
    /// Whether the node can take focus; tri-state so an explicit `false` overrides a default `true`.
    pub focusable: Attr<bool>,
    /// Requests focus after publication when nothing else already owns focus.
    pub autofocus: bool,
    pub(crate) scroll: Option<Box<ScrollConfig>>,
    /// Present only on a selectable region's host node; the paint pass carries it
    /// down the subtree, so the text leaves below become selectable.
    pub(crate) selection: Option<std::sync::Arc<crate::basic::selection::SelectionConfig>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScrollConfig {
    pub(crate) horizontal: bool,
    pub(crate) vertical: bool,
    pub(crate) scrollbar_visibility: ScrollbarVisibility,
    pub(crate) offset: Option<ScrollOffset>,
    pub(crate) enable_mouse: bool,
    pub(crate) enable_wheel: bool,
    pub(crate) enable_keyboard: bool,
    pub(crate) wheel_step: u16,
    pub(crate) scrollbar: ScrollbarStyle,
}

/// Construction goes through builders because `DomProps` holds a private field.
impl DomProps {
    /// Returns these props with the node style replaced.
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Returns these props with the event handlers replaced.
    pub fn with_events(mut self, events: EventHandlers) -> Self {
        self.events = events;
        self
    }

    /// Returns these props with focusability set explicitly.
    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = Attr::Set(focusable);
        self
    }

    /// Returns these props with the autofocus request set.
    pub fn with_autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    /// Attaches a region's live selection; `with_overrides` leaves it untouched so
    /// a caller cannot detach or replace a host's selection.
    pub(crate) fn with_selection_host(
        mut self,
        selection: std::sync::Arc<crate::basic::selection::SelectionConfig>,
    ) -> Self {
        self.selection = Some(selection);
        self
    }

    /// Returns these props with `overrides` layered on top; scroll state is replaced only when `overrides` carries it.
    pub fn with_overrides(mut self, overrides: &Self) -> Self {
        self.style = self.style.with_overrides(&overrides.style);
        self.events.merge(&overrides.events);
        self.focusable.overlay(&overrides.focusable);
        self.autofocus |= overrides.autofocus;
        if overrides.scroll.is_some() {
            self.scroll = overrides.scroll.clone();
        }
        self
    }
}

/// A node payload: DOM properties, children, and typed data `T`.
#[derive(Clone)]
pub struct Props<T> {
    /// DOM properties for the node this payload builds.
    pub dom: DomProps,
    /// Child nodes of this payload.
    pub children: Vec<Node>,
    /// Payload state, reachable only through `data`/`data_mut` (aliased as `extra`/`extra_mut`).
    data: T,
}

impl<T> Props<T> {
    /// Creates a payload with default DOM props and no children.
    pub fn new(data: T) -> Self {
        Self {
            dom: DomProps::default(),
            children: Vec::new(),
            data,
        }
    }

    /// Assembles a payload from its DOM props, children, and data.
    ///
    /// Kept because the payload field is private, so external code cannot build the struct literally.
    pub fn with_parts(dom: DomProps, children: Vec<Node>, data: T) -> Self {
        Self {
            dom,
            children,
            data,
        }
    }

    /// Returns the typed payload data, the canonical accessor.
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Returns the typed payload data mutably.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    /// Consumes the payload and returns its data.
    pub fn into_data(self) -> T {
        self.data
    }

    /// Replaces the payload data.
    pub fn set_data(&mut self, data: T) {
        self.data = data;
    }

    /// Alias for `data`.
    pub fn extra(&self) -> &T {
        self.data()
    }
    /// Alias for `data_mut`.
    pub fn extra_mut(&mut self) -> &mut T {
        self.data_mut()
    }

    /// Returns `defaults` with this payload's DOM props layered on top.
    pub fn host_props(&self, defaults: DomProps) -> DomProps {
        defaults.with_overrides(&self.dom)
    }

    /// Collects the children into a single node.
    pub fn children_node(&self) -> Node {
        self.children.clone().into_iter().collect()
    }

    /// Returns this payload with the DOM props replaced.
    pub fn with_dom(mut self, dom: DomProps) -> Self {
        self.dom = dom;
        self
    }

    /// Appends `children` to the payload's children.
    pub fn children(mut self, children: impl IntoIterator<Item = crate::Node>) -> Self {
        self.children.extend(children);
        self
    }

    /// Replaces the payload data, keeping DOM props and children.
    pub fn with_extra<U>(self, data: U) -> Props<U> {
        self.map(|_| data)
    }

    /// Transforms the payload data, keeping DOM props and children.
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Props<U> {
        let Props {
            dom,
            children,
            data,
        } = self;
        Props {
            dom,
            children,
            data: map(data),
        }
    }

    /// Consumes the payload into `(dom, children, data)`.
    pub fn into_parts(self) -> (DomProps, Vec<Node>, T) {
        (self.dom, self.children, self.data)
    }
}

/// Transparent payload access delegates to `data`, keeping one owner of the payload state.
impl<T> Deref for Props<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data()
    }
}

impl<T> DerefMut for Props<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data_mut()
    }
}

impl<T> From<T> for Props<T> {
    fn from(data: T) -> Self {
        Self::new(data)
    }
}

impl From<&str> for Props<Text> {
    fn from(value: &str) -> Self {
        Self::new(Text::new(value))
    }
}

impl From<String> for Props<Text> {
    fn from(value: String) -> Self {
        Self::new(Text::new(value))
    }
}

impl<T: Default> Default for Props<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
