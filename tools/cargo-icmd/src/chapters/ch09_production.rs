//! Chapter 09 — Production.
//!
//! Turns a convincing UI into a reliable terminal application: configuration,
//! lifecycle, graceful exit, effect hygiene, a quiet render path, resource
//! budgets, layered tests, and a release checklist.

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use icmd::{
    AlertVariant, Align, BadgeVariant, BorderKind, ButtonVariant, Component, ComponentContext,
    Dimension, Edges, Justify, Layout, Node, Props, Span, Text, TextWrap, alert, badge, button,
    card, checkbox, empty, muted, progress_bar, row, ui, view,
};

use super::{ChapterProps, document, masthead, section};
use crate::demos::{Checklist, LIFECYCLE_PHASES, push_bounded};
use crate::docs::{self, ApiRow, CalloutKind};
use crate::metadata::{self, SectionMeta};
use crate::snippets;

/// One configuration knob: its name, its default, and what it decides.
struct Field {
    /// Field name as written in the struct.
    name: &'static str,
    /// Default value.
    default: &'static str,
    /// What the field decides.
    meaning: &'static str,
}

/// Renders a configuration table as compact reference rows.
fn field_list(theme: &icmd::theme::Theme, fields: &[Field]) -> Node {
    fields
        .iter()
        .map(|field| docs::ref_row(theme, field.name, field.meaning, field.default))
        .collect::<Node>()
}

/// Every `RuntimeConfig` field with its default.
const RUNTIME_FIELDS: [Field; 14] = [
    Field {
        name: "exit_key",
        default: "Ctrl+C",
        meaning: "The one key the runtime answers itself; it is the key a user can always reach.",
    },
    Field {
        name: "poll_interval",
        default: "50 ms",
        meaning: "How long the loop may block between polls. Lower is more responsive and busier.",
    },
    Field {
        name: "alternate_screen",
        default: "true",
        meaning: "Enter the alternate screen and restore the previous one on exit.",
    },
    Field {
        name: "mouse_capture",
        default: "true",
        meaning: "Report pointer events and release mouse reporting on exit.",
    },
    Field {
        name: "bracketed_paste",
        default: "true",
        meaning: "Receive pastes as one event instead of a burst of keystrokes.",
    },
    Field {
        name: "focus_change",
        default: "true",
        meaning: "Receive terminal focus gained and lost events.",
    },
    Field {
        name: "enhanced_keyboard",
        default: "true",
        meaning: "Ask a capable terminal for unambiguous modified keys, without which Ctrl+Shift+C collapses into Ctrl+C.",
    },
    Field {
        name: "system_clipboard",
        default: "true",
        meaning: "Mirror copies into the terminal's own clipboard with an OSC 52 write.",
    },
    Field {
        name: "image_protocol",
        default: "Auto",
        meaning: "Prefer a detected native graphics protocol and fall back to symbols.",
    },
    Field {
        name: "image_cache_bytes",
        default: "64 MiB",
        meaning: "Decoded image bytes retained for reuse across frames.",
    },
    Field {
        name: "image_update_policy",
        default: "Adaptive",
        meaning: "Whether a surface may repaint with a native protocol or only when one is available.",
    },
    Field {
        name: "emoji_merging",
        default: "default",
        meaning: "How emoji sequences are merged during layout and rendering.",
    },
    Field {
        name: "limits",
        default: "ResourceLimits::default()",
        meaning: "The hard ceilings validated before any thread or terminal mode exists.",
    },
    Field {
        name: "events_per_tick",
        default: "64",
        meaning: "The fairness budget that keeps a continuous input stream from starving presentation.",
    },
];

