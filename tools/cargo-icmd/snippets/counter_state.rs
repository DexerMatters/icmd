//! A controlled counter that explains `set` versus `update`.

use icmd::{
    ButtonVariant, ComponentContext, Dimension, Node, Props, StateSetter, Text, button, card, row,
    ui,
};

/// Visible state owned by the component and rendered through a controlled value.
pub fn counter(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (count, set_count) = cx.use_state(|| 0_i64);
    let decrement = set_count.clone();
    let increment = set_count.clone();

    ui! {
        <card style={|style| { style.gap /= 1; }}>
            {Text::new(format!("count = {count}")).bold()}
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 1; }}>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| decrement.update(|value| *value -= 1)}>"−1"</button>
                <button on_press={move |_| increment.update(|value| *value += 1)}>"+1"</button>
                <button variant={ButtonVariant::Destructive}
                    on_press={move |_| set_count.set(0)}>"reset"</button>
            </row>
        </card>
    }
}

/// A helper shows the setter is a plain cloneable value, not a borrow of state.
pub fn bump(setter: &StateSetter<i64>, by: i64) {
    setter.update(move |value| *value += by);
}
