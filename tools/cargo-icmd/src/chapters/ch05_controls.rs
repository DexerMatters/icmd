//! Chapter 05 — Controls and Events.
//!
//! Semantic activation, controlled selection controls, both editor ownership
//! models, the raw input engine, focus and keyboard routing, and a bounded
//! event ledger.

use crossterm::event::{KeyCode, KeyModifiers};

use icmd::theme::Theme;
use icmd::{
    Align, BorderKind, ButtonVariant, Component, ComponentContext, Dimension, DomProps, Edges,
    FocusEvent, Justify, KeyboardEvent, Layout, Node, Props, RawInputAppearance, RawInputMode,
    ScrollAxes, ScrollbarVisibility, Span, Text, TextStyle, TextValueEvent, TextWrap, button, card,
    checkbox, column, empty, input, muted, radio, raw_input, row, scroll_area, switch, textarea,
    ui, view,
};

use super::{ChapterProps, document, masthead, route_card, section};
use crate::demos;
use crate::docs::{self, ApiRow, CalloutKind};
use crate::metadata::{self, SectionMeta};
use crate::snippets;

/// Five semantic actions in one place: variants, disabled, and autofocus.
fn button_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (last, set_last) = cx.use_state(|| String::from("no action yet"));
    let (presses, set_presses) = cx.use_state(|| 0_u32);
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let (publish_last, publish_count) = (set_last.clone(), set_presses.clone());
    let (save_last, save_count) = (set_last.clone(), set_presses.clone());
    let (discard_last, discard_count) = (set_last.clone(), set_presses.clone());
    let (canary_last, canary_count) = (set_last.clone(), set_presses.clone());
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                <button
                    autofocus={true}
                    on_press={move |_| {
                        publish_last.set(String::from("publish (primary)"));
                        publish_count.update(|value| *value += 1);
                    }}>"Publish"</button>
                <button
                    variant={ButtonVariant::Secondary}
                    on_press={move |_| {
                        save_last.set(String::from("save draft (secondary)"));
                        save_count.update(|value| *value += 1);
                    }}>"Save draft"</button>
                <button
                    variant={ButtonVariant::Destructive}
                    on_press={move |_| {
                        discard_last.set(String::from("discard (destructive)"));
                        discard_count.update(|value| *value += 1);
                    }}>"Discard"</button>
            </row>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 2;
            }}>
                <button
                    disabled={true}
                    on_press={move |_| {
                        canary_last.set(String::from("disabled fired: this should never be seen"));
                        canary_count.update(|value| *value += 1);
                    }}>"Publish to canary"</button>
                <muted>{Text::new("disabled: the handler is installed but never reached").wrap(TextWrap::Soft)}</muted>
            </row>
            {Text::from_spans([
                Span::new("▶ last activation: ").foreground(muted_foreground),
                Span::new(last).foreground(primary).bold(),
                Span::new(format!("   ·   accepted activations {presses}")),
            ]).wrap(TextWrap::Soft)}
            <muted>{Text::new(
                "Publish asks for focus on mount, so it is already outlined: press Enter or Space to activate it. Clicking any control focuses it and fires the same on_press, and the disabled button never counts.",
            ).wrap(TextWrap::Soft)}</muted>
        </column>
    }
}

/// A preferences card whose live summary is written by the owner of the state.
fn preferences_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (telemetry, set_telemetry) = cx.use_state(|| true);
    let (channel, set_channel) = cx.use_state(|| 0_usize);
    let (compact, set_compact) = cx.use_state(|| false);
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let stable = set_channel.clone();
    let nightly = set_channel.clone();
    let summary = format!(
        "telemetry {} · channel {} · rows {}",
        if telemetry { "on" } else { "off" },
        if channel == 0 { "stable" } else { "nightly" },
        if compact { "compact" } else { "comfortable" },
    );
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <checkbox
                checked={telemetry}
                label={"Send anonymous telemetry"}
                on_change={move |next: bool| set_telemetry.set(next)} />
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 2;
            }}>
                <radio
                    selected={channel == 0}
                    label={"Stable channel"}
                    on_select={move |_| stable.set(0)} />
                <radio
                    selected={channel == 1}
                    label={"Nightly channel"}
                    on_select={move |_| nightly.set(1)} />
            </row>
            <switch
                on={compact}
                label={"Compact rows"}
                on_change={move |next: bool| set_compact.set(next)} />
            {Text::from_spans([
                Span::new("● summary: ").foreground(primary),
                Span::new(summary).foreground(muted_foreground),
            ]).wrap(TextWrap::Soft)}
            <muted>{Text::new(
                "The controls render what they are told. A checkbox and a switch carry the proposed bool; a radio carries nothing, because exclusivity is the group owner's decision.",
            ).wrap(TextWrap::Soft)}</muted>
        </card>
    }
}

