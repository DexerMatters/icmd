//! A tour of the `ui!` forms: tags, text, expressions, fragments, and listeners.

use icmd::{
    ComponentContext, Dimension, Node, Props, Text, TextWrap, button, column, fragment, heading,
    paragraph, ui,
};

/// One rendered frame exercising every `ui!` child form.
pub fn ui_syntax(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (count, set_count) = cx.use_state(|| 0_u32);
    let rows = ["alpha", "beta", "gamma"]
        .iter()
        .map(|name| ui! { <paragraph>{Text::new(*name)}</paragraph> })
        .collect::<Node>();

    ui! {
        <column style={|style| {
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <heading>"ui! syntax"</heading>
            <paragraph>"Plain text children need no wrapper."</paragraph>
            {Text::new(format!("count = {count}")).wrap(TextWrap::Soft)}
            {fragment(vec![
                ui! { <paragraph>"A fragment groups siblings without a box."</paragraph> },
                ui! { <paragraph>"Every tag is an ordinary component call."</paragraph> },
            ])}
            {if count == 0 {
                ui! { <paragraph>"The conditional branch renders a node."</paragraph> }
            } else {
                ui! { <paragraph>"The other branch renders instead."</paragraph> }
            }}
            {rows}
            <button on_press={move |_| set_count.update(|value| *value += 1)}>"Increment"</button>
        </column>
    }
}
