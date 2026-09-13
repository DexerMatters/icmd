pub(crate) mod model;
pub(crate) mod view;

use crate::{
    Attr, Component, Dimension, DomProps, Edges, EventListener, Justify, Layout, Node, Overflow,
    Props, Style, TextWrap, basic::ComponentContext, theme::Theme,
};

pub use view::{RawInputAppearance, RawInputMode, RawInputProps, raw_input};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextValueEvent {
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextClipboardAction {
    #[default]
    Copy,
    Cut,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextClipboardEvent {
    pub action: TextClipboardAction,
    pub text: String,
}

#[derive(Clone, Default)]
pub struct InputProps {
    pub value: Attr<String>,
    pub default_value: Attr<String>,
    pub placeholder: Attr<String>,
    pub max_length: Attr<usize>,
    pub disabled: Attr<bool>,
    pub read_only: Attr<bool>,
    pub autofocus: Attr<bool>,
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

#[derive(Clone, Default)]
pub struct TextareaProps {
    pub value: Attr<String>,
    pub default_value: Attr<String>,
    pub placeholder: Attr<String>,
    pub wrap: Attr<TextWrap>,
    pub max_length: Attr<usize>,
    pub disabled: Attr<bool>,
    pub read_only: Attr<bool>,
    pub autofocus: Attr<bool>,
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

// The fields both editor controls share. Single-line and multi-line policy is
// expressed separately in `EditorMode`, so common prop translation and listener
// wiring exist exactly once.
struct EditorHost {
    value: Attr<String>,
    default_value: Attr<String>,
    placeholder: Attr<String>,
    max_length: Attr<usize>,
    disabled: Attr<bool>,
    read_only: Attr<bool>,
    autofocus: Attr<bool>,
    on_change: Attr<EventListener<TextValueEvent>>,
    on_clipboard: Attr<EventListener<TextClipboardEvent>>,
}

enum EditorMode {
    SingleLine {
        on_submit: Attr<EventListener<TextValueEvent>>,
    },
    Multiline {
        wrap: Attr<TextWrap>,
    },
}

// One owner for host construction and raw-prop translation. Every semantic
// difference between the two public controls is explicit here rather than
// spread across two near-identical component bodies.
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
    // Border box: a 40x7 content box plus horizontal padding and a full border.
    style.width /= Dimension::Cells(44);
    style.height /= Dimension::Cells(9);
    DomProps {
        style,
        ..DomProps::default()
    }
}
