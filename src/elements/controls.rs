use crossterm::style::Color;

use crate::{
    Attr, DomProps, EventListener, Node, Props, Span, Text, basic::ComponentContext, theme::Theme,
    ui,
};

// The controls add no host styling of their own; the themed visuals are the
// child text node.
fn icmd_dom() -> DomProps {
    DomProps::default()
}

use super::interactive::{Activation, interactive};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckboxProps {
    pub checked: Attr<bool>,
    pub label: Attr<String>,
    pub disabled: Attr<bool>,
    pub autofocus: Attr<bool>,
    // Proposed new state. The component is controlled: it renders `checked` and
    // requests the toggle rather than mutating itself.
    pub on_change: Attr<EventListener<bool>>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RadioProps {
    pub selected: Attr<bool>,
    pub label: Attr<String>,
    pub disabled: Attr<bool>,
    pub autofocus: Attr<bool>,
    // Selection is exclusive, so the request carries no value: the group owner
    // decides which radio becomes selected.
    pub on_select: Attr<EventListener<()>>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwitchProps {
    pub on: Attr<bool>,
    pub label: Attr<String>,
    pub disabled: Attr<bool>,
    pub autofocus: Attr<bool>,
    pub on_change: Attr<EventListener<bool>>,
}

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
