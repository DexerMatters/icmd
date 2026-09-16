//! A preferences card whose summary updates through controlled props.

use icmd::{
    AlertVariant, ComponentContext, Dimension, Node, Props, Text, alert, card, checkbox, radio,
    row, switch, ui,
};

/// Every control reports the change it wants; the owner applies it.
pub fn preferences(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (telemetry, set_telemetry) = cx.use_state(|| true);
    let (updates, set_updates) = cx.use_state(|| String::from("stable"));
    let (density, set_density) = cx.use_state(|| 0_usize);

    let telemetry_change = set_telemetry.clone();
    let stable_change = set_updates.clone();
    let nightly_change = set_updates.clone();
    let density_change = set_density.clone();
    let summary = format!(
        "telemetry {} · updates {} · density {}",
        if telemetry { "on" } else { "off" },
        updates,
        ["comfortable", "compact", "dense"][density.min(2)]
    );

    ui! {
        <card style={|style| { style.gap /= 1; }}>
            {Text::new("Preferences").bold()}
            <checkbox
                checked={telemetry}
                label={"Send anonymous telemetry"}
                on_change={move |next: bool| telemetry_change.set(next)} />
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 2; }}>
                <radio
                    selected={updates == "stable"}
                    label={"Stable channel"}
                    on_select={move |_| stable_change.set(String::from("stable"))} />
                <radio
                    selected={updates == "nightly"}
                    label={"Nightly channel"}
                    on_select={move |_| nightly_change.set(String::from("nightly"))} />
            </row>
            <switch
                on={density == 1}
                label={"Compact rows"}
                on_change={move |next: bool| density_change.set(if next { 1 } else { 0 })} />
            <alert variant={AlertVariant::Info} title={"Live summary"} message={summary} />
        </card>
    }
}
