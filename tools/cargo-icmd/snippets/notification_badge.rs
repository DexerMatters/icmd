//! A notification badge positioned over a card with absolute layout.

use icmd::{
    Align, AxisPosition, BadgeVariant, ComponentContext, Dimension, Edges, Justify, Layout, Node,
    Props, Text, badge, card, muted, row, ui,
};

/// Absolute placement plus `z_index` puts the badge where flow layout cannot.
pub fn notification_badge(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    ui! {
        <card style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
            style.padding /= Edges::all(1);
        }}>
            <row style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.height /= Dimension::Cells(4);
            }}>
                <card style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                    style.padding /= Edges::all(1);
                }}>
                    {Text::new("Inbox").bold()}
                    <muted>"three unread messages"</muted>
                </card>
                <badge text={"3"} variant={BadgeVariant::Destructive}
                    style={|style| {
                        style.layout /= Layout::Absolute;
                        style.line /= AxisPosition::Cells(0);
                        style.column /= AxisPosition::Cells(28);
                        style.z_index /= 10;
                    }} />
            </row>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::End;
            }}>
                <muted>"the badge paints above the card"</muted>
            </row>
        </card>
    }
}