/// Every `ResourceLimits` field with its default.
const LIMIT_FIELDS: [Field; 12] = [
    Field {
        name: "max_input_bytes",
        default: "64 MiB",
        meaning: "Largest editable input text accepted.",
    },
    Field {
        name: "max_nodes",
        default: "1,000,000",
        meaning: "Largest logical tree, counted after lowering.",
    },
    Field {
        name: "max_tree_depth",
        default: "1,024",
        meaning: "Deepest tree, bounding recursive traversal.",
    },
    Field {
        name: "max_encoded_image_bytes",
        default: "32 MiB",
        meaning: "Largest encoded image payload accepted.",
    },
    Field {
        name: "max_source_width",
        default: "16,384 px",
        meaning: "Widest accepted source image.",
    },
    Field {
        name: "max_source_height",
        default: "16,384 px",
        meaning: "Tallest accepted source image.",
    },
    Field {
        name: "max_source_pixels",
        default: "64 Mpx",
        meaning: "Largest accepted source area.",
    },
    Field {
        name: "max_decoded_image_bytes",
        default: "256 MiB",
        meaning: "Most decoded image data held in memory.",
    },
    Field {
        name: "max_in_flight_image_bytes",
        default: "256 MiB",
        meaning: "Most decoded bytes in flight at once.",
    },
    Field {
        name: "max_transform_pixels",
        default: "64 Mpx",
        meaning: "Largest output of a single image transform.",
    },
    Field {
        name: "max_cache_bytes",
        default: "64 MiB",
        meaning: "Most bytes retained in the renderer image cache.",
    },
    Field {
        name: "max_output_bytes_per_frame",
        default: "64 MiB",
        meaning: "Most encoded output emitted in one frame.",
    },
];

/// What each lifecycle phase is for.
const PHASE_ROLE: [(&str, &str); 5] = [
    ("Boot", "terminal acquired, configuration validated"),
    ("Mount", "the first tree is committed"),
    (
        "Ready",
        "the loop is running and frames are being presented",
    ),
    ("Unmount", "the tree is gone; effects have cleaned up"),
    ("Exit", "terminal modes are restored and the session ends"),
];

/// The lifecycle phases as a labeled timeline.
fn lifecycle_timeline(theme: &icmd::theme::Theme) -> Node {
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let border = theme.colors.border;
    let stages = LIFECYCLE_PHASES.iter().enumerate().map(|(index, phase)| {
        let role = PHASE_ROLE[index].1;
        let order = if index < 3 {
            "startup order"
        } else {
            "reverse order"
        };
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
            }}>
                <row style={move |style| {
                    style.layout /= Layout::Horizontal;
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.gap /= 1;
                    style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
                    style.background /= card;
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                    style.border.background /= card;
                }}>
                    {Text::new(format!("{}.", index + 1)).foreground(muted_foreground)}
                    {Text::new(*phase).foreground(primary).bold()}
                    <muted>{Text::new(role).wrap(TextWrap::Soft)}</muted>
                    {Text::new(order.to_string()).foreground(muted_foreground)}
                </row>
                {if index + 1 < LIFECYCLE_PHASES.len() {
                    ui! { <muted>{Text::new("↓").foreground(card_foreground)}</muted> }
                } else {
                    empty()
                }}
            </view>
        }
    });
    stages.collect::<Node>()
}

/// An effect-owned worker cancelled by its cleanup closure.
fn effect_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (running, set_running) = cx.use_state(|| false);
    let (ticks, set_ticks) = cx.use_state(|| 0_u32);
    let (events, set_events) = cx.use_state(Vec::<String>::new);

    cx.use_effect(running, move || {
        let (cancel, worker) = channel::<()>();
        if running {
            let set_ticks = set_ticks.clone();
            let set_events = set_events.clone();
            std::thread::spawn(move || {
                loop {
                    match worker.recv_timeout(Duration::from_millis(150)) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {
                            set_ticks.update(|value| *value += 1);
                            set_events.update(|log| {
                                push_bounded(log, String::from("worker tick"), 5);
                            });
                        }
                    }
                }
            });
        }
        move || {
            let _ = cancel.send(());
        }
    });

    let toggle = set_running.clone();
    let label = if running { "stop" } else { "start" };
    let log = events
        .iter()
        .map(|entry| ui! { <muted>{Text::new(entry.clone())}</muted> })
        .collect::<Node>();
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            {Text::new(format!("worker {label} · {ticks} ticks")).bold()}
            <row style={|style| { style.gap /= 1; }}>
                <button variant={if running { ButtonVariant::Destructive } else { ButtonVariant::Primary }}
                    on_press={move |_| toggle.update(|value| *value = !*value)}>
                    {label}
                </button>
            </row>
            {log}
            <muted>"The cleanup closure sends the cancellation token; leaving the chapter unmounts the effect and stops the thread."</muted>
        </card>
    }
}

