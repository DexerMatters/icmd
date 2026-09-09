use icmd::{
    Attr, Component, ComponentContext, Node, Props, RuntimeConfig, button, paragraph, render, ui,
    vbox,
};

#[derive(Default)]
struct StatusProps {
    message: Attr<String>,
}

fn status(_cx: &mut ComponentContext, props: &Props<StatusProps>) -> Node {
    ui! { <paragraph dom={props.dom.clone()}>{props.message.clone() | String::new()}</paragraph> }
}

fn app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let caption = "Save";
    ui! {
        <vbox>
            <status message="Ready" />
            <button on_click={|_| {}}>{caption}</button>
            <>
                "Built with ui!"
                <status message={String::from("typed component props")} />
            </>
        </vbox>
    }
}

fn main() -> Result<(), icmd::RenderError> {
    render(app.apply(()), RuntimeConfig::default())
}
