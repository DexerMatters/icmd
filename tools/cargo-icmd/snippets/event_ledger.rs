//! A bounded, scrollable event ledger fed by a preferences form.

use icmd::{
    ComponentContext, Dimension, Edges, Node, Props, ScrollAxes, ScrollbarVisibility, Text, card,
    checkbox, muted, scroll_area, switch, ui, view,
};

/// Recent semantic events, bounded so interaction cannot grow memory.
pub fn event_ledger(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (telemetry, set_telemetry) = cx.use_state(|| true);
    let (verbose, set_verbose) = cx.use_state(|| false);
    let (log, set_log) = cx.use_state(Vec::<String>::new);

    let telemetry_log = set_log.clone();
    let verbose_log = set_log.clone();

    let entries = log
        .iter()
        .rev()
        .map(|entry| ui! { <muted>{Text::new(entry.clone())}</muted> })
        .collect::<Node>();

    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <checkbox checked={telemetry} label={"Send telemetry"}
                on_change={move |next: bool| {
                    set_telemetry.set(next);
                    telemetry_log.update(move |entries| {
                        entries.push(format!("telemetry -> {next}"));
                        while entries.len() > 8 { entries.remove(0); }
                    });
                }} />
            <switch on={verbose} label={"Verbose frames"}
                on_change={move |next: bool| {
                    set_verbose.set(next);
                    verbose_log.update(move |entries| {
                        entries.push(format!("verbose -> {next}"));
                        while entries.len() > 8 { entries.remove(0); }
                    });
                }} />
            <muted>"RECENT EVENTS"</muted>
            <scroll_area axes={ScrollAxes::Vertical}
                scrollbar_visibility={ScrollbarVisibility::Auto}
                style={|style| {
                    style.layout /= icmd::Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(6);
                    style.padding /= Edges::symmetric(0, 1);
                }}>
                <view style={|style| {
                    style.layout /= icmd::Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                }}>
                    {entries}
                </view>
            </scroll_area>
        </card>
    }
}
