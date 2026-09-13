// A downstream application built only from the high-level tiers. It must not
// name `icmd::advanced`, any `__ui_*` helper, or an internal module.
use icmd::events::{EventListener, KeyEvent, TerminalFocusEvent};
use icmd::style::{Dimension, Style};
use icmd::theme::{Theme, ThemeMode, ThemePreset};
use icmd::widgets::{button, column, input, raster_image, scroll_area, text, view};
use icmd::{Component, ComponentContext, Node, Props, RuntimeConfig};

fn app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    icmd::ui! {
        <column style={|style: &mut Style| { style.width /= Dimension::Max; }}>
            "hello"
            <input />
            <button on_click={|_| {}}>"press"</button>
        </column>
    }
}

#[allow(dead_code)]
fn uses_only_high_level_tiers() {
    let _ = ThemePreset::Nord.theme(ThemeMode::Dark);
    let _ = Theme::light();
    let _ = RuntimeConfig::default();
    let _ = TerminalFocusEvent::Gained;
    let _ = EventListener::new(|_event: KeyEvent| {});
    let _node: Node = app.apply(());
    let _ = (button, input, raster_image, scroll_area, view);
    let _ = text(String::from("typed"));
}

fn main() {
    let _ = RuntimeConfig::default();
    println!("high-level fixture builds");
}
