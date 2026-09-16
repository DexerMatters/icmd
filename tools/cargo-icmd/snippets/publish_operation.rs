//! A publish operation driven by one state model and a cleaned-up timer.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use icmd::{
    AlertVariant, BadgeVariant, ButtonVariant, ComponentContext, Dimension, Node, Props, Text,
    alert, badge, button, card, muted, progress_bar, row, spinner, ui,
};

/// Publish phases; the UI derives every surface from this one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nothing has started.
    Idle,
    /// The operation is advancing.
    Running,
    /// The operation is held.
    Paused,
    /// The operation completed.
    Done,
}

/// Start, pause, and reset a simulated publish whose timer an effect owns.
pub fn publish_operation(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (phase, set_phase) = cx.use_state(|| Phase::Idle);
    let (progress, set_progress) = cx.use_state(|| 0_u64);
    let (frame, set_frame) = cx.use_state(|| 0_usize);
    let (log, set_log) = cx.use_state(Vec::<String>::new);

    let effect_phase = set_phase.clone();
    let effect_progress = set_progress.clone();
    let effect_frame = set_frame.clone();
    let effect_log = set_log.clone();
    cx.use_effect(phase, move || {
        let stop = Arc::new(AtomicBool::new(false));
        if phase == Phase::Running {
            let worker_stop = Arc::clone(&stop);
            let set_progress = effect_progress.clone();
            let set_frame = effect_frame.clone();
            let set_phase = effect_phase.clone();
            let set_log = effect_log.clone();
            std::thread::spawn(move || {
                let mut tick = 0_usize;
                while !worker_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(120));
                    tick += 1;
                    set_frame.set(tick);
                    set_progress.update(|value| *value = (*value + 5).min(100));
                    if tick % 4 == 0 {
                        let chunk = tick / 4;
                        set_log.update(move |entries| {
                            entries.push(format!("uploaded chunk {chunk}"));
                            if entries.len() > 6 {
                                entries.remove(0);
                            }
                        });
                    }
                    if tick >= 20 {
                        set_phase.set(Phase::Done);
                        set_log.update(|entries| entries.push(String::from("release published")));
                        break;
                    }
                }
            });
        }
        let cleanup_stop = Arc::clone(&stop);
        move || cleanup_stop.store(true, Ordering::Relaxed)
    });

    let start = set_phase.clone();
    let pause = set_phase.clone();
    let reset_phase = set_phase;
    let reset_progress = set_progress;
    let reset_log = set_log;

    let status = match phase {
        Phase::Idle => "idle",
        Phase::Running => "publishing",
        Phase::Paused => "paused",
        Phase::Done => "published",
    };
    let variant = match phase {
        Phase::Done => BadgeVariant::Secondary,
        Phase::Paused => BadgeVariant::Accent,
        Phase::Idle => BadgeVariant::Muted,
        Phase::Running => BadgeVariant::Primary,
    };
    let entries = log
        .iter()
        .map(|entry| ui! { <muted>{Text::new(entry.clone())}</muted> })
        .collect::<Node>();

    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= icmd::Align::Center;
                style.justify /= icmd::Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new("Publish release").bold()}
                <badge text={status} variant={variant} />
            </row>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.gap /= 1;
                style.align /= icmd::Align::Center;
            }}>
                <spinner frame={frame} label={"working"} />
                <progress_bar value={progress} max={100} width={28} label={"upload"} />
            </row>
            <row style={|style| { style.gap /= 1; }}>
                <button on_press={move |_| start.set(Phase::Running)}>"start"</button>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| pause.set(Phase::Paused)}>"pause"</button>
                <button variant={ButtonVariant::Destructive}
                    on_press={move |_| {
                        reset_phase.set(Phase::Idle);
                        reset_progress.set(0);
                        reset_log.update(|entries| entries.clear());
                    }}>"reset"</button>
            </row>
            {if phase == Phase::Done {
                ui! { <alert variant={AlertVariant::Success} title={"Done"} message={"The release is live."} /> }
            } else {
                ui! { <alert variant={AlertVariant::Info} title={"Status log"} message={"Recent chunks appear here."} /> }
            }}
            {entries}
        </card>
    }
}