/// Both ownership models in one form, plus the disabled and read-only affordances.
fn input_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (name, set_name) = cx.use_state(|| String::from("field-guide"));
    let (submitted, set_submitted) = cx.use_state(|| String::from("nothing submitted"));
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let change = set_name.clone();
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {Text::new("CONTROLLED · value + on_change").foreground(muted_foreground).bold()}
            <input
                value={name.clone()}
                max_length={24}
                placeholder={"workspace name"}
                style={|style| { style.width /= Dimension::Max; }}
                on_change={move |event: TextValueEvent| change.set(event.value)} />
            {Text::from_spans([
                Span::new("owner state: ").foreground(muted_foreground),
                Span::new(name).foreground(primary).bold(),
            ]).wrap(TextWrap::Soft)}
            {Text::new("UNCONTROLLED · default_value + on_submit").foreground(muted_foreground).bold()}
            <input
                default_value={"draft-001"}
                placeholder={"seeded once, then self-owned"}
                style={|style| { style.width /= Dimension::Max; }}
                on_submit={move |event: TextValueEvent| set_submitted.set(event.value)} />
            {Text::from_spans([
                Span::new("last submit: ").foreground(muted_foreground),
                Span::new(submitted).foreground(primary).bold(),
            ]).wrap(TextWrap::Soft)}
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 2;
            }}>
                <input
                    value={"frozen"}
                    read_only={true}
                    style={|style| { style.width /= Dimension::Cells(18); }} />
                <input
                    placeholder={"disabled"}
                    disabled={true}
                    style={|style| { style.width /= Dimension::Cells(18); }} />
            </row>
            <muted>{Text::new("The first accepts selection and copy; the second is inert.").wrap(TextWrap::Soft)}</muted>
        </column>
    }
}

/// A commit-message editor with a wrap toggle and a capped height.
fn textarea_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (message, set_message) = cx.use_state(|| {
        String::from(
            "Document the selection engine.\n\n- active and inactive styling\n- link the rustdoc",
        )
    });
    let (hard, set_hard) = cx.use_state(|| false);
    let change = set_message.clone();
    let toggle = set_hard.clone();
    let wrap = if hard { TextWrap::Hard } else { TextWrap::Soft };
    let characters = message.chars().count();
    let lines = message.lines().count().max(1);
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                <button
                    variant={ButtonVariant::Secondary}
                    on_press={move |_| toggle.update(|value| *value = !*value)}>
                    {if hard { "Wrap: Hard" } else { "Wrap: Soft" }}
                </button>
                <muted>{Text::new(format!("{characters} characters · {lines} lines · height capped at 6 rows")).wrap(TextWrap::Soft)}</muted>
            </row>
            <textarea
                value={message.clone()}
                wrap={wrap}
                placeholder={"describe the change"}
                max_length={400}
                style={|style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(6);
                }}
                on_change={move |event: TextValueEvent| change.set(event.value)} />
            <muted>{Text::new(
                "The value is controlled here; drop value and use default_value for the uncontrolled model. Pointer selection, Shift+arrows, Ctrl+A, Ctrl+C, Ctrl+X, and paste are all handled by the editor engine.",
            ).wrap(TextWrap::Soft)}</muted>
        </column>
    }
}

