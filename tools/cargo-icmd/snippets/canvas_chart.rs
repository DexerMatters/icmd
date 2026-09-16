//! A small activity chart drawn directly on a fixed cell canvas.

use std::sync::Arc;

use crossterm::style::Color;
use icmd::{CanvasContext, ComponentContext, Node, Props, canvas, card, muted, ui};

/// Canvas drawing: bars, a baseline, a trend line, and clipped text.
pub fn canvas_chart(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let accent = theme.colors.accent;
    let muted_foreground = theme.colors.muted_foreground;

    ui! {
        <card style={|style| { style.gap /= 0; }}>
            <canvas width={44} height={10} draw={Arc::new(move |ctx: &mut CanvasContext| {
                let baseline = ctx.height() as i32 - 2;
                ctx.set_foreground(muted_foreground);
                let _ = ctx.line(0, baseline, ctx.width() as i32 - 1, baseline, "─");
                let samples = [3_i32, 7, 5, 9, 6, 11, 8];
                let mut previous: Option<(i32, i32)> = None;
                for (index, sample) in samples.iter().enumerate() {
                    let x = index as i32 * 6 + 1;
                    let height = *sample as u16;
                    ctx.set_foreground(if index % 2 == 0 { primary } else { accent });
                    let _ = ctx.fill_rect(x, baseline - i32::from(height), 4, height, "█");
                    let point = (x + 1, baseline - i32::from(height));
                    if let Some(from) = previous {
                        ctx.set_foreground(muted_foreground);
                        let _ = ctx.line(from.0, from.1, point.0, point.1, "·");
                    }
                    previous = Some(point);
                }
                ctx.set_foreground(theme.colors.foreground);
                let _ = ctx.fill_text("requests / minute", 1, 0);
            })} />
            <muted>"The canvas is a fixed cell grid; text is clipped to its bounds."</muted>
        </card>
    }
}
