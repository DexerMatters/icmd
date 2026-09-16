//! A reusable panel that accepts arbitrary children.

use icmd::{
    ComponentContext, Dimension, Edges, Layout, Node, Props, Text, TextWrap, card, heading,
    paragraph, ui,
};

/// A panel wraps caller-supplied content in a titled card surface.
pub fn panel(cx: &mut ComponentContext, props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let border = theme.colors.border;
    ui! {
        <card style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
            style.padding /= Edges::all(1);
            style.border.foreground /= border;
        }}>
            <heading>"Panel"</heading>
            <paragraph>{Text::new("Children arrive from the caller.").wrap(TextWrap::Soft)}</paragraph>
            {props.children_node()}
        </card>
    }
}