/// A would-be graceful exit that records the request instead of performing it.
fn exit_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (requests, set_requests) = cx.use_state(|| 0_u32);
    let (reasons, set_reasons) = cx.use_state(Vec::<String>::new);
    let request = set_requests.clone();
    let log = set_reasons.clone();
    let clear = set_reasons.clone();
    let reset = set_requests.clone();
    let entries = reasons
        .iter()
        .map(|entry| ui! { <muted>{Text::new(entry.clone())}</muted> })
        .collect::<Node>();
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            {Text::new(format!("recorded requests: {requests}")).bold()}
            <row style={|style| { style.gap /= 1; }}>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| {
                        request.update(|value| *value += 1);
                        log.update(|entries| {
                            push_bounded(entries, String::from("ExitReason::Requested"), 4);
                        });
                    }}>"simulate request_exit"</button>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| clear.update(|entries| entries.clear())}>"clear log"</button>
                <button variant={ButtonVariant::Destructive}
                    on_press={move |_| reset.set(0)}>"reset"</button>
            </row>
            {entries}
            <muted>"Nothing here calls request_exit: that would close this guide. The real exit is Ctrl+C, handled by the runtime."</muted>
        </card>
    }
}

/// Counts renders and memo recomputations to make the quiet-path rules concrete.
fn render_path_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (value, set_value) = cx.use_state(|| 0_u32);
    let (events, set_events) = cx.use_state(Vec::<String>::new);
    let renders = cx.use_ref(|| 0_u32);
    let recomputes = cx.use_ref(|| 0_u32);

    {
        let mut counter = renders.lock().expect("render counter");
        *counter += 1;
    }
    let doubled = cx.use_memo(value, || {
        let mut counter = recomputes.lock().expect("memo counter");
        *counter += 1;
        value * 2
    });
    let render_count = *renders.lock().expect("render counter");
    let recompute_count = *recomputes.lock().expect("memo counter");

    let bump = set_value.clone();
    let same = set_value.clone();
    let log_changed = set_events.clone();
    let log_redundant = set_events.clone();
    let entries = events
        .iter()
        .map(|entry| ui! { <muted>{Text::new(entry.clone())}</muted> })
        .collect::<Node>();
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            {Text::from_spans([
                Span::new("value ").bold(),
                Span::new(value.to_string()).bold(),
                Span::new("  · doubled "),
                Span::new(doubled.to_string()).bold(),
            ])}
            {Text::from_spans([
                Span::new("renders ").bold(),
                Span::new(render_count.to_string()),
                Span::new("  · memo recomputes "),
                Span::new(recompute_count.to_string()).bold(),
            ])}
            <row style={|style| { style.gap /= 1; }}>
                <button on_press={move |_| {
                    bump.update(|value| *value += 1);
                    log_changed.update(|entries| {
                        push_bounded(entries, String::from("state changed"), 4)
                    });
                }}>"change the value"</button>
                <button variant={ButtonVariant::Secondary} on_press={move |_| {
                    same.set(value);
                    log_redundant.update(|entries| {
                        push_bounded(entries, String::from("redundant update"), 4)
                    });
                }}>"set the same value"</button>
            </row>
            {entries}
            <muted>"Setting the same value still renders, but the memo dependency is unchanged, so no work is redone."</muted>
        </card>
    }
}

/// The release checklist items.
const CHECKLIST_ITEMS: [&str; 8] = [
    "Commit at 140×45, 120×40, 80×24, and 60×20",
    "Drive every control with the keyboard only",
    "Copy with Ctrl+Shift+C into another program",
    "Check CJK, combining marks, and emoji alignment",
    "Read the light and dark theme for contrast",
    "Verify native images, then force the Symbols fallback",
    "Cancel a worker mid-flight and confirm restoration",
    "Inspect the packaged crate for assets and snippets",
];

