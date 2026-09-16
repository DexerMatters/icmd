//! Selection controls: checkbox, radio, and switch.

use crossterm::style::Color;

use crate::{
    Attr, DomProps, EventListener, Node, Props, Span, Text, basic::ComponentContext, theme::Theme,
    ui,
};

/// The controls add no host styling of their own; the themed visuals are the
/// child text node.
fn icmd_dom() -> DomProps {
    DomProps::default()
}

use super::interactive::{Activation, interactive};

/// Configuration for [`checkbox`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckboxProps {
    /// Whether the box renders checked; defaults to `false`.
    pub checked: Attr<bool>,
    /// Text shown after the box; defaults to empty.
    pub label: Attr<String>,
    /// Whether activation and focus are refused; defaults to `false`.
    pub disabled: Attr<bool>,
    /// Whether the control requests focus on mount; defaults to `false`.
    pub autofocus: Attr<bool>,
    /// Listener for the proposed new state. The component is controlled: it
    /// renders `checked` and requests the toggle rather than mutating itself.
    pub on_change: Attr<EventListener<bool>>,
}

/// Render a selection control's symbol and label with the given selected,
/// disabled, and inactive colors.
fn selection_control(
    theme: &Theme,
    selected: bool,
    disabled: bool,
    label: String,
    symbols: (&str, &str),
    inactive: Color,
) -> Node {
    let color = if disabled {
        theme.colors.muted_foreground
    } else if selected {
        theme.colors.primary
    } else {
        inactive
    };
    let label_style = if disabled {
        theme.typography.muted.clone()
    } else {
        theme.typography.body.clone()
    };
    ui! {
        {Text::from_spans([
            Span::new(if selected { symbols.0 } else { symbols.1 }).foreground(color),
            Span::new(format!(" {label}")).style(label_style),
        ])}
    }
}

/// Checkbox control; see [`CheckboxProps`] for its configuration.
pub fn checkbox(cx: &mut ComponentContext, props: &Props<CheckboxProps>) -> Node {
    let theme = cx.use_theme();
    let checked = props.checked | false;
    let disabled = props.disabled | false;
    let child = selection_control(
        &theme,
        checked,
        disabled,
        props.label.clone() | String::new(),
        ("☑", "☐"),
        theme.colors.foreground,
    );
    let on_change = props.on_change.as_ref().cloned();
    let activation = Activation::new(disabled, props.autofocus | false).on_activate(move || {
        if let Some(listener) = &on_change {
            listener.call(!checked);
        }
    });
    interactive(icmd_dom(), &props.dom, activation, vec![child])
}

/// Configuration for [`radio`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RadioProps {
    /// Whether the radio renders selected; defaults to `false`.
    pub selected: Attr<bool>,
    /// Text shown after the radio; defaults to empty.
    pub label: Attr<String>,
    /// Whether activation and focus are refused; defaults to `false`.
    pub disabled: Attr<bool>,
    /// Whether the control requests focus on mount; defaults to `false`.
    pub autofocus: Attr<bool>,
    /// Listener for the selection request. Selection is exclusive, so the
    /// request carries no value: the group owner decides which radio becomes
    /// selected.
    pub on_select: Attr<EventListener<()>>,
}

/// Radio control; see [`RadioProps`] for its configuration.
pub fn radio(cx: &mut ComponentContext, props: &Props<RadioProps>) -> Node {
    let theme = cx.use_theme();
    let disabled = props.disabled | false;
    let child = selection_control(
        &theme,
        props.selected | false,
        disabled,
        props.label.clone() | String::new(),
        ("◉", "○"),
        theme.colors.foreground,
    );
    let on_select = props.on_select.as_ref().cloned();
    let activation = Activation::new(disabled, props.autofocus | false).on_activate(move || {
        if let Some(listener) = &on_select {
            listener.call(());
        }
    });
    interactive(icmd_dom(), &props.dom, activation, vec![child])
}

/// Configuration for [`switch`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwitchProps {
    /// Whether the switch renders on; defaults to `false`.
    pub on: Attr<bool>,
    /// Text shown after the switch; defaults to empty.
    pub label: Attr<String>,
    /// Whether activation and focus are refused; defaults to `false`.
    pub disabled: Attr<bool>,
    /// Whether the control requests focus on mount; defaults to `false`.
    pub autofocus: Attr<bool>,
    /// Listener for the proposed new state; defaults to none.
    pub on_change: Attr<EventListener<bool>>,
}

/// Switch control; see [`SwitchProps`] for its configuration.
pub fn switch(cx: &mut ComponentContext, props: &Props<SwitchProps>) -> Node {
    let theme = cx.use_theme();
    let on = props.on | false;
    let disabled = props.disabled | false;
    let child = selection_control(
        &theme,
        on,
        disabled,
        props.label.clone() | String::new(),
        ("━●", "●━"),
        theme.colors.muted_foreground,
    );
    let on_change = props.on_change.as_ref().cloned();
    let activation = Activation::new(disabled, props.autofocus | false).on_activate(move || {
        if let Some(listener) = &on_change {
            listener.call(!on);
        }
    });
    interactive(icmd_dom(), &props.dom, activation, vec![child])
}