/// A command field built directly on `raw_input`: policy plus presentation.
fn command_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (value, set_value) = cx.use_state(String::new);
    let (log, set_log) = cx.use_state(Vec::<String>::new);
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let primary_foreground = theme.colors.primary_foreground;
    let muted_foreground = theme.colors.muted_foreground;
    let accent = theme.colors.accent;
    let surface = theme.colors.muted;
    let border = theme.colors.border;
    let appearance = RawInputAppearance {
        placeholder: TextStyle::default().foreground(muted_foreground),
        selection: TextStyle::default()
            .foreground(primary_foreground)
            .background(primary),
        selection_inactive: TextStyle::default().dim(),
        caret: TextStyle::default().reverse(),
        focused_border: Some(accent),
    };
    let change = set_value.clone();
    let submit_value = set_value.clone();
    let ledger = set_log.clone();
    let entries = log
        .iter()
        .rev()
        .map(|entry| ui! { <muted>{Text::new(entry.clone())}</muted> })
        .collect::<Node>();
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <muted>{Text::new("COMMAND FIELD · appearance + policy, not editing mechanics").wrap(TextWrap::Soft)}</muted>
            <raw_input
                mode={RawInputMode::SingleLine}
                value={value.clone()}
                placeholder={":type a command"}
                max_length={48}
                appearance={appearance}
                on_change={move |event: TextValueEvent| change.set(event.value)}
                on_submit={move |event: TextValueEvent| {
                    let command = event.value.trim().to_owned();
                    if !command.is_empty() {
                        ledger.update(move |entries| {
                            demos::push_bounded(entries, format!("ran {command}"), 8);
                        });
                    }
                    submit_value.set(String::new());
                }}
                style={move |style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(3);
                    style.padding /= Edges::symmetric(0, 1);
                    style.background /= surface;
                    style.border.kind /= BorderKind::Rounded;
                    style.border.foreground /= border;
                    style.border.background /= surface;
                }} />
            {Text::new("RECENT COMMANDS").foreground(muted_foreground).bold()}
            <scroll_area
                axes={ScrollAxes::Vertical}
                scrollbar_visibility={ScrollbarVisibility::Auto}
                style={move |style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(5);
                    style.padding /= Edges::symmetric(0, 1);
                    style.background /= surface;
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                }}>{entries}</view>
            </scroll_area>
            <muted>{Text::new(
                "Enter submits, the value clears, and the command is recorded. Caret math, key maps, selection, paste, and clipboard remain inside the engine; this component supplies the host styling, the appearance, and the max length.",
            ).wrap(TextWrap::Soft)}</muted>
        </column>
    }
}

/// A focusable panel: local key handling, button activation, and an app shortcut.
fn focus_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    const PANES: [&str; 3] = ["Build", "Docs", "Deploy"];
    let (pane, set_pane) = cx.use_state(|| 0_usize);
    let (activations, set_activations) = cx.use_state(|| 0_u32);
    let (last_key, set_last_key) = cx.use_state(|| String::from("none"));
    let (shortcut, set_shortcut) = cx.use_state(|| String::from("none yet"));
    let (focused, set_focused) = cx.use_state(|| false);
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let primary_foreground = theme.colors.primary_foreground;
    let muted_foreground = theme.colors.muted_foreground;
    let accent = theme.colors.accent;
    let border = theme.colors.border;
    let surface = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;

    let panes = PANES
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if index == pane {
                ui! {
                    {Text::new(format!("▸ {name}"))
                        .foreground(primary_foreground)
                        .background(primary)
                        .bold()}
                }
            } else {
                ui! { {Text::new(*name).foreground(muted_foreground)} }
            }
        })
        .collect::<Node>();

    let keyboard = {
        let pane = set_pane.clone();
        let last_key = set_last_key.clone();
        move |event: KeyboardEvent| {
            let label = format!("{:?}", event.key.code);
            let handled = match event.key.code {
                KeyCode::Left => {
                    pane.update(|value| *value = value.saturating_sub(1));
                    true
                }
                KeyCode::Right => {
                    pane.update(|value| *value = (*value + 1).min(PANES.len() - 1));
                    true
                }
                KeyCode::Enter | KeyCode::Char(' ') => true,
                _ => false,
            };
            if handled {
                last_key.set(label);
                event.stop_propagation();
            }
        }
    };
    let app_shortcut = {
        let shortcut = set_shortcut.clone();
        move |event: KeyboardEvent| {
            if event.key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(event.key.code, KeyCode::Char('j'))
            {
                shortcut.set(String::from("Ctrl+J"));
            }
        }
    };
    let focus_listener = {
        let focused = set_focused.clone();
        move |event: FocusEvent| focused.set(event == FocusEvent::Gained)
    };
    let activate = set_activations.clone();

    let mut dom = DomProps::default().with_focusable(true);
    dom.style.layout /= Layout::Vertical;
    dom.style.width /= Dimension::Max;
    dom.style.gap /= 1;
    dom.style.padding /= Edges::all(1);
    dom.style.background /= surface;
    dom.style.text.foreground /= card_foreground;
    dom.style.border.kind /= BorderKind::Rounded;
    dom.style.border.foreground /= if focused { accent } else { border };
    dom.style.border.background /= surface;

    let focus_label = if focused {
        "● panel owns focus"
    } else {
        "○ panel not focused"
    };
    ui! {
        <view
            dom={dom}
            on_key_down={keyboard}
            on_app_key={app_shortcut}
            on_focus_event={focus_listener}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new("FOCUSABLE PANEL").foreground(muted_foreground).bold()}
                {Text::new(focus_label).foreground(if focused { accent } else { muted_foreground })}
            </row>
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 2; }}>{panes}</row>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                <button
                    variant={ButtonVariant::Secondary}
                    on_press={move |_| activate.update(|value| *value += 1)}>
                    "Run focused action"
                </button>
                <muted>{Text::new(format!("activations {activations} · last local key {last_key}"))}</muted>
            </row>
            <muted>{Text::new(
                "Click the panel to focus it: ← and → move the highlighted pane and the handler stops the key before it scrolls the page. Click the action button, then press Enter or Space to activate it. Press Ctrl+J anywhere to record the app-level shortcut.",
            ).wrap(TextWrap::Soft)}</muted>
            <muted>{Text::new(format!("last global shortcut: {shortcut}"))}</muted>
        </view>
    }
}

