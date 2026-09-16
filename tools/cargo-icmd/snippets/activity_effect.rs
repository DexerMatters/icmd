//! A start/stop activity whose worker is always cleaned up on unmount.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use icmd::{ButtonVariant, ComponentContext, Node, Props, Text, button, card, row, ui};

/// A worker thread owned by an effect, cancelled by its cleanup closure.
pub fn activity(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (running, set_running) = cx.use_state(|| false);
    let (ticks, set_ticks) = cx.use_state(|| 0_u32);

    cx.use_effect(running, move || {
        let stop = Arc::new(AtomicBool::new(false));
        if running {
            let worker_stop = Arc::clone(&stop);
            let set_ticks = set_ticks.clone();
            std::thread::spawn(move || {
                while !worker_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(200));
                    set_ticks.update(|value| *value += 1);
                }
            });
        }
        let cleanup_stop = Arc::clone(&stop);
        move || cleanup_stop.store(true, Ordering::Relaxed)
    });

    let toggle = set_running.clone();
    let label = if running { "stop" } else { "start" };
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            {Text::new(format!("worker {label} · {ticks} ticks")).bold()}
            <row style={|style| { style.gap /= 1; }}>
                <button variant={if running { ButtonVariant::Destructive } else { ButtonVariant::Primary }}
                    on_press={move |_| toggle.update(|value| *value = !*value)}>
                    {label}
                </button>
            </row>
        </card>
    }
}
