use icmd::{Attr, ComponentContext, Node, Props, ui, view};

#[derive(Default)]
struct PropsForPanel {
    title: Attr<String>,
}

fn panel(_cx: &mut ComponentContext, props: &Props<PropsForPanel>) -> Node {
    ui! {
        <view dom={props.dom.clone()}>{props.title.clone() | String::new()}</view>
    }
}

fn main() {
    let title = String::from("ready");
    let _node = ui! { <panel title={title} /> };
}