/// The capture, target, and bubble phases as a labeled flow.
fn event_phase_flow(theme: &Theme) -> Node {
    const DETAIL: [&str; 3] = [
        "root → target, before the target's own listeners; a capture listener can stop the dispatch",
        "the hit region for pointer input, or the focused region for keys",
        "target → root; ancestors see the event after the target",
    ];
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let card = theme.colors.card;
    let border = theme.colors.border;
    let last = demos::EVENT_PHASES.len() - 1;
    demos::EVENT_PHASES
        .iter()
        .zip(DETAIL)
        .enumerate()
        .map(|(index, (phase, detail))| {
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
                        style.gap /= 2;
                        style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
                        style.background /= card;
                        style.border.kind /= BorderKind::Single;
                        style.border.foreground /= border;
                        style.border.background /= card;
                    }}>
                        {Text::new(format!("{}. {phase}", index + 1)).foreground(primary).bold()}
                        <muted>{Text::new(detail).wrap(TextWrap::Soft)}</muted>
                    </row>
                    {if index < last {
                        ui! { <muted>{Text::new("↓").foreground(muted_foreground)}</muted> }
                    } else {
                        empty()
                    }}
                </view>
            }
        })
        .collect::<Node>()
}

/// A preferences form that records recent semantic events in a bounded ledger.
fn ledger_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (telemetry, set_telemetry) = cx.use_state(|| true);
    let (verbose, set_verbose) = cx.use_state(|| false);
    let (channel, set_channel) = cx.use_state(|| 0_usize);
    let (log, set_log) = cx.use_state(Vec::<String>::new);
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let surface = theme.colors.muted;
    let telemetry_log = set_log.clone();
    let verbose_log = set_log.clone();
    let stable_channel = set_channel.clone();
    let stable_log = set_log.clone();
    let nightly_channel = set_channel.clone();
    let nightly_log = set_log.clone();
    let entries = log
        .iter()
        .rev()
        .map(|entry| ui! { <muted>{Text::new(entry.clone())}</muted> })
        .collect::<Node>();
    let depth = log.len();
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <muted>{Text::new(format!("every control reports a semantic event; the ledger keeps the newest 8 ({depth} held)")).wrap(TextWrap::Soft)}</muted>
            <checkbox
                checked={telemetry}
                label={"Send telemetry"}
                on_change={move |next: bool| {
                    set_telemetry.set(next);
                    telemetry_log.update(move |entries| {
                        demos::push_bounded(entries, format!("checkbox → telemetry {next}"), 8);
                    });
                }} />
            <switch
                on={verbose}
                label={"Verbose frames"}
                on_change={move |next: bool| {
                    set_verbose.set(next);
                    verbose_log.update(move |entries| {
                        demos::push_bounded(entries, format!("switch → verbose {next}"), 8);
                    });
                }} />
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                <radio
                    selected={channel == 0}
                    label={"Stable"}
                    on_select={move |_| {
                        stable_channel.set(0);
                        stable_log.update(|entries| {
                            demos::push_bounded(entries, String::from("radio → stable"), 8);
                        });
                    }} />
                <radio
                    selected={channel == 1}
                    label={"Nightly"}
                    on_select={move |_| {
                        nightly_channel.set(1);
                        nightly_log.update(|entries| {
                            demos::push_bounded(entries, String::from("radio → nightly"), 8);
                        });
                    }} />
            </row>
            {Text::new("RECENT EVENTS").foreground(muted_foreground).bold()}
            <scroll_area
                axes={ScrollAxes::Vertical}
                scrollbar_visibility={ScrollbarVisibility::Auto}
                style={move |style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(6);
                    style.padding /= Edges::symmetric(0, 1);
                    style.background /= surface;
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                }}>{entries}</view>
            </scroll_area>
            {Text::from_spans([
                Span::new("▶ newest first: ").foreground(primary),
                Span::new("the log is bounded, so interaction cannot grow memory or page height.").foreground(muted_foreground),
            ]).wrap(TextWrap::Soft)}
        </card>
    }
}