/// An interactive, session-local release checklist.
fn checklist_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (checklist, set_checklist) = cx.use_state(|| Checklist::new(CHECKLIST_ITEMS.len()));
    let items = CHECKLIST_ITEMS
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let setter = set_checklist.clone();
            let done = checklist.is_done(index);
            ui! {
                <checkbox
                    key={index as u64}
                    checked={done}
                    label={*item}
                    on_change={move |next: bool| {
                        setter.update(move |checklist| {
                            if checklist.is_done(index) != next {
                                checklist.toggle(index);
                            }
                        });
                    }} />
            }
        })
        .collect::<Node>();
    let reset = set_checklist.clone();
    let completed = checklist.completed();
    let total = checklist.total();
    let percent = checklist.percent();
    let variant = if completed == total {
        BadgeVariant::Secondary
    } else {
        BadgeVariant::Muted
    };
    let status = if completed == total {
        "ready"
    } else {
        "in progress"
    };
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::from_spans([
                    Span::new("Checklist ").bold(),
                    Span::new(format!("{completed}/{total}")),
                ])}
                <badge text={status} variant={variant} />
            </row>
            <progress_bar value={percent} max={100} width={32} label={"complete"} />
            {items}
            <row style={|style| { style.gap /= 1; }}>
                <button variant={ButtonVariant::Destructive}
                    on_press={move |_| reset.update(|checklist| checklist.reset())}>"reset"</button>
                <muted>"Session-local: the list resets when the chapter unmounts."</muted>
            </row>
            {if completed == total {
                ui! { <alert variant={AlertVariant::Success} title={"Ready to ship"} message={"Every terminal-facing check has been exercised."} /> }
            } else {
                ui! { <alert variant={AlertVariant::Info} title={"Still to check"} message={"Walk the remaining rows in a real terminal, not a fixed viewport."} /> }
            }}
        </card>
    }
}

