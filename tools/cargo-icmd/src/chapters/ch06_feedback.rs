//! Chapter 06 — Feedback and Canvas.
//!
//! Status surfaces read their meaning from one state value, and a canvas turns
//! a fixed grid of cells into a chart without a charting dependency.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use crossterm::style::{Attribute, Attributes};
use icmd::theme::Theme;
use icmd::{
    AlertVariant, Align, BadgeVariant, ButtonVariant, CanvasContext, Component, ComponentContext,
    Dimension, Justify, Layout, Node, Props, Span, Text, TextWrap, alert, badge, button, canvas,
    card, muted, progress_bar, row, skeleton, spinner, ui, view,
};

use super::{ChapterProps, document, masthead, route_card, section};
use crate::demos::{PublishAction, PublishPhase, publish_tick, push_bounded};
use crate::docs::{self, ApiRow, CalloutKind};
use crate::metadata::{self, SectionMeta};
use crate::snippets;

/// One label per badge variant, each beside the text that carries its meaning.
const BADGES: [(BadgeVariant, &str, &str); 5] = [
    (
        BadgeVariant::Primary,
        "building",
        "work in progress; the current run is live",
    ),
    (
        BadgeVariant::Secondary,
        "verified",
        "a check passed and nothing needs attention",
    ),
    (
        BadgeVariant::Accent,
        "pinned",
        "a deliberate hold, not a failure",
    ),
    (
        BadgeVariant::Muted,
        "draft",
        "queued work with no urgency attached",
    ),
    (
        BadgeVariant::Destructive,
        "blocked",
        "a failure that needs a human decision",
    ),
];

/// Role name, title, and copy for one alert variant.
const ALERTS: [(AlertVariant, &str, &str, &str); 4] = [
    (
        AlertVariant::Info,
        "info",
        "Heads up",
        "The index rebuilds in the background; the current view stays interactive.",
    ),
    (
        AlertVariant::Success,
        "success",
        "Release published",
        "v0.4.2 is live for x86_64 and aarch64.",
    ),
    (
        AlertVariant::Warning,
        "warning",
        "Stale lockfile",
        "Two crates resolved differently than the lockfile records.",
    ),
    (
        AlertVariant::Error,
        "error",
        "Upload failed",
        "The artifact store refused the write: permission denied.",
    ),
];

/// Timer ticks a simulated publish needs, so twenty steps reach one hundred.
const PUBLISH_TOTAL: usize = 20;
/// Progress added per tick; this many steps reach the maximum.
const PUBLISH_STEP: u64 = 5;
/// Entries the status log keeps; older entries are dropped.
const PUBLISH_LOG_CAP: usize = 8;

/// Every badge variant beside the sentence that carries its meaning.
fn badges_demo(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let rows = BADGES
        .iter()
        .map(|(variant, label, meaning)| {
            ui! {
                <row style={|style| {
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.gap /= 1;
                }}>
                    <view style={|style| { style.width /= Dimension::Cells(14); }}>
                        <badge text={*label} variant={*variant} />
                    </view>
                    <muted>{Text::new(*meaning).wrap(TextWrap::Soft)}</muted>
                </row>
            }
        })
        .collect::<Node>();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <muted>"variant · label · what the label means"</muted>
            {rows}
        </view>
    }
}

/// The accent role one alert variant paints, exactly as the component picks it.
fn alert_accent(theme: &Theme, variant: AlertVariant) -> crossterm::style::Color {
    match variant {
        AlertVariant::Info => theme.colors.primary,
        AlertVariant::Success => theme.colors.secondary,
        AlertVariant::Warning => theme.colors.accent,
        AlertVariant::Error => theme.colors.destructive,
    }
}

