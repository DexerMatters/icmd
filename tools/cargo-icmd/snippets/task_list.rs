//! A keyed task list whose order can be reversed without losing row identity.

use icmd::{
    Align, ButtonVariant, ComponentContext, Dimension, Edges, Justify, Node, Props, Text, button,
    card, row, ui, view,
};

/// Keyed rows: reversing the order moves each row instead of rebuilding it.
pub fn task_list(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    const TASKS: [&str; 3] = ["write the guide", "commit frames", "ship the crate"];
    let (order, set_order) = cx.use_state(|| vec![0_usize, 1, 2]);

    let rows = order
        .iter()
        .enumerate()
        .map(|(position, id)| {
            let task = TASKS[*id];
            ui! {
                <view key={*id as u64} style={|style| {
                    style.layout /= icmd::Layout::Horizontal;
                    style.width /= Dimension::Max;
                    style.gap /= 1;
                    style.padding /= Edges::symmetric(0, 1);
                }}>
                    {Text::new(format!("{}.", position + 1))}
                    {Text::new(task)}
                </view>
            }
        })
        .collect::<Node>();

    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new("Task list").bold()}
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| set_order.update(|order| order.reverse())}>
                    "Reverse"
                </button>
            </row>
            {rows}
        </card>
    }
}
