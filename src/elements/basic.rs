//! Layout and typography primitives: containers, text elements, and the
//! interactive button.

use crate::{
    Align, Attr, BorderKind, Dimension, Edges, EventListener, Justify, Layout, Node, Props,
    basic::ComponentContext,
};

use super::interactive::{Activation, interactive};
use super::themed;

/// Configuration for [`button`].
///
/// A button is interactive: it owns press, disabled, and autofocus semantics
/// instead of leaving every caller to attach its own click listener.
#[derive(Clone, Default)]
pub struct ButtonProps {
    /// Whether activation and focus are refused; defaults to `false`.
    pub disabled: Attr<bool>,
    /// Whether the button requests focus on mount; defaults to `false`.
    pub autofocus: Attr<bool>,
    /// Listener invoked once per semantic activation, however it was triggered.
    pub on_press: Attr<EventListener<()>>,
    /// Presentation-only override of the themed style; defaults to
    /// [`ButtonVariant::Primary`].
    pub variant: Attr<ButtonVariant>,
}

/// The listener slot is opaque, so it is omitted from the debug output.
impl std::fmt::Debug for ButtonProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ButtonProps")
            .field("disabled", &self.disabled)
            .field("autofocus", &self.autofocus)
            .field("variant", &self.variant)
            .finish_non_exhaustive()
    }
}

/// Semantic role that selects a button's themed colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Primary call to action.
    #[default]
    Primary,
    /// Supporting action.
    Secondary,
    /// Destructive action.
    Destructive,
}

/// Full-size vertical container with the background color; children lay out top
/// to bottom with extra-small spacing.
pub fn container(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.width /= Dimension::Max;
        style.height /= Dimension::Max;
        style.gap /= theme.spacing.xs;
        style.background /= theme.colors.background;
    })
}

/// Vertical stack with small spacing between children.
pub fn column(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.gap /= theme.spacing.sm;
    })
}

/// Horizontal stack with small spacing between children.
pub fn row(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Horizontal;
        style.gap /= theme.spacing.sm;
    })
}

/// Full-width vertical group with medium spacing, used to separate page
/// sections.
pub fn section(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.width /= Dimension::Max;
        style.gap /= theme.spacing.md;
    })
}

/// Full-width row that centers its children on both axes.
pub fn center(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Horizontal;
        style.width /= Dimension::Max;
        style.align /= Align::Center;
        style.justify /= Justify::Center;
        style.gap /= theme.spacing.sm;
    })
}

/// Full-width vertical panel with card background, border, and symmetric
/// padding.
pub fn card(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.width /= Dimension::Max;
        style.gap /= theme.spacing.xs;
        style.padding /= Edges::symmetric(theme.spacing.xs, theme.spacing.sm);
        style.background /= theme.colors.card;
        style.text.foreground /= theme.colors.card_foreground;
        style.border.kind /= theme.borders.kind;
        style.border.edges /= theme.borders.edges;
        style.border.foreground /= theme.borders.foreground;
        style.border.background /= theme.colors.card;
    })
}

/// Heading-level text using the theme's heading typography.
pub fn heading(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.text = theme.typography.heading.clone();
    })
}

/// Body text laid out as a vertical block.
pub fn paragraph(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.text = theme.typography.body.clone();
    })
}

/// Text using the theme's label typography.
pub fn label(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.text = theme.typography.label.clone();
    })
}

/// De-emphasized text using the theme's muted typography.
pub fn muted(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.text = theme.typography.muted.clone();
    })
}

/// Inline code text on the muted background with horizontal padding.
pub fn code(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.padding /= Edges::symmetric(theme.spacing.xs, theme.spacing.sm);
        style.background /= theme.colors.muted;
        style.text = theme.typography.code.clone();
    })
}

/// Interactive button; see [`ButtonProps`] for its configuration.
///
/// Disabled suppresses activation and focus, not just color.
pub fn button(cx: &mut ComponentContext, props: &Props<ButtonProps>) -> Node {
    let theme = cx.use_theme();
    let disabled = props.disabled | false;
    let variant = props.variant | ButtonVariant::Primary;

    let (background, foreground, border) = match (disabled, variant) {
        (true, _) => (
            theme.colors.muted,
            theme.colors.muted_foreground,
            theme.colors.border,
        ),
        (false, ButtonVariant::Primary) => (
            theme.colors.primary,
            theme.colors.primary_foreground,
            theme.colors.primary,
        ),
        (false, ButtonVariant::Secondary) => (
            theme.colors.secondary,
            theme.colors.secondary_foreground,
            theme.colors.secondary,
        ),
        (false, ButtonVariant::Destructive) => (
            theme.colors.destructive,
            theme.colors.destructive_foreground,
            theme.colors.destructive,
        ),
    };

    let mut style = crate::Style {
        text: theme.typography.body.clone(),
        ..crate::Style::default()
    };
    style.padding /= Edges::symmetric(theme.spacing.xs, theme.spacing.sm);
    style.background /= background;
    style.text.foreground /= foreground;
    style.text.attr.bold /= !disabled;
    style.border.kind /= theme.borders.kind;
    style.border.edges /= Edges::all(false);
    style.border.foreground /= border;
    style.border.background /= background;

    let on_press = props.on_press.as_ref().cloned();
    let activation = Activation::new(disabled, props.autofocus | false).on_activate(move || {
        if let Some(listener) = &on_press {
            listener.call(());
        }
    });
    interactive(
        crate::DomProps::default().with_style(style),
        &props.dom,
        activation,
        vec![props.children_node()],
    )
}

/// Full-width single-cell horizontal rule on the top edge.
pub fn divider(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.width /= Dimension::Max;
        style.height /= Dimension::Cells(1);
        style.border.kind /= BorderKind::Single;
        style.border.edges /= Edges {
            top: true,
            right: false,
            bottom: false,
            left: false,
        };
        style.border.foreground /= theme.colors.border;
    })
}

/// Fixed one-cell-square gap used to separate siblings.
pub fn spacer(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, _theme| {
        style.width /= Dimension::Cells(1);
        style.height /= Dimension::Cells(1);
    })
}

/// Horizontal row of muted text, used as a page footer.
pub fn footer(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Horizontal;
        style.gap /= theme.spacing.sm;
        style.text = theme.typography.muted.clone();
    })
}

/// Muted vertical block with a heavy left border and symmetric padding.
pub fn blockquote(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.padding /= Edges {
            top: theme.spacing.xs,
            right: theme.spacing.sm,
            bottom: theme.spacing.xs,
            left: theme.spacing.sm,
        };
        style.text = theme.typography.muted.clone();
        style.border.kind /= BorderKind::Heavy;
        style.border.edges /= Edges {
            top: false,
            right: false,
            bottom: false,
            left: true,
        };
        style.border.foreground /= theme.colors.accent;
    })
}

/// Keyboard-key text on the muted background with a border and label
/// typography.
pub fn kbd(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.padding /= Edges::symmetric(theme.spacing.xs, theme.spacing.sm);
        style.background /= theme.colors.muted;
        style.text = theme.typography.label.clone();
        style.border.kind /= theme.borders.kind;
        style.border.edges /= Edges::all(false);
        style.border.foreground /= theme.colors.border;
        style.border.background /= theme.colors.muted;
    })
}
