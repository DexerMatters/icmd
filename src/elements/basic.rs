use crate::{
    Align, BorderKind, Dimension, Edges, Justify, Layout, Node, Props, basic::ComponentContext,
};

use super::themed;

pub fn container(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.width /= Dimension::Max;
        style.height /= Dimension::Max;
        style.gap /= theme.spacing.xs;
        style.background /= theme.colors.background;
    })
}

pub fn vbox(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.gap /= theme.spacing.sm;
    })
}

pub fn hbox(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Horizontal;
        style.gap /= theme.spacing.sm;
    })
}

pub fn section(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.width /= Dimension::Max;
        style.gap /= theme.spacing.md;
    })
}

pub fn center(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Horizontal;
        style.width /= Dimension::Max;
        style.align /= Align::Center;
        style.justify /= Justify::Center;
        style.gap /= theme.spacing.sm;
    })
}

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

pub fn header(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.text = theme.typography.heading.clone();
    })
}

pub fn title(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    header(cx, props)
}

pub fn paragraph(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Vertical;
        style.text = theme.typography.body.clone();
    })
}

pub fn label(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.text = theme.typography.label.clone();
    })
}

pub fn muted(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.text = theme.typography.muted.clone();
    })
}

pub fn code(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.padding /= Edges::symmetric(theme.spacing.xs, theme.spacing.sm);
        style.background /= theme.colors.muted;
        style.text = theme.typography.code.clone();
    })
}

pub fn button(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.padding /= Edges::symmetric(theme.spacing.xs, theme.spacing.sm);
        style.background /= theme.colors.primary;
        style.text.foreground /= theme.colors.primary_foreground;
        style.text.attr.bold /= true;
        style.border.kind /= theme.borders.kind;
        style.border.edges /= Edges::all(false);
        style.border.foreground /= theme.colors.primary;
        style.border.background /= theme.colors.primary;
    })
}

pub fn input(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.padding /= Edges::symmetric(theme.spacing.xs, theme.spacing.sm);
        style.background /= theme.colors.input;
        style.text.foreground /= theme.colors.foreground;
        style.border.kind /= theme.borders.kind;
        style.border.edges /= Edges {
            top: false,
            right: false,
            bottom: true,
            left: false,
        };
        style.border.foreground /= theme.colors.border;
        style.border.background /= theme.colors.input;
    })
}

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

pub fn spacer(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, _theme| {
        style.width /= Dimension::Cells(1);
        style.height /= Dimension::Cells(1);
    })
}

pub fn footer(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    themed(cx, props, |style, theme| {
        style.layout /= Layout::Horizontal;
        style.gap /= theme.spacing.sm;
        style.text = theme.typography.muted.clone();
    })
}

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

pub use hbox as row;
pub use header as heading;
pub use vbox as column;
