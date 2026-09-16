//! The box vocabulary assembled into one miniature screen.

use icmd::{
    Align, ComponentContext, Dimension, Edges, Justify, Layout, Node, Props, Text, card, center,
    column, container, footer, heading, muted, row, section, spacer, ui, view,
};

/// One screen built from every layout primitive, each in its intended role.
pub fn layout_anatomy(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    ui! {
        <container>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
            }}>
                <heading>"Deploy console"</heading>
                <muted>"staging"</muted>
            </row>
            <section>
                <card>
                    <row style={|style| { style.width /= Dimension::Max; style.gap /= 1; }}>
                        {Text::new("build").bold()}
                        <spacer />
                        {Text::new("ready")}
                    </row>
                </card>
                <center style={|style| { style.height /= Dimension::Cells(3); }}>
                    <muted>"center() aligns both axes"</muted>
                </center>
            </section>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.height /= Dimension::Max;
                style.gap /= 1;
            }}>
                <column style={|style| { style.width /= Dimension::Cells(20); }}>
                    <muted>"column"</muted>
                    <muted>"stacks vertically"</muted>
                </column>
                <column style={|style| {
                    style.width /= Dimension::Max;
                    style.padding /= Edges::all(1);
                }}>
                    <muted>"the remaining width"</muted>
                </column>
            </view>
            <footer>
                <muted>"footer() is a muted horizontal row"</muted>
            </footer>
        </container>
    }
}
