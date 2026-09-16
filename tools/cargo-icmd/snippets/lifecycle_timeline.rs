//! The application lifecycle with a hook in each phase.

use icmd::{
    AppLifecycle, Component, ComponentContext, ExitReason, Node, Props, RuntimeConfig, Text,
    column, heading, paragraph, render_with, ui,
};

fn main() -> Result<(), icmd::RenderError> {
    let lifecycle = AppLifecycle::new()
        .on_boot(|session| {
            debug_assert_eq!(session.phase, icmd::AppPhase::Boot);
        })
        .on_mount(|session| {
            let _ = session.viewport;
        })
        .on_ready(|session| {
            let _ = session.frames_presented;
        })
        .on_unmount(|session| {
            let _ = session.exit;
        })
        .on_exit(|session| {
            if session.exit == Some(ExitReason::ExitKey) {
                eprintln!("exited on the configured key");
            }
        });

    render_with(app.apply(()), RuntimeConfig::default(), lifecycle)
}

/// A component whose tree is handed to the lifecycle-aware entry point.
fn app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    ui! {
        <column style={|style| { style.gap /= 1; }}>
            <heading>"Lifecycle"</heading>
            <paragraph>{Text::new("Boot, Mount, Ready, Unmount, Exit.")}</paragraph>
        </column>
    }
}
