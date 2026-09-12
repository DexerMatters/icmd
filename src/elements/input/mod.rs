//! `raw_input`, `input`, and `textarea`.
//!
//! This module owns text-entry behavior. `raw_input` is the single primitive;
//! `input` and `textarea` are thin policy/theme wrappers over it. Neither
//! wrapper holds edit state, geometry helpers, or internal event handling.
//!
//! # Value ownership
//!
//! `value` means *controlled* and `default_value` is read exactly once when the
//! control is uncontrolled. Supplying both is permitted, and `value` wins.
//!
//! A controlled field follows an explicit, revision-based contract with no
//! value-comparison heuristics:
//!
//! 1. At every render the supplied `value` is authoritative. The component
//!    normalizes it and records a new render revision.
//! 2. Every input event dispatched before the next render reduces against one
//!    optimistic draft, so rapid or repeated keystrokes accumulate without
//!    waiting for the owner.
//! 3. At the next render, if the supplied value equals the emitted draft the
//!    draft's selection is restored (*acceptance*). Otherwise the selection is
//!    clamped into the supplied value (*rejection* or external replacement).
//! 4. Switching from uncontrolled to controlled adopts `value`; switching back
//!    retains the last authoritative rendered value and resumes local
//!    ownership.
//!
//! The owner therefore has final authority at every render, and rejection is
//! predictable rather than inferred.
//!
//! # Normalization, Unicode, and length
//!
//! Single-line mode maps CR, LF, and tab to spaces and discards other control
//! characters. Multiline mode canonicalizes CRLF/CR to LF, keeps newlines and
//! tabs, and discards other control characters. `max_length` counts extended
//! grapheme clusters in the normalized value *after* the current selection is
//! replaced, so a combining mark that merges with its base is measured in
//! context.
//!
//! Layout, wrapping, caret placement, and pointer hit-testing all use extended
//! grapheme clusters and terminal cells; source byte offsets are UTF-8 offsets
//! that are always valid grapheme boundaries. Wide graphemes split at their
//! halfway cell for pointer hits, and a width-two grapheme in a one-cell
//! viewport keeps its leading boundary visible rather than scrolling into a
//! continuation cell.
//!
//! # Extension and styling
//!
//! Extension means wrapping `raw_input` in an ordinary function component and
//! forwarding props, style, and events; there is no subclassing and no private
//! API. Children passed to `raw_input` are **ignored**, because arbitrary nodes
//! cannot be mapped to source positions.
//!
//! Sizing has exactly one owner: `props.dom.style`. Style dimensions are
//! border-box dimensions, and caller style overrides component defaults field
//! by field. Internal handlers and caller observers share one event slot: the
//! editor reduces state first and the caller's observer then runs on the same
//! host. Keys the editor handles stop propagation so ancestors do not also act
//! on them; unrecognized keys continue to ancestors.
//!
//! Focus is the standard `DomProps.events.focus_event`; there is no separate
//! input-specific focus callback.
//!
//! # Not supported
//!
//! Undo/redo history, validation, form integration, IME composition, terminal
//! clipboard ownership, password masking, and platform-specific shortcut
//! remapping are out of scope. The edit model leaves room for them without
//! claiming support.

pub(crate) mod model;
pub(crate) mod view;

use crate::{
    Attr, Component, Dimension, DomProps, Edges, EventListener, Justify, Layout, Node, Overflow,
    Props, Style, TextWrap, basic::ComponentContext, theme::Theme,
};

pub use view::{RawInputAppearance, RawInputMode, RawInputProps, raw_input};

/// A text value carried by change and submit events.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextValueEvent {
    pub value: String,
}

/// The clipboard gesture a [`TextClipboardEvent`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextClipboardAction {
    #[default]
    Copy,
    Cut,
}

/// A clipboard request produced by the editor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextClipboardEvent {
    pub action: TextClipboardAction,
    pub text: String,
}

/// Semantic props for the single-line [`input`] control.
///
/// Sizing is `props.dom.style` only; there is no width or height prop.
#[derive(Clone, Default)]
pub struct InputProps {
    pub value: Attr<String>,
    pub default_value: Attr<String>,
    pub placeholder: Attr<String>,
    pub max_length: Attr<usize>,
    pub disabled: Attr<bool>,
    pub read_only: Attr<bool>,
    pub on_change: Attr<EventListener<TextValueEvent>>,
    pub on_submit: Attr<EventListener<TextValueEvent>>,
    pub on_clipboard: Attr<EventListener<TextClipboardEvent>>,
}

impl std::fmt::Debug for InputProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputProps")
            .field("value", &self.value)
            .field("default_value", &self.default_value)
            .field("placeholder", &self.placeholder)
            .field("max_length", &self.max_length)
            .field("disabled", &self.disabled)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

/// Semantic props for the multiline [`textarea`] control.
///
/// Sizing is `props.dom.style` only; there is no width or height prop. Enter
/// inserts a newline, so there is no ambiguous submit gesture.
#[derive(Clone, Default)]
pub struct TextareaProps {
    pub value: Attr<String>,
    pub default_value: Attr<String>,
    pub placeholder: Attr<String>,
    pub wrap: Attr<TextWrap>,
    pub max_length: Attr<usize>,
    pub disabled: Attr<bool>,
    pub read_only: Attr<bool>,
    pub on_change: Attr<EventListener<TextValueEvent>>,
    pub on_clipboard: Attr<EventListener<TextClipboardEvent>>,
}

