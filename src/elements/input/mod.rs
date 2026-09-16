//! Text editor controls: the flat `input`/`textarea` props and the raw input
//! engine they share.

pub(crate) mod model;
pub(crate) mod view;

use crate::{
    Attr, Component, Dimension, DomProps, Edges, EventListener, Justify, Layout, Node, Overflow,
    Props, Style, TextWrap, basic::ComponentContext, theme::Theme,
};

#[doc(hidden)]
pub use model::normalize_for_test;
pub use view::{RawInputAppearance, RawInputMode, RawInputProps, raw_input};

/// The value carried by a text control's change or submit event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextValueEvent {
    /// The editor's current value.
    pub value: String,
}

/// Which clipboard operation a text control performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextClipboardAction {
    /// Text was copied to the clipboard.
    #[default]
    Copy,
    /// Text was removed and copied to the clipboard.
    Cut,
}

/// A clipboard operation reported by a text control.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextClipboardEvent {
    /// Operation that occurred.
    pub action: TextClipboardAction,
    /// Text involved in the operation.
    pub text: String,
}

/// Configuration for [`input`].
#[derive(Clone, Default)]
pub struct InputProps {
    /// Controlled value; when set, the control renders it and requests changes
    /// instead of mutating itself.
    pub value: Attr<String>,
    /// Initial value for an uncontrolled control.
    pub default_value: Attr<String>,
    /// Text shown while the value is empty.
    pub placeholder: Attr<String>,
    /// Maximum value length in display units; unbounded when unset.
    pub max_length: Attr<usize>,
    /// Whether editing and focus are refused; defaults to `false`.
    pub disabled: Attr<bool>,
    /// Whether the value may be selected and copied but not edited; defaults
    /// to `false`.
    pub read_only: Attr<bool>,
    /// Whether the control requests focus on mount; defaults to `false`.
    pub autofocus: Attr<bool>,
    /// Listener for each committed value change.
    pub on_change: Attr<EventListener<TextValueEvent>>,
    /// Listener for Enter in a single-line input.
    pub on_submit: Attr<EventListener<TextValueEvent>>,
    /// Listener for copy and cut operations answered by the control.
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

/// Configuration for [`textarea`].
#[derive(Clone, Default)]
pub struct TextareaProps {
    /// Controlled value; when set, the control renders it and requests changes
    /// instead of mutating itself.
    pub value: Attr<String>,
    /// Initial value for an uncontrolled control.
    pub default_value: Attr<String>,
    /// Text shown while the value is empty.
    pub placeholder: Attr<String>,
    /// Line wrapping mode; defaults to [`TextWrap::Soft`].
    pub wrap: Attr<TextWrap>,
    /// Maximum value length in display units; unbounded when unset.
    pub max_length: Attr<usize>,
    /// Whether editing and focus are refused; defaults to `false`.
    pub disabled: Attr<bool>,
    /// Whether the value may be selected and copied but not edited; defaults
    /// to `false`.
    pub read_only: Attr<bool>,
    /// Whether the control requests focus on mount; defaults to `false`.
    pub autofocus: Attr<bool>,
    /// Listener for each committed value change.
    pub on_change: Attr<EventListener<TextValueEvent>>,
    /// Listener for copy and cut operations answered by the control.
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

/// The fields both editor controls share. Single-line and multi-line policy is
/// expressed separately in `EditorMode`, so common prop translation and
/// listener wiring exist exactly once.
struct EditorHost {
    /// Controlled value forwarded to the raw input.
    value: Attr<String>,
    /// Uncontrolled initial value forwarded to the raw input.
    default_value: Attr<String>,
    /// Placeholder text forwarded to the raw input.
    placeholder: Attr<String>,
    /// Maximum length in display units forwarded to the raw input.
    max_length: Attr<usize>,
    /// Disabled flag forwarded to the raw input.
    disabled: Attr<bool>,
    /// Read-only flag forwarded to the raw input.
    read_only: Attr<bool>,
    /// Autofocus flag forwarded to the raw input.
    autofocus: Attr<bool>,
    /// Change listener forwarded to the raw input.
    on_change: Attr<EventListener<TextValueEvent>>,
    /// Clipboard listener forwarded to the raw input.
    on_clipboard: Attr<EventListener<TextClipboardEvent>>,
}

/// Per-control policy that selects the raw input mode and its extra wiring.
enum EditorMode {
    /// Single-line mode with an optional submit listener.
    SingleLine {
        /// Listener invoked when Enter submits the value.
        on_submit: Attr<EventListener<TextValueEvent>>,
    },
    /// Multi-line mode with the requested wrap mode.
    Multiline {
        /// Line wrapping mode for the textarea.
        wrap: Attr<TextWrap>,
    },
}

/// One owner for host construction and raw-prop translation. Every semantic
/// difference between the two public controls is explicit here rather than
/// spread across two near-identical component bodies.
fn editor_host(
    cx: &mut ComponentContext,
    host_dom: &DomProps,
    host: EditorHost,
    mode: EditorMode,
) -> Node {
    let theme = cx.use_theme();
    let (default_dom, raw_mode, wrap, on_submit) = match mode {
        EditorMode::SingleLine { on_submit } => (
            input_host(&theme),
            RawInputMode::SingleLine,
            Attr::Unset,
            on_submit,
        ),
        EditorMode::Multiline { wrap } => (
            textarea_host(&theme),
            RawInputMode::Multiline,
            wrap,
            Attr::Unset,
        ),
    };
    let dom = default_dom.with_overrides(host_dom);
    let raw = RawInputProps {
        mode: Attr::Set(raw_mode),
        value: host.value,
        default_value: host.default_value,
        placeholder: host.placeholder,
        wrap,
        max_length: host.max_length,
        disabled: host.disabled,
        read_only: host.read_only,
        autofocus: host.autofocus,
        appearance: Attr::Set(appearance(&theme)),
        on_change: host.on_change,
        on_submit,
        on_clipboard: host.on_clipboard,
    };
    let mut props = Props::new(raw);
    props.dom = dom;
    raw_input.apply(props)
}

/// Single-line text input; see [`InputProps`] for its configuration.
pub fn input(cx: &mut ComponentContext, props: &Props<InputProps>) -> Node {
    editor_host(
        cx,
        &props.dom,
        EditorHost {
            value: props.value.clone(),
            default_value: props.default_value.clone(),
            placeholder: props.placeholder.clone(),
            max_length: props.max_length,
            disabled: props.disabled,
            read_only: props.read_only,
            autofocus: props.autofocus,
            on_change: props.on_change.clone(),
            on_clipboard: props.on_clipboard.clone(),
        },
        EditorMode::SingleLine {
            on_submit: props.on_submit.clone(),
        },
    )
}

/// Multi-line text area; see [`TextareaProps`] for its configuration.
pub fn textarea(cx: &mut ComponentContext, props: &Props<TextareaProps>) -> Node {
    editor_host(
        cx,
        &props.dom,
        EditorHost {
            value: props.value.clone(),
            default_value: props.default_value.clone(),
            placeholder: props.placeholder.clone(),
            max_length: props.max_length,
            disabled: props.disabled,
            read_only: props.read_only,
            autofocus: props.autofocus,
            on_change: props.on_change.clone(),
            on_clipboard: props.on_clipboard.clone(),
        },
        EditorMode::Multiline { wrap: props.wrap },
    )
}

/// Derive the editor's appearance from the theme.
///
/// The selection colours come from the engine's one theme derivation, so an
/// editor and a selectable region present the same selection.
fn appearance(theme: &Theme) -> RawInputAppearance {
    use crate::TextStyle;
    let selection = crate::basic::selection::SelectionStyles::from_theme(theme);
    RawInputAppearance {
        placeholder: TextStyle::default().foreground(theme.colors.muted_foreground),
        selection: selection.active,
        selection_inactive: selection.inactive,
        caret: TextStyle::default().reverse(),
        focused_border: Some(theme.colors.ring),
    }
}

/// Default host for a single-line input: one 24-cell content row above a single
/// underline row, with one padding cell on each side (a 26x2 border box).
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
    style.width /= Dimension::Cells(26);
    style.height /= Dimension::Cells(2);
    DomProps {
        style,
        ..DomProps::default()
    }
}

/// Default host for a multi-line textarea: a 40x7 content box plus horizontal
/// padding and a full border (a 44x9 border box).
fn textarea_host(theme: &Theme) -> DomProps {
    let mut style = Style::default();
    style.layout /= Layout::Vertical;
    style.justify /= Justify::Start;
    style.align /= crate::Align::Start;
    style.background /= theme.colors.input;
    style.text.foreground /= theme.colors.foreground;
    style.padding /= Edges::symmetric(0, 1);
    style.border.kind /= theme.borders.kind;
    style.border.edges /= Edges::all(true);
    style.border.foreground /= theme.colors.border;
    style.border.background /= theme.colors.input;
    style.overflow /= Overflow::Clip;
    style.overflow_x /= Overflow::Clip;
    style.overflow_y /= Overflow::Clip;
    style.width /= Dimension::Cells(44);
    style.height /= Dimension::Cells(9);
    DomProps {
        style,
        ..DomProps::default()
    }
}
