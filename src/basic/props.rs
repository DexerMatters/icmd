use std::{
    error::Error,
    fmt,
    ops::{self, Deref, DerefMut},
    sync::Arc,
};

use crossterm::style::{Attribute, Attributes as CrosstermAttributes, Color};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{Node, data::MAX_GLYPH_BYTES};

use super::{common::Attr, events::EventHandlers, text::Text};

/// The reference space used when resolving a percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum PercentBasis {
    /// Resolve against the space offered by the parent layout.
    #[default]
    Available,
    /// Resolve against the terminal viewport on the corresponding axis.
    Viewport,
}

/// A percentage whose reference space is either the parent or the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Percent {
    basis_points: i32,
    basis: PercentBasis,
}

impl Percent {
    pub const ZERO: Self = Self::available(0);
    pub const FULL: Self = Self::available(100);
    pub const VIEWPORT_FULL: Self = Self::viewport(100);

    /// Construct a percentage relative to the space offered by the parent.
    pub const fn available(value: i32) -> Self {
        Self::available_basis_points(value.saturating_mul(100))
    }

    /// Construct a percentage relative to the terminal viewport.
    pub const fn viewport(value: i32) -> Self {
        Self::viewport_basis_points(value.saturating_mul(100))
    }

    /// Construct an available-space percentage.
    ///
    /// This is retained as an alias for [`Self::available`] so existing code
    /// keeps its parent-relative behavior.
    pub const fn from_percent(value: i32) -> Self {
        Self::available(value)
    }

