// A downstream application built only from the high-level tiers. It must not
// name `icmd::advanced`, any `__ui_*` helper, or an internal module.
use icmd::MarkdownProps;
use icmd::events::{EventListener, KeyEvent, TerminalFocusEvent};
use icmd::theme::{Theme, ThemeMode, ThemePreset};
use icmd::widgets::{
    button, column, input, link, markdown, raster_image, scroll_area, selection_area, text, view,
};
use icmd::{AppLifecycle, AppPhase, render_with};
use icmd::{Component, ComponentContext, Node, Props, RuntimeConfig, TextClipboardEvent};
use icmd::{Dimension, Style};

fn app(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    // The session handle is part of the high-level facade: a button press can
    // stop the application gracefully.
    let exit = cx.use_handle();
    icmd::ui! {
        <column style={|style: &mut Style| { style.width /= Dimension::Max; }}>
            "hello"
            <input />
            <button on_click={|_| {}}>"press"</button>
            <button on_press={move |_| exit.request_exit()}>"quit"</button>
            <link href="https://example.com/icmd" on_follow={|_target: String| {}}>"docs"</link>
            <selection_area on_clipboard={|_event: TextClipboardEvent| {}}>
                "selectable text"
            </selection_area>
            <markdown text="# **Markdown**" />
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
    let _ = (
        button,
        input,
        raster_image,
        scroll_area,
        selection_area,
        view,
        markdown,
    );
    let _ = MarkdownProps::default();
    let _ = text(String::from("typed"));
    let _ = AppPhase::Exit;
}

// The lifecycle tier is part of the high-level facade: this compiles against
// `AppLifecycle` and `render_with` without naming `icmd::advanced`.
#[allow(dead_code)]
fn lifecycle_entry(node: Node, config: RuntimeConfig) -> Result<(), icmd::RenderError> {
    render_with(
        node,
        config,
        AppLifecycle::new()
            .on_boot(|_session| {})
            .on_exit(|_session| {}),
    )
}

fn main() {
    let _ = RuntimeConfig::default();
    println!("high-level fixture builds");
}
