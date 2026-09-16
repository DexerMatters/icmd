//! Status feedback: progress bars, spinners, badges, alerts, and skeletons.

use crate::{
    Attr, DomProps, Edges, Layout, Node, Props, Span, Text,
    basic::{ComponentContext, view},
    ui,
};
/// Configuration for [`progress_bar`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgressBarProps {
    /// Completed units; defaults to `0` and is clamped to `max`.
    pub value: Attr<u64>,
    /// Total units; defaults to `100` and is floored at `1`.
    pub max: Attr<u64>,
    /// Bar width in terminal cells; defaults to `20`.
    pub width: Attr<u16>,
    /// Whether the trailing percentage is shown; defaults to `true`.
    pub show_percentage: Attr<bool>,
    /// Optional text drawn before the bar; defaults to empty.
    pub label: Attr<String>,
}

/// Horizontal progress bar filled in proportion to `value / max`; see
/// [`ProgressBarProps`] for its configuration.
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

/// Configuration for [`spinner`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpinnerProps {
    /// Frame index into the ten-frame animation; defaults to `0` and wraps.
    pub frame: Attr<usize>,
    /// Optional text drawn after the frame; defaults to empty.
    pub label: Attr<String>,
}

/// Single-frame spinner; callers advance `frame` to animate it. See
/// [`SpinnerProps`] for its configuration.
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

/// Semantic role that selects a badge's themed colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BadgeVariant {
    /// Primary role.
    #[default]
    Primary,
    /// Secondary role.
    Secondary,
    /// Accent role.
    Accent,
    /// Muted role.
    Muted,
    /// Destructive role.
    Destructive,
}

/// Configuration for [`badge`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BadgeProps {
    /// Badge text, padded with one space on each side; defaults to empty.
    pub text: Attr<String>,
    /// Color role; defaults to [`BadgeVariant::Primary`].
    pub variant: Attr<BadgeVariant>,
}

/// Small solid-background label; see [`BadgeProps`] for its configuration.
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

/// Semantic role that selects an alert's accent color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AlertVariant {
    /// Informational accent.
    #[default]
    Info,
    /// Success accent.
    Success,
    /// Warning accent.
    Warning,
    /// Error accent.
    Error,
}

/// Configuration for [`alert`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AlertProps {
    /// Bold title line; defaults to empty.
    pub title: Attr<String>,
    /// Wrapped body message; defaults to empty.
    pub message: Attr<String>,
    /// Accent role; defaults to [`AlertVariant::Info`].
    pub variant: Attr<AlertVariant>,
}

/// Card with a colored left stripe, a bold title, and a softly wrapped
/// message; see [`AlertProps`] for its configuration.
///
/// The stripe keeps the solid role; the title falls back to the card
/// foreground when that role would be unreadable as text on the card.
pub fn alert(cx: &mut ComponentContext, props: &Props<AlertProps>) -> Node {
    let theme = cx.use_theme();
    let accent = match props.variant | AlertVariant::Info {
        AlertVariant::Info => theme.colors.primary,
        AlertVariant::Success => theme.colors.secondary,
        AlertVariant::Warning => theme.colors.accent,
        AlertVariant::Error => theme.colors.destructive,
    };
    let title_color = theme.on_card(accent);
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
            {Text::new(title_text).foreground(title_color).bold()}
            {Text::new(message_text)
                .text_style(theme.typography.body.clone())
                .wrap(crate::TextWrap::Soft)}
        </view>
    }
}

/// Configuration for [`skeleton`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SkeletonProps {
    /// Placeholder width in terminal cells; defaults to `12`.
    pub width: Attr<u16>,
}

/// Muted block of placeholder glyphs; see [`SkeletonProps`] for its
/// configuration.
pub fn skeleton(cx: &mut ComponentContext, props: &Props<SkeletonProps>) -> Node {
    let theme = cx.use_theme();
    ui! {
        <view dom={props.dom.clone()}>{Text::new("░".repeat(usize::from(props.width | 12)))
            .foreground(theme.colors.muted_foreground)
            .background(theme.colors.muted)}</view>
    }
}
