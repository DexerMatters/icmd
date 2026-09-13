use std::{
    error::Error,
    fmt,
    ops::{self, Deref, DerefMut},
    sync::Arc,
};

use crossterm::style::{Attribute, Attributes as CrosstermAttributes, Color};

use crate::{Node, data::MAX_GLYPH_BYTES};

use super::{common::Attr, events::EventHandlers, text::Text};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum PercentBasis {
    #[default]
    Available,
    Viewport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Percent {
    basis_points: i32,
    basis: PercentBasis,
}

impl Percent {
    pub const ZERO: Self = Self::available(0);
    pub const FULL: Self = Self::available(100);
    pub const VIEWPORT_FULL: Self = Self::viewport(100);

    pub const fn available(value: i32) -> Self {
        Self::available_basis_points(value.saturating_mul(100))
    }

    pub const fn viewport(value: i32) -> Self {
        Self::viewport_basis_points(value.saturating_mul(100))
    }

    pub const fn available_basis_points(value: i32) -> Self {
        Self {
            basis_points: value,
            basis: PercentBasis::Available,
        }
    }

    pub const fn viewport_basis_points(value: i32) -> Self {
        Self {
            basis_points: value,
            basis: PercentBasis::Viewport,
        }
    }

    pub const fn basis_points(self) -> i32 {
        self.basis_points
    }

    pub const fn basis(self) -> PercentBasis {
        self.basis
    }

    pub fn resolve(self, reference: i32) -> i32 {
        ((reference as i64 * self.basis_points as i64) / 10_000)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Dimension {
    #[default]
    Auto,
    Cells(u16),
    Percent(Percent),
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AxisPosition {
    #[default]
    Start,
    Cells(i32),
    Percent(Percent),
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Layout {
    Absolute,
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Visibility {
    #[default]
    Visible,
    Invisible,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Overflow {
    #[default]
    Clip,
    Visible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScrollAxes {
    #[default]
    Vertical,
    Horizontal,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScrollbarVisibility {
    #[default]
    Auto,
    Always,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ScrollOffset {
    pub x: u32,
    pub y: u32,
}

impl ScrollOffset {
    pub const fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ScrollDelta {
    pub x: i32,
    pub y: i32,
}

impl ScrollDelta {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fill {
    symbol: Arc<str>,
    width: usize,
}

// Translation from the shared glyph error to the fill-specific public error.
pub(crate) fn fill_error_from_glyph(error: crate::glyph::GlyphError) -> FillError {
    match error {
        crate::glyph::GlyphError::Empty => FillError::Empty,
        crate::glyph::GlyphError::SymbolTooLong(bytes) => FillError::SymbolTooLong(bytes),
        crate::glyph::GlyphError::MultipleGraphemes => FillError::MultipleGraphemes,
        crate::glyph::GlyphError::ControlCharacter => FillError::ControlCharacter,
        crate::glyph::GlyphError::UnsupportedWidth(width) => FillError::UnsupportedWidth(width),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillError {
    Empty,
    MultipleGraphemes,
    ControlCharacter,
    SymbolTooLong(usize),
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
    pub fn new(symbol: impl Into<String>) -> Result<Self, FillError> {
        // The shared validator owns the terminal-glyph invariant; `Fill`
        // translates its failure into the fill-specific public error. The
        // validated display width is cached so width is never recomputed.
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

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScrollbarGlyph(Fill);

impl ScrollbarGlyph {
    pub fn new(symbol: impl Into<String>) -> Result<Self, FillError> {
        // A scrollbar glyph is the same invariant with a one-column policy.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Edges<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Edges<T> {
    pub const fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BorderKind {
    #[default]
    Single,
    Rounded,
    Double,
    Heavy,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Attributes {
    pub bold: Attr<bool>,
    pub dim: Attr<bool>,
    pub italic: Attr<bool>,
    pub underlined: Attr<bool>,
    pub slow_blink: Attr<bool>,
    pub rapid_blink: Attr<bool>,
    pub reverse: Attr<bool>,
    pub hidden: Attr<bool>,
    pub crossed_out: Attr<bool>,
    pub fraktur: Attr<bool>,
    pub framed: Attr<bool>,
    pub encircled: Attr<bool>,
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
            pub fn $field(mut self, color: Color) -> Self {
                self.$field /= color;
                self
            }
        )+
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BorderStyle {
    pub kind: Attr<BorderKind>,
    pub edges: Attr<Edges<bool>>,
    pub foreground: Attr<Color>,
    pub background: Attr<Color>,
    pub attr: Attributes,
}

impl BorderStyle {
    pub fn kind(mut self, kind: BorderKind) -> Self {
        self.kind /= kind;
        self
    }

    pub fn edges(mut self, edges: Edges<bool>) -> Self {
        self.edges /= edges;
        self
    }

    color_builders!(foreground, background);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point<T> {
    pub line: T,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub foreground: Attr<Color>,
    pub background: Attr<Color>,
    pub attr: Attributes,
}

impl TextStyle {
    color_builders!(foreground, background);
}

macro_rules! text_attribute_builders {
    ($($name:ident),+ $(,)?) => {
        $(
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollbarStyle {
    pub vertical_track: ScrollbarGlyph,
    pub vertical_thumb: ScrollbarGlyph,
    pub horizontal_track: ScrollbarGlyph,
    pub horizontal_thumb: ScrollbarGlyph,
    pub track: TextStyle,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    pub layout: Attr<Layout>,
    pub width: Attr<Dimension>,
    pub height: Attr<Dimension>,
    pub line: Attr<AxisPosition>,
    pub column: Attr<AxisPosition>,
    pub margin: Attr<Edges<u16>>,
    pub padding: Attr<Edges<u16>>,
    pub gap: Attr<u16>,
    pub justify: Attr<Justify>,
    pub align: Attr<Align>,
    pub overflow: Attr<Overflow>,
    pub overflow_x: Attr<Overflow>,
    pub overflow_y: Attr<Overflow>,
    pub visibility: Attr<Visibility>,
    pub z_index: Attr<i32>,
    pub background: Attr<Color>,
    pub fill: Attr<Fill>,
    pub border: BorderStyle,
    pub text: TextStyle,
}

pub type StylePatch = Arc<dyn Fn(&mut Style) + Send + Sync + 'static>;

pub fn style_patch(apply: impl Fn(&mut Style) + Send + Sync + 'static) -> StylePatch {
    Arc::new(apply)
}

impl Attributes {
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
    pub fn with_overrides(mut self, overrides: &Self) -> Self {
        self.foreground.overlay(&overrides.foreground);
        self.background.overlay(&overrides.background);
        self.attr = self.attr.with_overrides(&overrides.attr);
        self
    }
}

impl Style {
    pub fn patch(&mut self, patch: &StylePatch) {
        patch(self);
    }

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

// Operator assignment is a transitional convenience. It cannot report a
// validation failure, so an invalid glyph is ignored rather than allowed to
// panic a worker or a caller; use `set_fill` for a fallible, explicit path.
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
    // Fallible dynamic-glyph assignment. This is the canonical path for values
    // that did not come from a validated literal.
    pub fn set_fill(&mut self, symbol: impl Into<String>) -> Result<(), FillError> {
        let fill = Fill::new(symbol)?;
        self.overlay(&Attr::Set(fill));
        Ok(())
    }

    pub fn fill(&self) -> Option<&Fill> {
        match self {
            Attr::Set(fill) => Some(fill),
            Attr::Unset => None,
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DomProps {
    pub style: Style,
    pub events: EventHandlers,
    // Tri-state so an explicit `false` can override a default `true`. The old
    // `bool` plus boolean-OR merge made a false override impossible.
    pub focusable: Attr<bool>,
    // Ask the runtime to focus this region after it is published, if nothing
    // else owns focus. This is the explicit post-publication focus request an
    // input uses to start focused without inferring focus from input delivery.
    pub autofocus: bool,
    pub(crate) scroll: Option<Box<ScrollConfig>>,
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

impl DomProps {
    // `DomProps` holds one private configuration field, so external callers
    // cannot use a struct literal with `..Default::default()`. These builders
    // are the supported construction path.
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn with_events(mut self, events: EventHandlers) -> Self {
        self.events = events;
        self
    }

    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = Attr::Set(focusable);
        self
    }

    pub fn with_autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

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

#[derive(Clone)]
pub struct Props<T> {
    pub dom: DomProps,
    pub children: Vec<Node>,
    pub user_defined: T,
}

impl<T> Props<T> {
    pub fn new(user_defined: T) -> Self {
        Self {
            dom: DomProps::default(),
            children: Vec::new(),
            user_defined,
        }
    }

    pub fn extra(&self) -> &T {
        &self.user_defined
    }
    pub fn extra_mut(&mut self) -> &mut T {
        &mut self.user_defined
    }

    pub fn host_props(&self, defaults: DomProps) -> DomProps {
        defaults.with_overrides(&self.dom)
    }

    pub fn children_node(&self) -> Node {
        self.children.clone().into_iter().collect()
    }

    pub fn with_dom(mut self, dom: DomProps) -> Self {
        self.dom = dom;
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = crate::Node>) -> Self {
        self.children.extend(children);
        self
    }

    pub fn with_extra<U>(self, user_defined: U) -> Props<U> {
        self.map(|_| user_defined)
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Props<U> {
        let Props {
            dom,
            children,
            user_defined,
        } = self;
        Props {
            dom,
            children,
            user_defined: map(user_defined),
        }
    }

    pub fn into_parts(self) -> (DomProps, Vec<Node>, T) {
        (self.dom, self.children, self.user_defined)
    }
}

impl<T> Deref for Props<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.user_defined
    }
}

impl<T> DerefMut for Props<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.user_defined
    }
}

impl<T> From<T> for Props<T> {
    fn from(user_defined: T) -> Self {
        Self::new(user_defined)
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