/// One legend row: the role swatch, then the title color `on_card` resolves.
fn alert_legend(theme: &Theme, variant: AlertVariant, role: &str) -> Node {
    let accent = alert_accent(theme, variant);
    let title = theme.on_card(accent);
    let role = role.to_string();
    ui! {
        <row style={|style| {
            style.width /= Dimension::Max;
            style.align /= Align::Center;
            style.gap /= 1;
        }}>
            <view style={|style| { style.width /= Dimension::Cells(10); }}>
                <muted>{Text::new(role)}</muted>
            </view>
            {Text::new("████").foreground(accent)}
            <muted>"stripe"</muted>
            {Text::new("Title").foreground(title).bold()}
            <muted>"title"</muted>
        </row>
    }
}

/// The four alert severities with realistic copy, plus the role legend.
fn alerts_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let alerts = ALERTS
        .iter()
        .map(|(variant, _role, title, message)| {
            ui! { <alert variant={*variant} title={*title} message={*message} /> }
        })
        .collect::<Node>();
    let legend = ALERTS
        .iter()
        .map(|(variant, role, _title, _message)| alert_legend(&theme, *variant, role))
        .collect::<Node>();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {alerts}
            <muted>"each legend row: the solid role painted as a stripe, then the title color the theme resolves for that role on a card"</muted>
            {legend}
        </view>
    }
}

/// Several progress values, including one past the maximum that clamps.
fn progress_demo(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <muted>"value / max · width in cells"</muted>
            <progress_bar value={0_u64} max={100_u64} width={26_u16} label={"cold start"} />
            <progress_bar value={25_u64} max={100_u64} width={26_u16} label={"index"} />
            <progress_bar value={60_u64} max={100_u64} width={26_u16} label={"upload"} />
            <progress_bar value={100_u64} max={100_u64} width={26_u16} label={"verify"} />
            <progress_bar value={140_u64} max={100_u64} width={26_u16} label={"over max"} />
            <progress_bar value={3_u64} max={7_u64} width={18_u16} label={"3 of 7"} />
            <progress_bar value={42_u64} max={100_u64} width={26_u16} label={"no percentage"} show_percentage={false} />
            <progress_bar value={50_u64} max={100_u64} width={6_u16} label={"width 6"} />
        </view>
    }
}

/// Ten spinner frames, two labeled spinners, and a skeleton placeholder.
fn spinners_demo(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let frames = (0..10_usize)
        .map(|frame| {
            ui! {
                <view style={|style| { style.width /= Dimension::Cells(2); }}>
                    <spinner frame={frame} />
                </view>
            }
        })
        .collect::<Node>();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <muted>"every frame in the table, in order; the component adds nothing of its own"</muted>
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 0; }}>
                {frames}
            </row>
            <muted>"the same component with a caller-supplied label"</muted>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 2;
            }}>
                <spinner frame={0_usize} label={"indexing"} />
                <spinner frame={4_usize} label={"linking"} />
            </row>
            <muted>"skeleton width={36} while the notes load"</muted>
            <skeleton width={36_u16} />
            <muted>"the same region once the caller decides the content has arrived"</muted>
            {Text::new("release notes ready · 42 entries").bold()}
        </view>
    }
}

