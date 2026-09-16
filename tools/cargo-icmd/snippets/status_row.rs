//! A reusable status row with typed props and defaults.

use icmd::{
    Align, Attr, BadgeVariant, ComponentContext, Dimension, Justify, Node, Props, Span, Text,
    badge, card, row, ui,
};

/// Props for [`status_row`]; every field is optional through `Attr`.
#[derive(Clone, Default)]
pub struct StatusRowProps {
    /// Row label.
    pub label: Attr<String>,
    /// Row value.
    pub value: Attr<String>,
    /// Badge role; defaults to [`BadgeVariant::Muted`].
    pub variant: Attr<BadgeVariant>,
}

/// Reusable status row: a label, a right-aligned value, and a role badge.
pub fn status_row(_cx: &mut ComponentContext, props: &Props<StatusRowProps>) -> Node {
    let label = props.data().label.clone() | String::from("unnamed");
    let value = props.data().value.clone() | String::from("—");
    let variant = props.data().variant | BadgeVariant::Muted;
    ui! {
        <card style={|style| { style.gap /= 0; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::from_spans([
                    Span::new(label).bold(),
                    Span::new("  "),
                    Span::new(value),
                ])}
                <badge text={variant_name(variant)} variant={variant} />
            </row>
        </card>
    }
}

/// Display name for a badge role; status always carries a textual cue.
fn variant_name(variant: BadgeVariant) -> String {
    match variant {
        BadgeVariant::Primary => "primary",
        BadgeVariant::Secondary => "ok",
        BadgeVariant::Accent => "notice",
        BadgeVariant::Muted => "idle",
        BadgeVariant::Destructive => "failed",
    }
    .to_string()
}