    /// Construct an available-space percentage from basis points.
    pub const fn from_basis_points(value: i32) -> Self {
        Self::available_basis_points(value)
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

    /// Resolve the percentage against an explicitly supplied reference size.
    ///
    /// Layout uses [`Self::basis`] to select either the available space or
    /// viewport before calling this method.
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
    /// A percentage of either available parent space or the viewport,
    /// selected by [`Percent::basis`].
    Percent(Percent),
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AxisPosition {
    #[default]
    Start,
    Cells(i32),
    /// An offset relative to the available travel or the viewport,
    /// selected by [`Percent::basis`].
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Overflow {
    #[default]
    Visible,
    Clip,
    Scroll(ScrollProps),
    Auto(AutoProps),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScrollAxes {
    #[default]
    Both,
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverflowScrollbarStyle {
    pub vertical_track: Fill,
    pub vertical_thumb: Fill,
    pub horizontal_track: Fill,
    pub horizontal_thumb: Fill,
    pub track: TextStyle,
    pub thumb: TextStyle,
}

impl Default for OverflowScrollbarStyle {
    fn default() -> Self {
        Self {
            vertical_track: Fill::new("│").expect("default scrollbar glyph is valid"),
            vertical_thumb: Fill::new("┃").expect("default scrollbar glyph is valid"),
            horizontal_track: Fill::new("─").expect("default scrollbar glyph is valid"),
            horizontal_thumb: Fill::new("━").expect("default scrollbar glyph is valid"),
            track: TextStyle::default(),
            thumb: TextStyle::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollProps {
    pub axes: ScrollAxes,
    pub draw_scrollbar: bool,
    pub wheel_step: u16,
    pub scrollbar: OverflowScrollbarStyle,
}

impl Default for ScrollProps {
    fn default() -> Self {
        Self {
            axes: ScrollAxes::Both,
            draw_scrollbar: true,
            wheel_step: 1,
            scrollbar: OverflowScrollbarStyle::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoProps {
    pub axes: ScrollAxes,
    pub draw_scrollbar: bool,
    pub wheel_step: u16,
    pub scrollbar: OverflowScrollbarStyle,
}

impl Default for AutoProps {
    fn default() -> Self {
        Self {
            axes: ScrollAxes::Both,
            draw_scrollbar: true,
            wheel_step: 1,
            scrollbar: OverflowScrollbarStyle::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fill(String);

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
        let symbol = symbol.into();
        if symbol.is_empty() {
            return Err(FillError::Empty);
        }
        if symbol.len() > MAX_GLYPH_BYTES {
            return Err(FillError::SymbolTooLong(symbol.len()));
        }
        if symbol.graphemes(true).count() != 1 {
            return Err(FillError::MultipleGraphemes);
        }
        if symbol.chars().any(char::is_control) {
            return Err(FillError::ControlCharacter);
        }
        let width = UnicodeWidthStr::width(symbol.as_str());
        if !(1..=2).contains(&width) {
            return Err(FillError::UnsupportedWidth(width));
        }
        Ok(Self(symbol))
    }

    pub fn symbol(&self) -> &str {
        &self.0
    }

    pub fn width(&self) -> usize {
        UnicodeWidthStr::width(self.0.as_str())
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
    pub visibility: Attr<Visibility>,
    pub z_index: Attr<i32>,
    pub background: Attr<Color>,
    pub fill: Attr<Fill>,
    pub border: BorderStyle,
    pub text: TextStyle,
}

/// A reusable, thread-safe style mutation.
pub type StylePatch = Arc<dyn Fn(&mut Style) + Send + Sync + 'static>;

/// Create a reusable style mutation for composing component defaults.
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

    #[deprecated(note = "use `with_overrides` to make precedence explicit")]
    pub fn merge(self, overrides: &Self) -> Self {
        self.with_overrides(overrides)
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

    #[deprecated(note = "use `with_overrides` to make precedence explicit")]
    pub fn merge(self, overrides: &Self) -> Self {
        self.with_overrides(overrides)
    }
}

impl TextStyle {
    pub fn with_overrides(mut self, overrides: &Self) -> Self {
        self.foreground.overlay(&overrides.foreground);
        self.background.overlay(&overrides.background);
        self.attr = self.attr.with_overrides(&overrides.attr);
        self
    }

    #[deprecated(note = "use `with_overrides` to make precedence explicit")]
    pub fn merge(self, overrides: &Self) -> Self {
        self.with_overrides(overrides)
    }
}

impl Style {
    /// Apply a reusable style patch.
    pub fn patch(&mut self, patch: &StylePatch) {
        patch(self);
    }

    /// Merge caller overrides over these defaults, preserving unset fields.
    pub fn with_overrides(mut self, overrides: &Self) -> Self {
        macro_rules! merge {
            ($($field:ident),+ $(,)?) => {
                $(self.$field.overlay(&overrides.$field);)+
            };
        }
        merge!(
            layout, width, height, line, column, margin, padding, gap, justify, align, overflow,
            visibility, z_index, background, fill,
        );
        self.border = self.border.with_overrides(&overrides.border);
        self.text = self.text.with_overrides(&overrides.text);
        self
    }

    #[deprecated(note = "use `with_overrides` to make precedence explicit")]
    pub fn merge(self, overrides: &Self) -> Self {
        self.with_overrides(overrides)
    }
}

impl ops::DivAssign<&str> for Attr<Fill> {
    fn div_assign(&mut self, rhs: &str) {
        *self /= Fill::new(rhs).expect("style fill must be one printable terminal grapheme");
    }
}

impl ops::DivAssign<char> for Attr<Fill> {
    fn div_assign(&mut self, rhs: char) {
        *self /=
            Fill::new(rhs.to_string()).expect("style fill must be one printable terminal grapheme");
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
}

impl DomProps {
    /// Merge caller DOM props over component defaults field by field.
    pub fn with_overrides(mut self, overrides: &Self) -> Self {
        self.style = self.style.with_overrides(&overrides.style);
        self.events.merge(&overrides.events);
        self
    }

    /// Compatibility spelling for [`DomProps::with_overrides`].
    #[deprecated(note = "use `with_overrides` to make precedence explicit")]
    pub fn merge(self, overrides: &Self) -> Self {
        self.with_overrides(overrides)
    }
}

#[derive(Clone)]
pub struct Props<T> {
    /// DOM props supplied to this component. Function components must
    /// explicitly forward these to a host [`crate::view`] to make them
    /// visible; logical components do not create a renderer node themselves.
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

    /// Merge caller-supplied host props over component defaults.
    pub fn host_props(&self, defaults: DomProps) -> DomProps {
        defaults.with_overrides(&self.dom)
    }

    /// Collect this component's children into a logical fragment node.
    pub fn children_node(&self) -> Node {
        self.children.clone().into_iter().collect()
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