/// Start, pause, and reset a simulated publish whose timer an effect owns.
fn publish_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let (phase, set_phase) = cx.use_state(PublishPhase::default);
    let (progress, set_progress) = cx.use_state(|| 0_u64);
    let (frame, set_frame) = cx.use_state(|| 0_usize);
    let (log, set_log) = cx.use_state(Vec::<String>::new);

    let effect_phase = set_phase.clone();
    let effect_progress = set_progress.clone();
    let effect_frame = set_frame.clone();
    let effect_log = set_log.clone();
    // A resume continues from the model the previous run left behind.
    let resume_tick = frame;
    let resume_progress = progress;
    cx.use_effect(phase, move || {
        let stop = Arc::new(AtomicBool::new(false));
        if phase.is_running() {
            let worker_stop = Arc::clone(&stop);
            let set_progress = effect_progress.clone();
            let set_frame = effect_frame.clone();
            let set_phase = effect_phase.clone();
            let set_log = effect_log.clone();
            std::thread::spawn(move || {
                let mut tick = resume_tick;
                let mut progress = resume_progress;
                loop {
                    if worker_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(140));
                    if worker_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    tick += 1;
                    let next = publish_tick(tick, progress, PUBLISH_STEP, PUBLISH_TOTAL);
                    progress = next.progress;
                    set_frame.set(next.frame);
                    set_progress.set(next.progress);
                    if tick % 4 == 0 {
                        let chunk = tick / 4;
                        set_log.update(move |entries| {
                            push_bounded(
                                entries,
                                format!("chunk {chunk} verified"),
                                PUBLISH_LOG_CAP,
                            );
                        });
                    }
                    if next.phase == PublishPhase::Done {
                        set_phase.set(next.phase);
                        set_log.update(|entries| {
                            push_bounded(
                                entries,
                                String::from("release published"),
                                PUBLISH_LOG_CAP,
                            );
                        });
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
    let reset_frame = set_frame;
    let reset_log = set_log;

    let (badge_variant, alert_variant, headline, message) = match phase {
        PublishPhase::Idle => (
            BadgeVariant::Muted,
            AlertVariant::Info,
            "Publish release",
            "Nothing is running. Start begins the simulated upload.",
        ),
        PublishPhase::Running => (
            BadgeVariant::Primary,
            AlertVariant::Info,
            "Publishing release",
            "The timer owns the clock; every surface below reads the same model.",
        ),
        PublishPhase::Paused => (
            BadgeVariant::Accent,
            AlertVariant::Warning,
            "Publish paused",
            "The worker was stopped in cleanup. Start resumes from this progress.",
        ),
        PublishPhase::Done => (
            BadgeVariant::Secondary,
            AlertVariant::Success,
            "Publish complete",
            "Progress reached one hundred and the operation stopped itself.",
        ),
    };
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let entries = log
        .iter()
        .map(|entry| ui! { <muted>{Text::new(entry.clone())}</muted> })
        .collect::<Node>();
    let kept = log.len();
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new(headline).bold()}
                <badge text={phase.label()} variant={badge_variant} />
            </row>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                <spinner frame={frame} label={phase.label()} />
                <progress_bar value={progress} max={100_u64} width={30_u16} label={"upload"} />
            </row>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                <button on_press={move |_| start.update(|value| *value = value.apply(PublishAction::Start))}>"start"</button>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| pause.update(|value| *value = value.apply(PublishAction::Pause))}>"pause"</button>
                <button variant={ButtonVariant::Destructive}
                    on_press={move |_| {
                        reset_phase.update(|value| *value = value.apply(PublishAction::Reset));
                        reset_progress.set(0_u64);
                        reset_frame.set(0_usize);
                        reset_log.update(|entries| entries.clear());
                    }}>"reset"</button>
                {Text::from_spans([
                    Span::new(format!("tick {frame} of {PUBLISH_TOTAL}")).foreground(muted_foreground),
                    Span::new(format!("   ·   {progress}%")).foreground(primary).bold(),
                ])}
            </row>
            <alert variant={alert_variant} title={headline} message={message} />
            {Text::new(format!("status log · {kept} of {PUBLISH_LOG_CAP} entries kept")).foreground(muted_foreground)}
            {if log.is_empty() {
                ui! { <muted>"no checkpoints yet"</muted> }
            } else {
                ui! {
                    <view style={|style| {
                        style.layout /= Layout::Vertical;
                        style.width /= Dimension::Max;
                        style.gap /= 0;
                    }}>{entries}</view>
                }
            }}
        </card>
    }
}

/// A small activity chart drawn on a fixed cell canvas.
fn chart_canvas(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let accent = theme.colors.accent;
    let muted_foreground = theme.colors.muted_foreground;
    let foreground = theme.colors.foreground;
    ui! {
        <canvas width={40} height={9} draw={Arc::new(move |ctx: &mut CanvasContext| {
            ctx.set_foreground(foreground);
            ctx.set_attributes(Attributes::none().with(Attribute::Bold));
            let _ = ctx.fill_text("requests / minute", 1, 0);
            ctx.set_attributes(Attributes::none());
            let baseline = ctx.height() as i32 - 2;
            ctx.set_foreground(muted_foreground);
            let _ = ctx.line(0, baseline, ctx.width() as i32 - 1, baseline, "─");
            let samples = [3_i32, 6, 4, 8, 5, 7, 2];
            let mut previous: Option<(i32, i32)> = None;
            for (index, sample) in samples.iter().enumerate() {
                let column = index as i32 * 5 + 1;
                let height = *sample as u16;
                ctx.set_foreground(if index % 2 == 0 { primary } else { accent });
                let _ = ctx.fill_rect(column, baseline - i32::from(height), 3, height, "█");
                let point = (column + 1, baseline - i32::from(height));
                if let Some(from) = previous {
                    ctx.set_foreground(muted_foreground);
                    let _ = ctx.line(from.0, from.1, point.0, point.1, "·");
                }
                previous = Some(point);
            }
        })} />
    }
}

/// A canvas that reports what the cell layer clips and what it refuses.
fn canvas_report(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let foreground = theme.colors.foreground;
    let accent = theme.colors.accent;
    let muted_foreground = theme.colors.muted_foreground;
    ui! {
        <canvas width={40} height={7} draw={Arc::new(move |ctx: &mut CanvasContext| {
            ctx.set_foreground(muted_foreground);
            let _ = ctx.stroke_rect(0, 0, 40, 7);
            ctx.set_foreground(foreground);
            ctx.set_attributes(Attributes::none().with(Attribute::Bold));
            let _ = ctx.fill_text("clipping and rejection are values", 2, 1);
            ctx.set_attributes(Attributes::none());
            // A rectangle that starts left of the canvas and runs past its right
            // edge: clamped to the grid, and reported as success.
            let clamped = ctx.fill_rect(-6, 3, 52, 1, "░").is_ok();
            // A cell outside the canvas: ignored, and also reported as success.
            let outside = ctx.set(-6, 40, "#").is_ok();
            // A symbol the cell layer cannot accept: an error value.
            let rejected = ctx.set(2, 4, "ab");
            let outcome = match rejected {
                Ok(()) => String::from("two-grapheme cell accepted"),
                Err(error) => format!("two-grapheme cell → {error}"),
            };
            ctx.set_foreground(accent);
            let _ = ctx.fill_text(&outcome, 2, 4);
            ctx.set_foreground(muted_foreground);
            let _ = ctx.fill_text(
                &format!("clamped fill → {clamped} · off-canvas set → {outside} · this line is cut at the edge"),
                2,
                5,
            );
        })} />
    }
}

/// Chapter 06.
pub(super) fn feedback_and_canvas(cx: &mut ComponentContext, props: &Props<ChapterProps>) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(5);
    let sections: &[SectionMeta] = meta.sections;

    let badges_section = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body("A badge is a short label with a semantic role: a solid background, one line, and no interaction. Because the background is the only thing that changes, the label has to carry the meaning by itself; the color only reinforces a word the reader can already parse.")}
            {docs::body("Reach for a badge when the state fits in a word or two. A sentence belongs in an alert, and anything a reader must act on belongs in a control.")}
            {docs::specimen(&theme, "every badge variant", ui! { {badges_demo.apply(())} })}
            {docs::notice(&theme, "BadgeVariant selects a theme role: Primary, Secondary, Accent, Muted, and Destructive. The component pads its text with one space on each side and adds no gap or icon of its own, so the padded label is the whole surface.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "badge", purpose: "compact, non-interactive status label", defaults: "text empty, BadgeVariant::Primary", events: "none" },
                ApiRow { name: "BadgeProps", purpose: "the label and its semantic role", defaults: "text empty, variant Primary", events: "none" },
                ApiRow { name: "BadgeVariant", purpose: "selects the background and foreground pair", defaults: "Primary", events: "none" },
                ApiRow { name: "Text", purpose: "the padded, styled line a badge draws", defaults: "soft wrap, theme foreground", events: "none" },
            ])}
        },
    );

    let alerts_section = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body("An alert is a card with a colored left stripe, a bold title, and a softly wrapped message. Its variant selects one accent role, and that role paints the stripe and colors the title.")}
            {docs::body("The stripe is decoration. The title is the readable cue, and the component resolves its color through Theme::on_card, which returns the role color only while it keeps enough contrast on a card and otherwise falls back to the card foreground. A reader who cannot see color still reads the title.")}
            {docs::specimen(&theme, "four severities", ui! { {alerts_demo.apply(())} })}
            {docs::watch_for(&theme, "Do not use a stack of alerts as a status channel. One alert reports one event; a stream of events belongs in a log, and severity that changes over time belongs in a badge.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "alert", purpose: "card with a role stripe, title, and message", defaults: "AlertVariant::Info, empty title and message", events: "none" },
                ApiRow { name: "AlertProps", purpose: "title, message, and severity role", defaults: "Info", events: "none" },
                ApiRow { name: "AlertVariant", purpose: "Info, Success, Warning, Error", defaults: "Info", events: "none" },
                ApiRow { name: "Theme::on_card", purpose: "keeps a role readable as text on a card", defaults: "falls back to card_foreground", events: "none" },
            ])}
        },
    );

    let progress_section = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body("A progress bar renders a value; it does not own one. You pass value and max, and the component floors max at one, clamps value into 0..=max, rounds the filled cells to the nearest half cell, and prints the floored percentage right-aligned in three columns.")}
            {docs::body("The width is a cell count, so the bar never depends on measured layout, and the optional label is drawn before the bar with the theme's label style. Setting show_percentage to false leaves the number to your own readout.")}
            {docs::specimen(&theme, "progress values and widths", ui! { {progress_demo.apply(())} })}
            {docs::notice(&theme, "Because the caller supplies the value, an indeterminate operation has no honest bar to show. Use a spinner while the total is unknown and switch to a bar once it is known.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "progress_bar", purpose: "one-line bar filled in proportion to value / max", defaults: "value 0, max 100, width 20, show_percentage true", events: "none" },
                ApiRow { name: "ProgressBarProps", purpose: "value, max, width, label, show_percentage", defaults: "max floored at 1, value clamped to max", events: "none" },
            ])}
        },
    );

    let spinners_section = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body("A spinner is one frame, not an animation. The component owns a ten-frame table and wraps whatever frame index you give it, so the caller decides the pace and when the motion stops.")}
            {docs::body("A skeleton is the same idea for content: a block of placeholder glyphs of a caller-chosen width. Nothing measures your data, and nothing guesses when the placeholder is no longer needed.")}
            {docs::specimen(&theme, "frames, labels, and a placeholder", ui! { {spinners_demo.apply(())} })}
            {docs::notice(&theme, "Advancing a frame is an ordinary state update, so a spinner is driven by the same effect that reports real progress. When that effect stops, the frame stops with it.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "spinner", purpose: "one frame of a ten-frame indicator plus a label", defaults: "frame 0, empty label", events: "none" },
                ApiRow { name: "SpinnerProps", purpose: "the frame index and optional trailing text", defaults: "frame wraps modulo ten", events: "none" },
                ApiRow { name: "skeleton", purpose: "muted block of placeholder cells", defaults: "width 12", events: "none" },
                ApiRow { name: "SkeletonProps", purpose: "the placeholder width in cells", defaults: "width 12", events: "none" },
            ])}
        },
    );

    let publish_section = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body("A publish operation is one state machine and several views of it. The phase decides the badge, the alert, and whether a timer may run; progress and the spinner frame come from tick arithmetic; a bounded log records what happened without growing forever.")}
            {docs::body("The demonstration drives the shared reducer from the demos module: publish_tick clamps progress at one hundred and completes exactly once, and push_bounded keeps only the newest entries. Because the rules live outside the component, the behavior is covered by ordinary unit tests.")}
            {docs::live_example(
                &theme,
                "publish operation",
                "Start runs the timer, pause holds the progress, and reset returns the whole model to its initial state.",
                ui! { {publish_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "timer", "the effect spawns one worker while the phase is running and stops it in cleanup when the phase pauses or the chapter unmounts")} }),
            )}
            {docs::source_block(&theme, "the same operation as a component", snippets::PUBLISH_OPERATION)}
            {docs::callout(&theme, CalloutKind::Production, "The displayed snippet keeps its own phase enum so it reads as a complete program; the live demonstration imports the tested reducer instead, so the chapter cannot drift from the behavior the test suite covers.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "use_state", purpose: "phase, progress, frame, and log", defaults: "one initial value per hook", events: "set, update" },
                ApiRow { name: "use_effect", purpose: "own the timer and return its cleanup", defaults: "runs when the phase changes", events: "cleanup on change and unmount" },
                ApiRow { name: "PublishPhase", purpose: "Idle, Running, Paused, Done", defaults: "Idle; Done is sticky", events: "apply(PublishAction)" },
                ApiRow { name: "publish_tick", purpose: "clamp progress and finish exactly once", defaults: "progress capped at 100", events: "none" },
                ApiRow { name: "push_bounded", purpose: "append to a log and drop the oldest entries", defaults: "cap floored at 1", events: "none" },
            ])}
        },
    );

    let canvas_section = section(
        &theme,
        data,
        5,
        &sections[5],
        ui! {
            {docs::body("A canvas is a fixed grid of cells plus a drawing callback. CanvasContext reports its width and height, carries the current foreground, background, and text attributes, and offers set, fill_rect, stroke_rect, line, fill_text, clear, and draw_image.")}
            {docs::body("Coordinates are cell coordinates, and the box is exactly the size you asked for. Nothing is measured for you: a line or a run of text that leaves an edge is clipped by the cell layer, which is both correct and cheap.")}
            {docs::specimen(&theme, "activity chart and cell-layer report", ui! {
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 1;
                }}>
                    {chart_canvas.apply(())}
                    {canvas_report.apply(())}
                </view>
            })}
            {docs::source_block(&theme, "the activity chart as source", snippets::CANVAS_CHART)}
            {docs::watch_for(&theme, "Drawing returns a result rather than panicking. A symbol the cell layer cannot accept comes back as CanvasError::Cell, and a surface that would exceed 1,048,576 cells fails to construct, after which the canvas widget paints a short message instead of a chart. Ordinary mistakes - a negative coordinate, a rectangle wider than the canvas, text that runs off the edge - are clipped and are not errors, so decide explicitly whether you want to surface them.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "canvas", purpose: "fixed width by height cell grid", defaults: "width 1, height 1, no-op draw", events: "none" },
                ApiRow { name: "CanvasProps", purpose: "width, height, and the drawing callback", defaults: "1 x 1 with an empty callback", events: "none" },
                ApiRow { name: "CanvasContext", purpose: "colors, attributes, and the drawing primitives", defaults: "blank cells in the theme foreground", events: "none" },
                ApiRow { name: "CanvasDraw", purpose: "Arc<dyn Fn(&mut CanvasContext)> stored on the node", defaults: "no-op", events: "none" },
                ApiRow { name: "CanvasError", purpose: "Cell, Image, and Raster failures as values", defaults: "—", events: "none" },
            ])}
        },
    );

    let background = theme.colors.background;
    document(
        data,
        ui! {
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.background /= background;
            }}>
                {masthead(&theme, meta, data.section_count())}
                {badges_section}
                {alerts_section}
                {progress_section}
                {spinners_section}
                {publish_section}
                {canvas_section}
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 1;
                }}>
                    {route_card(&theme, data, 6, "▦", "07 · Scroll, Selection, and Raster Media", "Scroll ownership, selection inside a scroller, and terminal images.")}
                </view>
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}