/// Chapter 09.
pub(super) fn production(cx: &mut ComponentContext, props: &Props<ChapterProps>) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(8);
    let sections: &[SectionMeta] = meta.sections;

    let runtime = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body("`RuntimeConfig` is the whole contract between an application and its terminal. Every field below has a default that is safe for an ordinary full-screen application, and every one of them is a decision you can make differently.")}
            {docs::specimen(&theme, "RuntimeConfig, field by field", ui! { {field_list(&theme, &RUNTIME_FIELDS)} })}
            {docs::production_note(&theme, "`enhanced_keyboard` and `system_clipboard` are the two fields that decide whether a clipboard copy is usable. Without unambiguous modified keys a terminal collapses Ctrl+Shift+C into Ctrl+C, and without the OSC 52 mirror a terminal paste reads a clipboard nothing filled.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "RuntimeConfig", purpose: "session behavior and terminal modes", defaults: "safe full-screen defaults", events: "exit key, pointer, paste, resize" },
                ApiRow { name: "RuntimeConfig::new", purpose: "start from a different exit key", defaults: "copy of Default", events: "none" },
                ApiRow { name: "render", purpose: "enter the terminal and drive the loop", defaults: "RuntimeConfig::default()", events: "all terminal input" },
            ])}
        },
    );

    let lifecycle = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body("A session has five phases. Three run before and during the event loop in registration order; the last two run in reverse, which is what lets a startup hook and a teardown hook pair up safely.")}
            {docs::specimen(&theme, "the five phases", ui! { {lifecycle_timeline(&theme)} })}
            {docs::source_block(&theme, "a hook in every phase", snippets::LIFECYCLE_TIMELINE)}
            {docs::notice(&theme, "Every hook receives an `AppSession` with the phase, the measured viewport, the number of frames presented, and — during teardown — the `ExitReason`. Hooks take no borrows, so they are ordinary `FnOnce(&AppSession)` values.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "AppLifecycle", purpose: "register hooks per phase", defaults: "empty; render runs without one", events: "none" },
                ApiRow { name: "AppPhase", purpose: "which phase a hook is running in", defaults: "Boot", events: "none" },
                ApiRow { name: "AppSession", purpose: "read-only facts handed to a hook", defaults: "fresh per hook", events: "none" },
                ApiRow { name: "render_with", purpose: "render with a lifecycle attached", defaults: "requires an explicit AppLifecycle", events: "all terminal input" },
            ])}
        },
    );

    let exit = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body("Exiting is part of the interface. `AppHandle::request_exit` stops the session the same way the configured exit key does: the Unmount and Exit phases run, cleanup closures fire, and the terminal is restored. Calling `std::process::exit` does none of that.")}
            {docs::live_example(
                &theme,
                "a recorded exit request",
                "This demonstration records what a request would look like without actually closing the guide.",
                ui! { {exit_demo.apply(())} },
                None,
            )}
            {docs::specimen(&theme, "why a session ended", ui! {
                {field_list(&theme, &[
                    Field { name: "ExitReason::ExitKey", default: "clean", meaning: "The configured exit key was pressed." },
                    Field { name: "ExitReason::Requested", default: "clean", meaning: "An application handle asked for a graceful exit." },
                    Field { name: "ExitReason::RuntimeClosed", default: "clean", meaning: "The runtime's channels closed before a key was seen." },
                    Field { name: "ExitReason::Failed", default: "typed", meaning: "A stage, callback, frame, or I/O error ended the session." },
                    Field { name: "ExitReason::Aborted", default: "early", meaning: "Setup failed before or during the first commit." },
                ])}
            })}
            {docs::watch_for(&theme, "Restoration is not optional. A panic-free exit path must still leave the alternate screen, mouse reporting, bracketed paste, and focus reporting off; the runtime does that for you, but only if it is allowed to run.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "use_handle", purpose: "cloneable session control", defaults: "the session's own handle", events: "none" },
                ApiRow { name: "AppHandle::request_exit", purpose: "stop cleanly from anywhere", defaults: "equivalent to the exit key", events: "none" },
                ApiRow { name: "ExitReason", purpose: "why the session ended", defaults: "None while live", events: "none" },
                ApiRow { name: "use_unmount", purpose: "run cleanup when a component leaves", defaults: "once, at unmount", events: "none" },
            ])}
        },
    );

    let effects = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body("An effect is the only place work that outlives a render belongs. It runs after the commit whenever its dependencies change, and whatever it returns is its cleanup: a closure that cancels a thread, drops a channel, or releases a resource.")}
            {docs::live_example(
                &theme,
                "a cancelled worker",
                "Start the worker, then stop it — or leave the chapter and watch the effect clean up.",
                ui! { {effect_demo.apply(())} },
                None,
            )}
            {docs::body("Four rules keep effects honest. Give the effect the dependencies it actually reads, so it reruns when they change. Return a cleanup whenever the effect acquired something. Never mutate shared state from inside a render. And keep hook order stable: hooks are matched by position, so a conditional hook is a bug, not a shortcut.")}
            {docs::watch_for(&theme, "A thread with no cancellation path keeps running after its component is gone, and a channel that outlives its reader silently grows. Cleanup is what makes an effect a lease rather than a leak.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "use_effect", purpose: "run after commit when dependencies change", defaults: "dependencies compared with PartialEq", events: "none" },
                ApiRow { name: "use_mount_effect", purpose: "run once, at mount", defaults: "the same as use_effect((), ..)", events: "none" },
                ApiRow { name: "use_unmount", purpose: "run once, at unmount", defaults: "no body at mount", events: "none" },
                ApiRow { name: "EffectResult", purpose: "no cleanup, or a cleanup closure", defaults: "() means nothing to undo", events: "none" },
            ])}
        },
    );

    let render_path = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body("Frames are cheap, but not free. The commit stage diffs the tree and paints only changed cells, so the real cost is decided by how much of your tree changes and how often it changes.")}
            {docs::live_example(
                &theme,
                "renders versus recomputes",
                "Watch the two counters diverge: a redundant update renders, but the memo does not redo its work.",
                ui! { {render_path_demo.apply(())} },
                None,
            )}
            {docs::body("Five habits carry most of the win. Keep the tree structure stable so nodes are reconciled rather than rebuilt. Key every collection so identity follows the item. Memoize derived values with honest dependencies. Bound every log so a long session cannot grow its own page height. And do not queue a state update whose value is unchanged.")}
            {docs::notice(&theme, "This guide follows its own rules: the search overlay is the only continuously interactive surface, every log in these chapters is bounded, and each demonstration owns its state so leaving the chapter resets it.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "use_memo", purpose: "cache derived work across renders", defaults: "recomputes when dependencies differ", events: "none" },
                ApiRow { name: "Key", purpose: "stable identity for a collection item", defaults: "position when no key is given", events: "none" },
                ApiRow { name: "StateSetter::set", purpose: "replace state and wake the runtime", defaults: "wakes even when unchanged", events: "none" },
            ])}
        },
    );

    let limits = section(
        &theme,
        data,
        5,
        &sections[5],
        ui! {
            {docs::body("The runtime refuses work that would exceed its budgets rather than discovering the problem as an allocation failure. Limits are validated before any thread or terminal mode exists, so a misconfigured policy fails immediately and readably.")}
            {docs::specimen(&theme, "ResourceLimits, field by field", ui! { {field_list(&theme, &LIMIT_FIELDS)} })}
            {docs::body("From an application's side, three of these are yours to reason about: the tree budget bounds what a data-driven UI may build, the output budget bounds one frame, and the image budgets bound what a gallery may hold. `RenderError` reports the failure with its typed cause, so a failure can be surfaced instead of guessed at.")}
            {docs::production_note(&theme, "A limit that is too tight is a bug report waiting to happen; a limit that is too loose is a memory spike waiting to happen. Start from the defaults and only raise what your own measurements justify.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ResourceLimits", purpose: "hard ceilings for the whole runtime", defaults: "validated at startup", events: "none" },
                ApiRow { name: "ImageRenderOptions", purpose: "fit, alignment, and mode per surface", defaults: "contain, centered, Auto", events: "none" },
                ApiRow { name: "RenderError", purpose: "typed reason a session could not run", defaults: "carries its cause", events: "none" },
            ])}
        },
    );

    let testing = section(
        &theme,
        data,
        6,
        &sections[6],
        ui! {
            {docs::body("Test each decision at the layer that owns it. A reducer is a pure function and needs no terminal. A component needs only a committed frame. Geometry needs a fixed viewport. Routing needs events. Only the last pass needs a human at a real terminal.")}
            {docs::specimen(&theme, "the layers", ui! {
                {field_list(&theme, &[
                    Field { name: "pure state tests", default: "fastest", meaning: "Drive the reducers behind your demonstrations; this guide tests its checklist, publish timer, and bounded log exactly this way." },
                    Field { name: "component commits", default: "no terminal", meaning: "Lower a tree and commit it: a non-empty frame proves the component builds and lays out." },
                    Field { name: "fixed-viewport commits", default: "deterministic", meaning: "Commit at 140×45, 120×40, 80×24, and 60×20 to pin the responsive behavior." },
                    Field { name: "event routing", default: "targeted", meaning: "Send key and pointer events into a headless pipeline and assert what changed." },
                    Field { name: "terminal smoke check", default: "manual", meaning: "Only a real terminal can confirm image protocols, clipboard, and restoration." },
                ])}
            })}
            {docs::source_block(&theme, "a fixed-viewport commit test", snippets::TEST_COMMIT)}
            {docs::notice(&theme, "This guide's own suite commits every chapter at four sizes in both themes, compiles every displayed snippet, and decodes its bundled image — all without a terminal.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Commit", purpose: "lower, layout, and produce a frame", defaults: "explicit viewport", events: "optional event channel" },
                ApiRow { name: "Lower", purpose: "turn a node tree into a DOM", defaults: "advanced tier", events: "none" },
                ApiRow { name: "Runtime", purpose: "drive a headless pipeline", defaults: "advanced tier", events: "input and output channels" },
            ])}
        },
    );

    let checklist = section(
        &theme,
        data,
        7,
        &sections[7],
        ui! {
            {docs::body("The last pass is a checklist, because the failures that reach users are the ones nobody ran. Every row below is something a fixed viewport cannot prove.")}
            {docs::live_example(
                &theme,
                "release checklist",
                "Tick rows as you go; the progress bar and status are derived from the same state.",
                ui! { {checklist_demo.apply(())} },
                None,
            )}
            {docs::callout(&theme, CalloutKind::Production, "Run this list twice: once in the terminal you develop in, and once in the most constrained terminal you intend to support. Native image output and clipboard behavior are exactly where the two differ.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "checkbox", purpose: "controlled boolean row", defaults: "checked=false, disabled=false", events: "on_change" },
                ApiRow { name: "progress_bar", purpose: "caller-owned completion display", defaults: "max=100, width=20, percentage shown", events: "none" },
                ApiRow { name: "badge", purpose: "compact status with a textual cue", defaults: "BadgeVariant::Primary", events: "none" },
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
                {runtime}
                {lifecycle}
                {exit}
                {effects}
                {render_path}
                {limits}
                {testing}
                {checklist}
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}