/// Chapter 05.
pub(super) fn controls_and_events(cx: &mut ComponentContext, props: &Props<ChapterProps>) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(4);
    let sections: &[SectionMeta] = meta.sections;

    let buttons = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body_pair(
                "A button owns press semantics. It maps a pointer click and the Enter or Space key onto one activation, then reports that activation through `on_press`. The caller never attaches a raw click listener or reimplements the key test, so mouse and keyboard paths cannot drift apart.",
                "`variant` is a semantic role, not a paint choice: primary is the main action, secondary supports it, and destructive warns that the action removes something. `disabled` refuses activation and focus rather than only dimming the control, and `autofocus` requests focus after publication when nothing else already owns it.",
            )}
            {docs::live_example(
                &theme,
                "variants, disabled, and autofocus",
                "Click a button to focus and activate it, then press Enter or Space to activate it again. The readout names the semantic event that fired.",
                ui! { {button_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "one event", "pointer click, Enter, and Space all arrive as the same on_press")} }),
            )}
            {docs::watch_for(&theme, "Prefer `on_press` over `on_click`. A click listener only sees the pointer, so a keyboard user silently loses the action; composing a raw click listener onto a button also bypasses the disabled gate for that path.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "button", purpose: "semantic activation control", defaults: "primary, enabled, focusable", events: "on_press" },
                ApiRow { name: "ButtonProps", purpose: "variant, disabled, autofocus", defaults: "ButtonVariant::Primary", events: "on_press" },
                ApiRow { name: "ButtonVariant", purpose: "primary, secondary, or destructive role", defaults: "ButtonVariant::Primary", events: "none" },
                ApiRow { name: "on_press", purpose: "one call per accepted activation", defaults: "none", events: "pointer click, Enter, Space" },
                ApiRow { name: "disabled", purpose: "refuse activation and focus", defaults: "false", events: "none" },
            ])}
        },
    );

    let selection_controls = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body_pair(
                "Checkboxes, radios, and switches are controlled: the component renders the state it is given and asks the owner to change it. A checkbox and a switch report the proposed bool through `on_change`; a radio reports `()` through `on_select`, because a radio group is exclusive and only the group owner knows which peer should become selected.",
                "That contract keeps one writer for the value. The summary line below is derived from the same state the controls render, so the interface can never disagree with itself.",
            )}
            {docs::live_example(
                &theme,
                "a controlled preferences card",
                "Toggle the checkbox and switch, and move the radio selection. Each control asks for the change; the summary shows what the owner stored.",
                ui! { {preferences_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "the same card as a component", snippets::PREFERENCES_FORM)}
            {docs::notice(&theme, "`checked`, `on`, and `selected` are plain values, so the control is stateless and safe to drive from anywhere. Rendering a checkbox with `checked={state}` and then mutating that state inside the control would put two writers on one value, which is exactly the bug the controlled contract removes.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "checkbox", purpose: "independent on/off choice", defaults: "checked=false, enabled", events: "on_change(bool)" },
                ApiRow { name: "CheckboxProps", purpose: "checked, label, disabled, autofocus", defaults: "off, no label", events: "on_change" },
                ApiRow { name: "radio", purpose: "one choice in an exclusive group", defaults: "selected=false, enabled", events: "on_select(())" },
                ApiRow { name: "RadioProps", purpose: "selected, label, disabled, autofocus", defaults: "unselected, no label", events: "on_select" },
                ApiRow { name: "switch", purpose: "immediate on/off setting", defaults: "on=false, enabled", events: "on_change(bool)" },
                ApiRow { name: "SwitchProps", purpose: "on, label, disabled, autofocus", defaults: "off, no label", events: "on_change" },
            ])}
        },
    );

    let editing = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body_pair(
                "A text control follows one of two ownership models. With `value` set, the control is controlled: it renders that string and requests every edit through `on_change`, leaving the value with the caller. With only `default_value`, it is uncontrolled: it keeps the value itself and reports committed changes for observers.",
                "The rest of the surface is the same either way. `placeholder` shows while empty, `max_length` bounds the display length, `on_submit` reports Enter, `read_only` allows selection and copy but refuses edits, and `disabled` refuses both editing and focus.",
            )}
            {docs::live_example(
                &theme,
                "one form, both models",
                "Type in the first field and watch the owner state follow; type in the second and press Enter to submit it.",
                ui! { {input_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "pick one owner", "set value or default_value, not both, and let on_change carry the committed string")} }),
            )}
            {docs::callout(&theme, CalloutKind::Info, "`on_change` fires for each committed value change, and `on_submit` fires once per Enter in a single-line input. Both carry a `TextValueEvent` whose `value` is the editor's value at that moment; a controlled owner simply writes it back.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "input", purpose: "single-line text editing", defaults: "26x2 host, uncontrolled", events: "on_change, on_submit, on_clipboard" },
                ApiRow { name: "InputProps", purpose: "value, default_value, placeholder, max_length", defaults: "all unset", events: "on_change, on_submit" },
                ApiRow { name: "TextValueEvent", purpose: "the editor's value at change or submit", defaults: "—", events: "none" },
                ApiRow { name: "read_only", purpose: "selectable and copyable, not editable", defaults: "false", events: "none" },
                ApiRow { name: "disabled", purpose: "no editing and no focus", defaults: "false", events: "none" },
            ])}
        },
    );

    let multiline = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body_pair(
                "`textarea` is the same editor in multiline mode. It accepts the same controlled or uncontrolled value, adds `wrap` for `Soft` or `Hard` reflow, and gives the host a height you can cap; content that outgrows the box scrolls inside it rather than pushing the page.",
                "Selection, clipboard, and the editing keys belong to the engine, so the component only decides presentation and policy. Enter inserts a newline here, which is why a multiline control has no `on_submit`.",
            )}
            {docs::live_example(
                &theme,
                "a bounded commit-message editor",
                "Type or paste text, select with the pointer or Ctrl+A, copy with Ctrl+C, and switch the wrap mode to compare reflow.",
                ui! { {textarea_demo.apply(())} },
                None,
            )}
            {docs::notice(&theme, "A wrapped multiline editor scrolls on the vertical axis only, because rows are built at the granted width. An unwrapped one can overflow both axes and pans horizontally, exactly like a single-line input. `TextClipboardEvent` reports copy and cut with the text involved so an application can mirror the clipboard.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "textarea", purpose: "multiline text editing", defaults: "44x9 host, Soft wrap", events: "on_change, on_clipboard" },
                ApiRow { name: "TextareaProps", purpose: "value, default_value, wrap, max_length", defaults: "all unset", events: "on_change" },
                ApiRow { name: "TextWrap", purpose: "Soft reflow or Hard column break", defaults: "TextWrap::Soft", events: "none" },
                ApiRow { name: "TextClipboardEvent", purpose: "copy or cut with the involved text", defaults: "—", events: "on_clipboard" },
            ])}
        },
    );

    let raw = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body_pair(
                "`input` and `textarea` are thin policy layers over `raw_input`, the engine that actually edits. When a control needs a different presentation or a different policy, build on `raw_input` directly instead of reimplementing an editor.",
                "Two props do most of the work. `appearance` carries the placeholder, selection, inactive selection, caret, and focused border styles; `mode` selects single-line or multiline behavior. Editing policy stays declarative too: `max_length`, `read_only`, and `disabled` are forwarded, not re-derived.",
            )}
            {docs::live_example(
                &theme,
                "an application command field",
                "Type a command and press Enter. The host is restyled, the caret and selection use theme roles, and the history is bounded.",
                ui! { {command_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "presentation + policy", "the appearance, the mode, and the limits are props; caret math and key maps stay in the engine")} }),
            )}
            {docs::watch_for(&theme, "Do not build editing mechanics on top of a raw input. Caret arithmetic, grapheme-aware motion, key maps, paste reconciliation, and clipboard integration are already handled, and a second implementation will disagree with the first on Unicode and on selection.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "raw_input", purpose: "the editor engine as a component", defaults: "SingleLine mode", events: "on_change, on_submit, on_clipboard" },
                ApiRow { name: "RawInputProps", purpose: "mode, value, default_value, appearance, policy", defaults: "unset, single line", events: "on_change, on_submit" },
                ApiRow { name: "RawInputAppearance", purpose: "placeholder, selection, caret, focused border", defaults: "dim placeholder, reverse selection", events: "none" },
                ApiRow { name: "RawInputMode", purpose: "single-line or multiline behavior", defaults: "RawInputMode::SingleLine", events: "none" },
                ApiRow { name: "TextValueEvent", purpose: "the value at change or submit", defaults: "—", events: "none" },
            ])}
        },
    );

    let focus = section(
        &theme,
        data,
        5,
        &sections[5],
        ui! {
            {docs::body_pair(
                "Only focusable regions take part in focus. Focus changes explicitly: a pointer press focuses the region under the pointer, `autofocus` requests focus on mount when nothing else owns it, and the dispatcher can move focus programmatically. A focused button turns Enter or Space into the same activation a click produces, and a region can handle keys itself with `on_key_down`, which runs on the focused target's route.",
                "Application shortcuts use `on_app_key`, which receives a key that no focused widget consumed. That is why this guide binds modifier chords rather than bare characters: `Ctrl+K`, `Ctrl+B`, and `Alt+Arrow` can be reserved globally because a bare letter must keep reaching a demonstrated input. `Ctrl+C` is the runtime-managed exit key.",
            )}
            {docs::live_example(
                &theme,
                "local keys and a global shortcut",
                "Click the panel to focus it, then use ← and →. Click the action button and press Enter or Space. Press Ctrl+J for the app-level shortcut.",
                ui! { {focus_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "modifier chords", "global shortcuts use Ctrl or Alt so bare keys stay available to inputs")} }),
            )}
            {docs::notice(&theme, "A focused region is the only target for `on_key_down`; the handler runs from the target outward, so a descendant can consume a key before an ancestor sees it. `on_app_key` is different: it is a global press hook for keys nobody consumed, and every registered listener runs unless one stops propagation. `FocusEvent` reports DOM focus ownership, while `TerminalFocusEvent` reports whether the operating-system window is active.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "focusable", purpose: "whether a region may take focus", defaults: "false on plain views", events: "on_focus_event" },
                ApiRow { name: "autofocus", purpose: "request focus when nothing owns it", defaults: "false", events: "none" },
                ApiRow { name: "on_key_down", purpose: "handle a key on the focused route", defaults: "none", events: "press and repeat" },
                ApiRow { name: "on_app_key", purpose: "global shortcut for unconsumed keys", defaults: "none", events: "press and repeat" },
                ApiRow { name: "KeyboardEvent", purpose: "key code, modifiers, and edge", defaults: "—", events: "none" },
                ApiRow { name: "FocusEvent", purpose: "gained or lost DOM focus", defaults: "—", events: "on_focus_event" },
            ])}
        },
    );

    let events = section(
        &theme,
        data,
        6,
        &sections[6],
        ui! {
            {docs::body_pair(
                "Every input family has a payload type that names what happened. `PointerEvent` describes a press, release, move, hover, or completed click with button state and modifiers; `WheelEvent` carries a delta; `PasteEvent` carries the pasted text; `ResizeEvent` carries the new viewport size; `FocusEvent` reports DOM focus ownership and `TerminalFocusEvent` the terminal window's activation.",
                "Delivery is the same for all of them. A dispatch travels capture, target, then bubble. The target's `on_*` handler runs in the target phase, ancestors run in bubble, and the `*_capture` fields on `EventHandlers` run root-to-target first, before the target's own listeners.",
            )}
            {docs::specimen(&theme, "one dispatch, three phases", ui! { {event_phase_flow(&theme)} })}
            {docs::two_column(
                &theme,
                data.wide(),
                ui! {
                    {docs::ref_row(&theme, "PointerEvent", "Press, release, move, hover, or click with button state and modifiers.", "on_pointer_down · on_click")}
                    {docs::ref_row(&theme, "WheelEvent", "A wheel or trackpad scroll with per-axis deltas.", "on_wheel")}
                    {docs::ref_row(&theme, "PasteEvent", "Bracketed paste text delivered to the focused route.", "on_paste_event")}
                },
                ui! {
                    {docs::ref_row(&theme, "ResizeEvent", "The terminal viewport changed size.", "on_resize_event")}
                    {docs::ref_row(&theme, "FocusEvent", "The region gained or lost DOM focus.", "on_focus_event")}
                    {docs::ref_row(&theme, "TerminalFocusEvent", "The terminal window became active or inactive.", "on_terminal_focus")}
                },
            )}
            {docs::callout(&theme, CalloutKind::Info, "Any event can `stop_propagation()` to end the dispatch and `prevent_default()` to disable the framework's built-in action, such as focus-on-press, wheel scrolling, or text editing. That is the whole routing surface an application needs; the dispatcher's registry, capture ordering, and hit testing stay in rustdoc.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "PointerEvent", purpose: "pointer interaction with buttons and modifiers", defaults: "—", events: "on_pointer_*, on_click" },
                ApiRow { name: "WheelEvent", purpose: "per-axis wheel delta at a position", defaults: "—", events: "on_wheel" },
                ApiRow { name: "PasteEvent", purpose: "pasted text on the focused route", defaults: "—", events: "on_paste_event" },
                ApiRow { name: "ResizeEvent", purpose: "new viewport size in cells", defaults: "—", events: "on_resize_event" },
                ApiRow { name: "FocusEvent", purpose: "DOM focus gained or lost", defaults: "—", events: "on_focus_event" },
                ApiRow { name: "TerminalFocusEvent", purpose: "terminal window activation", defaults: "—", events: "on_terminal_focus" },
                ApiRow { name: "EventPhase", purpose: "capture, target, or bubble", defaults: "EventPhase::Target", events: "none" },
                ApiRow { name: "EventHandlers", purpose: "the per-region listener table", defaults: "all slots unset", events: "every family above" },
            ])}
        },
    );

    let ledger = section(
        &theme,
        data,
        7,
        &sections[7],
        ui! {
            {docs::body_pair(
                "A ledger is the honest way to show event flow: let the interface perform semantic actions, then record what was reported. This form writes one entry per change, newest first, inside a small scrollable region.",
                "The log is bounded with the same helper every accumulating demonstration in this guide uses. Without a bound, a form like this grows one string per interaction until the page height and the memory it needs follow the reader's patience.",
            )}
            {docs::live_example(
                &theme,
                "recent semantic events",
                "Change any control and watch the ledger. It keeps the newest eight entries no matter how long you interact.",
                ui! { {ledger_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "the ledger as a component", snippets::EVENT_LEDGER)}
            {docs::production_note(&theme, "Bound every accumulating collection, and prefer a semantic log over a raw one. \"radio → nightly\" is stable and cheap to read; a dump of every pointer move is neither, and it is the fastest way to turn a quiet interface into a busy one.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "use_state", purpose: "the owner of the value and the log", defaults: "initial closure runs once", events: "set, update" },
                ApiRow { name: "scroll_area", purpose: "a fixed-height window over the ledger", defaults: "vertical, auto scrollbar", events: "on_scroll" },
                ApiRow { name: "checkbox", purpose: "boolean preference", defaults: "off", events: "on_change(bool)" },
                ApiRow { name: "switch", purpose: "immediate setting", defaults: "off", events: "on_change(bool)" },
                ApiRow { name: "radio", purpose: "exclusive group member", defaults: "unselected", events: "on_select(())" },
                ApiRow { name: "push_bounded", purpose: "keep the newest entries under a cap", defaults: "cap chosen by the caller", events: "none" },
            ])}
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {route_card(&theme, data, 5, "◐", "06 · Feedback and Canvas", "Badges, alerts, progress, spinners, and direct cell drawing.")}
            </view>
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
                {buttons}
                {selection_controls}
                {editing}
                {multiline}
                {raw}
                {focus}
                {events}
                {ledger}
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}

#[cfg(test)]
mod tests {
    use icmd::{Component, Size, selection_area};

    use super::*;

    /// The guide wraps every lesson in one selection region. An editor surface
    /// owns its own selection, so it must still paint there rather than be
    /// skipped along with the document text it is not part of.
    #[test]
    fn text_controls_paint_inside_a_selection_region() {
        let screen = crate::screen::render_node(
            icmd::ui! {
                <selection_area style={|style| { style.width /= icmd::Dimension::Max; }}>
                    {input_demo.apply(())}
                </selection_area>
            },
            Size::new(90, 24),
        );
        for expected in ["field-guide", "draft-001", "frozen", "disabled"] {
            assert!(
                screen.contains(expected),
                "an editor inside a selection region must paint `{expected}`:\n{}",
                screen.text()
            );
        }
    }

    /// The controlled and uncontrolled input lesson shows both values.
    #[test]
    fn the_input_lesson_paints_both_ownership_models() {
        let screen = crate::screen::render_section(4, 2, Size::new(110, 40));
        for expected in ["field-guide", "draft-001", "frozen", "disabled"] {
            assert!(
                screen.contains(expected),
                "the input lesson must paint `{expected}`:\n{}",
                screen.text()
            );
        }
    }

    /// The multiline lesson paints its controlled value.
    #[test]
    fn the_textarea_lesson_paints_its_value() {
        let screen = crate::screen::render_section(4, 3, Size::new(110, 40));
        assert!(
            screen.contains("Document the selection engine"),
            "the textarea lesson must paint its value:\n{}",
            screen.text()
        );
    }
}
