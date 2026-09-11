use crossterm::style::Color;

use crate::{
    Attr, Node, Props, Span, Text,
    basic::{ComponentContext, view},
    theme::Theme,
    ui,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckboxProps {
    pub checked: Attr<bool>,
    pub label: Attr<String>,
    pub disabled: Attr<bool>,
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
    let child = selection_control(
        &theme,
        props.checked | false,
        props.disabled | false,
        props.label.clone() | String::new(),
        ("☑", "☐"),
        theme.colors.foreground,
    );
    ui! { <view dom={props.dom.clone()}>{child}</view> }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RadioProps {
    pub selected: Attr<bool>,
    pub label: Attr<String>,
    pub disabled: Attr<bool>,
}

pub fn radio(cx: &mut ComponentContext, props: &Props<RadioProps>) -> Node {
    let theme = cx.use_theme();
    let child = selection_control(
        &theme,
        props.selected | false,
        props.disabled | false,
        props.label.clone() | String::new(),
        ("◉", "○"),
        theme.colors.foreground,
    );
    ui! { <view dom={props.dom.clone()}>{child}</view> }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwitchProps {
    pub on: Attr<bool>,
    pub label: Attr<String>,
    pub disabled: Attr<bool>,
}

pub fn switch(cx: &mut ComponentContext, props: &Props<SwitchProps>) -> Node {
    let theme = cx.use_theme();
    let child = selection_control(
        &theme,
        props.on | false,
        props.disabled | false,
        props.label.clone() | String::new(),
        ("━●", "●━"),
        theme.colors.muted_foreground,
    );
    ui! { <view dom={props.dom.clone()}>{child}</view> }
}
