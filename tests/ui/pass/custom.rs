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

/// A custom field extends `raw_input` through ordinary composition: it sets
/// policy, forwards props and events, and adjusts style, without touching
/// crate-private state.
mod custom_field {
    use super::*;
    use icmd::{
        Attr, Component, Dimension, EventListener, Node, Props, RawInputMode, RawInputProps,
        TextValueEvent, raw_input, ui,
    };

    #[derive(Default)]
    pub struct CommandFieldProps {
        pub value: Attr<String>,
        pub on_change: Attr<EventListener<TextValueEvent>>,
    }

    pub fn command_field(cx: &mut ComponentContext, props: &Props<CommandFieldProps>) -> Node {
        let theme = cx.use_theme();
        let _ = &theme;
        let raw = RawInputProps {
            mode: Attr::Set(RawInputMode::SingleLine),
            value: props.value.clone(),
            on_change: props.on_change.clone(),
            ..RawInputProps::default()
        };
        raw_input
            .props(raw)
            .events({
                let observed = props.on_change.clone();
                move |handlers: &mut icmd::EventHandlers| {
                    handlers.pointer_down = Attr::Set(EventListener::new(move |_event| {
                        let _ = &observed;
                    }));
                }
            })
            .style(|style| style.width /= Dimension::Cells(20))
            .node()
    }

    pub fn build() -> Node {
        ui! {
            <command_field value={"cmd"} on_change={move |_event: TextValueEvent| {}} />
        }
    }
}
