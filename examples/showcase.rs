//! Overlay, weighted split, animated braille canvas, and titled panes in one
//! small application.

use std::sync::Arc;

use icmd::{
    CanvasContext, Component, ComponentContext, Layout, Node, Props, RuntimeConfig, canvas, dialog,
    pane, render, split, text, ui,
};

fn plot(tick: u64) -> Arc<dyn Fn(&mut CanvasContext) + Send + Sync> {
    Arc::new(move |drawing: &mut CanvasContext| {
        let width = drawing.dot_width() as i32;
        let height = drawing.dot_height() as i32;
        let phase = tick as f32 * 0.12;
        for x in 0..width {
            let normalized = x as f32 / width.max(1) as f32;
            let y = (normalized * std::f32::consts::TAU + phase).sin();
            let dot = ((y + 1.0) * 0.5 * (height.saturating_sub(1)) as f32) as i32;
            let _ = drawing.set_run(x, dot.min(height - 1), dot.min(height - 1));
        }
    })
}

fn app(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let tick = cx.use_live_state(|| 0_u64);
    let _clock = cx.use_frame_clock({
        let tick = tick.clone();
        move |_dt| {
            let _ = tick.update(|value| *value = value.wrapping_add(1));
        }
    });
    let frame = tick.read(|value| *value).unwrap_or(0);
    let drawing = plot(frame);
    ui! {
        <split axis={Layout::Horizontal} weights={vec![1, 2]}>
            <pane title="Workspace">
                {text("A live frame clock drives the plot.")}
                {text(format!("frame {frame}"))}
            </pane>
            <pane title="Signal">
                <canvas width={44} height={7} draw={drawing} />
            </pane>
        </split>
        <dialog
            open
            modal={false}
            title="Overlay"
            dismiss_on_outside={false}
        >
            {text("This card is a portal, centered over the split.")}
            {dialog_close_button()}
        </dialog>
    }
}

fn dialog_close_button() -> Node {
    text("Press Escape or close with your application's own state.")
}

fn main() -> Result<(), icmd::RenderError> {
    render(app.apply(()), RuntimeConfig::default())
}