impl std::fmt::Debug for TextareaProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextareaProps")
            .field("value", &self.value)
            .field("default_value", &self.default_value)
            .field("placeholder", &self.placeholder)
            .field("wrap", &self.wrap)
            .field("max_length", &self.max_length)
            .field("disabled", &self.disabled)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

/// Render a themed, single-line editable text control.
///
/// This is a thin `raw_input` specialization: it chooses single-line policy,
/// translates the theme into a host style and [`RawInputAppearance`], and
/// forwards the caller's DOM props and events. It contains no editing logic.
pub fn input(cx: &mut ComponentContext, props: &Props<InputProps>) -> Node {
    let theme = cx.use_theme();
    let dom = props.host_props(input_host(&theme));
    let raw = RawInputProps {
        mode: Attr::Set(RawInputMode::SingleLine),
        value: props.value.clone(),
        default_value: props.default_value.clone(),
        placeholder: props.placeholder.clone(),
        max_length: props.max_length,
        disabled: props.disabled,
        read_only: props.read_only,
        appearance: Attr::Set(appearance(&theme)),
        on_change: props.on_change.clone(),
        on_submit: props.on_submit.clone(),
        on_clipboard: props.on_clipboard.clone(),
        ..RawInputProps::default()
    };
    raw_input.apply(Props {
        dom,
        children: Vec::new(),
        user_defined: raw,
    })
}

/// Render a themed, multiline editable text control.
///
/// This is a thin `raw_input` specialization: it chooses multiline policy,
/// translates the theme into a host style and [`RawInputAppearance`], and
/// forwards the caller's DOM props and events. It contains no editing logic.
pub fn textarea(cx: &mut ComponentContext, props: &Props<TextareaProps>) -> Node {
    let theme = cx.use_theme();
    let dom = props.host_props(textarea_host(&theme));
    let raw = RawInputProps {
        mode: Attr::Set(RawInputMode::Multiline),
        value: props.value.clone(),
        default_value: props.default_value.clone(),
        placeholder: props.placeholder.clone(),
        wrap: props.wrap,
        max_length: props.max_length,
        disabled: props.disabled,
        read_only: props.read_only,
        appearance: Attr::Set(appearance(&theme)),
        on_change: props.on_change.clone(),
        on_clipboard: props.on_clipboard.clone(),
        ..RawInputProps::default()
    };
    raw_input.apply(Props {
        dom,
        children: Vec::new(),
        user_defined: raw,
    })
}

/// The editor appearance the theme translates to.
fn appearance(theme: &Theme) -> RawInputAppearance {
    use crate::TextStyle;
    RawInputAppearance {
        placeholder: TextStyle::default().foreground(theme.colors.muted_foreground),
        selection: TextStyle::default()
            .foreground(theme.colors.primary_foreground)
            .background(theme.colors.primary),
        selection_inactive: TextStyle::default()
            .foreground(theme.colors.muted_foreground)
            .background(theme.colors.muted),
        caret: TextStyle::default().reverse(),
        focused_border: Some(theme.colors.ring),
    }
}

/// The border-box host style for [`input`]: an underline rule under a padded
/// content row.
fn input_host(theme: &Theme) -> DomProps {
    let mut style = Style::default();
    style.layout /= Layout::Vertical;
    style.justify /= Justify::Start;
    style.align /= crate::Align::Start;
    style.background /= theme.colors.input;
    style.text.foreground /= theme.colors.foreground;
    style.padding /= Edges::symmetric(0, 1);
    style.border.kind /= theme.borders.kind;
    style.border.edges /= Edges {
        top: false,
        right: false,
        bottom: true,
        left: false,
    };
    style.border.foreground /= theme.colors.border;
    style.border.background /= theme.colors.input;
    style.overflow /= Overflow::Clip;
    style.overflow_x /= Overflow::Clip;
    style.overflow_y /= Overflow::Clip;
    // Border box: one 24-cell content row above a single underline row, with
    // one padding cell on each side.
    style.width /= Dimension::Cells(26);
    style.height /= Dimension::Cells(2);
    DomProps {
        style,
        ..DomProps::default()
    }
}

/// The border-box host style for [`textarea`]: a fully bordered,
/// vertically-padded multiline box.
fn textarea_host(theme: &Theme) -> DomProps {
    let mut style = Style::default();
    style.layout /= Layout::Vertical;
    style.justify /= Justify::Start;
    style.align /= crate::Align::Start;
    style.background /= theme.colors.input;
    style.text.foreground /= theme.colors.foreground;
    style.padding /= Edges::symmetric(1, 1);
    style.border.kind /= theme.borders.kind;
    style.border.edges /= Edges::all(true);
    style.border.foreground /= theme.colors.border;
    style.border.background /= theme.colors.input;
    style.overflow /= Overflow::Clip;
    style.overflow_x /= Overflow::Clip;
    style.overflow_y /= Overflow::Clip;
    // Border box: a 40x5 content box plus padding and a full border.
    style.width /= Dimension::Cells(44);
    style.height /= Dimension::Cells(9);
    DomProps {
        style,
        ..DomProps::default()
    }
}
