use crate::{
    Attr, DomProps, Edges, Layout, Node, Props, Span, Text,
    basic::{ComponentContext, view},
    ui,
};
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgressBarProps {
    pub value: Attr<u64>,
    pub max: Attr<u64>,
    pub width: Attr<u16>,
    pub show_percentage: Attr<bool>,
    pub label: Attr<String>,
}

pub fn progress_bar(cx: &mut ComponentContext, props: &Props<ProgressBarProps>) -> Node {
    let theme = cx.use_theme();
    let max = (props.max | 100).max(1);
    let value = (props.value | 0).min(max);
    let width = usize::from(props.width | 20);
    let filled =
        ((u128::from(value) * width as u128 + u128::from(max) / 2) / u128::from(max)) as usize;
    let percentage = (u128::from(value) * 100 / u128::from(max)) as u64;

    let label = props.label.clone() | String::new();
    let mut spans = Vec::new();
    if !label.is_empty() {
        spans.push(Span::new(format!("{label} ")).style(theme.typography.label.clone()));
    }
    if filled > 0 {
        spans.push(Span::new("█".repeat(filled)).foreground(theme.colors.primary));
    }
    if filled < width {
        spans.push(Span::new("░".repeat(width - filled)).foreground(theme.colors.muted));
    }
    if props.show_percentage | true {
        spans.push(Span::new(format!(" {percentage:>3}%")).style(theme.typography.muted.clone()));
    }
    ui! { <view dom={props.dom.clone()}>{Text::from_spans(spans)}</view> }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpinnerProps {
    pub frame: Attr<usize>,
    pub label: Attr<String>,
}

pub fn spinner(cx: &mut ComponentContext, props: &Props<SpinnerProps>) -> Node {
    const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let theme = cx.use_theme();
    let label = props.label.clone() | String::new();
    let mut spans =
        vec![Span::new(FRAMES[(props.frame | 0) % FRAMES.len()]).foreground(theme.colors.primary)];
    if !label.is_empty() {
        spans.push(Span::new(format!(" {label}")).style(theme.typography.body.clone()));
    }
    ui! { <view dom={props.dom.clone()}>{Text::from_spans(spans)}</view> }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BadgeVariant {
    #[default]
    Primary,
    Secondary,
    Accent,
    Muted,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BadgeProps {
    pub text: Attr<String>,
    pub variant: Attr<BadgeVariant>,
}

pub fn badge(cx: &mut ComponentContext, props: &Props<BadgeProps>) -> Node {
    let theme = cx.use_theme();
    let (background, foreground) = match props.variant | BadgeVariant::Primary {
        BadgeVariant::Primary => (theme.colors.primary, theme.colors.primary_foreground),
        BadgeVariant::Secondary => (theme.colors.secondary, theme.colors.secondary_foreground),
        BadgeVariant::Accent => (theme.colors.accent, theme.colors.accent_foreground),
        BadgeVariant::Muted => (theme.colors.muted, theme.colors.muted_foreground),
        BadgeVariant::Destructive => (
            theme.colors.destructive,
            theme.colors.destructive_foreground,
        ),
    };
    let text = props.text.clone() | String::new();
    ui! {
        <view dom={props.dom.clone()}>{Text::new(format!(" {text} "))
            .foreground(foreground)
            .background(background)
            .bold()}</view>
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AlertVariant {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AlertProps {
    pub title: Attr<String>,
    pub message: Attr<String>,
    pub variant: Attr<AlertVariant>,
}

pub fn alert(cx: &mut ComponentContext, props: &Props<AlertProps>) -> Node {
    let theme = cx.use_theme();
    let accent = match props.variant | AlertVariant::Info {
        AlertVariant::Info => theme.colors.primary,
        AlertVariant::Success => theme.colors.secondary,
        AlertVariant::Warning => theme.colors.accent,
        AlertVariant::Error => theme.colors.destructive,
    };
    let title_text = props.title.clone() | String::new();
    let message_text = props.message.clone() | String::new();
    let mut style = crate::Style::default();
    style.layout /= Layout::Vertical;
    style.gap /= theme.spacing.xs;
    style.padding /= Edges {
        top: 0,
        right: 0,
        bottom: 0,
        left: theme.spacing.sm,
    };
    style.background /= theme.colors.card;
    style.text.foreground /= theme.colors.card_foreground;
    style.border.kind /= theme.borders.kind;
    style.border.edges /= Edges {
        top: false,
        right: false,
        bottom: false,
        left: true,
    };
    style.border.foreground /= accent;
    style.border.background /= theme.colors.card;
    let dom = props.host_props(DomProps {
        style,
        ..DomProps::default()
    });
    ui! {
        <view dom={dom}>
            {Text::new(title_text).foreground(accent).bold()}
            {Text::new(message_text).style(theme.typography.body.clone())}
        </view>
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SkeletonProps {
    pub width: Attr<u16>,
}

pub fn skeleton(cx: &mut ComponentContext, props: &Props<SkeletonProps>) -> Node {
    let theme = cx.use_theme();
    ui! {
        <view dom={props.dom.clone()}>{Text::new("░".repeat(usize::from(props.width | 12)))
            .foreground(theme.colors.muted_foreground)
            .background(theme.colors.muted)}</view>
    }
}
