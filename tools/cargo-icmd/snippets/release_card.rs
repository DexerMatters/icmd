//! A static release-status card assembled from ordinary widgets.

use icmd::{
    BadgeVariant, ButtonVariant, ComponentContext, Dimension, Justify, Node, Props, badge, button,
    card, divider, heading, label, muted, paragraph, row, ui,
};

/// Release-status card: heading, badge, divider, metadata row, and actions.
pub fn release_card(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= icmd::Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                <heading>"icmd 0.2.0"</heading>
                <badge text={"STABLE"} variant={BadgeVariant::Secondary} />
            </row>
            <paragraph>"A retained, terminal-native UI framework with cell-aware layout and Unicode text."</paragraph>
            <divider />
            <row style={|style| { style.gap /= 2; style.align /= icmd::Align::Center; }}>
                <muted>"target"</muted>
                <label>"x86_64-unknown-linux-gnu"</label>
            </row>
            <row style={|style| { style.gap /= 1; }}>
                <button on_press={|_| {}}>"Read the guide"</button>
                <button variant={ButtonVariant::Secondary} on_press={|_| {}}>"Release notes"</button>
            </row>
        </card>
    }
}
