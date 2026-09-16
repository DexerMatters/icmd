//! The smallest complete `icmd` program: imports, a component, and `render`.

use icmd::{
    Component, ComponentContext, Edges, Node, Props, RuntimeConfig, column, heading, paragraph,
    render, ui,
};

fn main() -> Result<(), icmd::RenderError> {
    render(app.apply(()), RuntimeConfig::default())
}

/// A component is an ordinary function over the context and typed props.
fn app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    ui! {
        <column style={|style| {
            style.width /= icmd::Dimension::Max;
            style.padding /= Edges::all(1);
            style.gap /= 1;
        }}>
            <heading>"Hello from icmd"</heading>
            <paragraph>"A terminal-native component tree."</paragraph>
        </column>
    }
}
