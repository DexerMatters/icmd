#![recursion_limit = "512"]

use std::{
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossterm::{event::KeyCode, style::Color};
use icmd::{
    AlertVariant, Align, Attr, BadgeVariant, Component, ComponentContext, Dimension, Edges,
    EventListener, Justify, KeyboardEvent, Layout, Node, Percent, Props, RawInputMode,
    RawInputProps, RuntimeConfig, ScrollAxes, ScrollEvent, ScrollOffset, StateSetter, Text,
    TextOverflow, TextValueEvent, TextWrap, raw_input, render, theme_provider, ui, view,
};
use icmd::{
    EmojiMerging,
    theme::{Theme, ThemeMode, ThemePreset},
};
use icmd::{
    alert, badge, blockquote, button, canvas, card, center, checkbox, code, column, divider,
    heading, input, kbd, label, muted, paragraph, progress_bar, radio, row, scroll_area, skeleton,
    spacer, spinner, switch, textarea,
};

const TABS: [&str; 4] = ["Overview", "Components", "Data", "Theme"];

/// Props for the demo's custom field.
#[derive(Clone, Default)]
pub struct CommandFieldProps {
    pub value: Attr<String>,
    pub on_change: Attr<EventListener<TextValueEvent>>,
}

/// A user-defined field built by wrapping `raw_input`.
///
/// It chooses single-line policy, forwards the caller's value and change
/// observer, and restyles the host. The caller's pointer observer runs on the
/// same host that performs the editing, so it sees every press while the caret
/// still moves.
pub fn command_field(cx: &mut ComponentContext, props: &Props<CommandFieldProps>) -> Node {
    let hits = Arc::new(Mutex::new(String::new()));
    let label = hits.clone();
    let _ = cx;
    raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::SingleLine),
            value: props.value.clone(),
            placeholder: Attr::Set("command…".into()),
            on_change: props.on_change.clone(),
            ..RawInputProps::default()
        })
        .events({
            let label = label.clone();
            move |handlers: &mut icmd::EventHandlers| {
                handlers.pointer_down = Attr::Set(EventListener::new(move |_event| {
                    let mut slot = label.lock().unwrap();
                    *slot = "command field focused".to_string();
                }));
            }
        })
        .style(|style| {
            style.width /= Dimension::Cells(24);
            style.border.foreground /= Color::Yellow;
        })
        .node()
}

fn custom_field_demo() -> Node {
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
        }}>
            <label>"Custom wrapper over raw_input"</label>
            <command_field
                value={"run --all"}
                on_change={move |_event: TextValueEvent| {}}
            />
            <muted>"Wraps raw_input; forwards props, style, and events"</muted>
        </view>
    }
}

fn overview(theme: &Theme, tick: usize) -> Node {
    let progress_value = 42 + tick as u64 % 49;
    let canvas_background = theme.colors.card;
    let canvas_grid = theme.colors.border;
    let canvas_accent = theme.colors.primary;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <card style={|style| style.width /= Dimension::Max}>
                <view style={|style| {
                    style.layout /= Layout::Horizontal;
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.justify /= Justify::SpaceBetween;
                    style.gap /= 0;
                }}>
                    <heading>"Terminal UI, composed like a web page"</heading>
                    <badge text="LIVE" variant={BadgeVariant::Secondary} />
                </view>
                <paragraph>{Text::new(
                    "Dense by design: keyboard-first controls, cell-aware layout and incremental ANSI updates.",
                ).wrap(TextWrap::Hard)}</paragraph>
                <center>{Text::new("context-aware  •  incremental  •  Unicode-ready")
                    .foreground(theme.colors.accent)}</center>
            </card>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <card style={|style| style.width /= Dimension::Percent(Percent::available(32))}>
                    {Text::new("24").foreground(theme.colors.primary).bold()}
                    {Text::new("mounted nodes").text_style(theme.typography.muted.clone())}
                </card>
                <card style={|style| style.width /= Dimension::Percent(Percent::available(32))}>
                    {Text::new("99.9%").foreground(theme.colors.secondary).bold()}
                    {Text::new("render uptime").text_style(theme.typography.muted.clone())}
                </card>
                <card style={|style| style.width /= Dimension::Percent(Percent::available(32))}>
                    {Text::new("0").foreground(theme.colors.accent).bold()}
                    {Text::new("layout faults").text_style(theme.typography.muted.clone())}
                </card>
            </view>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <card style={|style| style.width /= Dimension::Percent(Percent::available(43))}>
                    <heading>"Live runtime"</heading>
                    <progress_bar value={progress_value} max={100} width={24} label="build" />
                    <spinner frame={tick} label="streaming diff frames" />
                    <muted>"Damaged cells only."</muted>
                </card>
                <card style={|style| style.width /= Dimension::Percent(Percent::available(43))}>
                    <view style={|style| {
                        style.layout /= Layout::Horizontal;
                        style.width /= Dimension::Max;
                        style.align /= Align::Center;
                        style.justify /= Justify::SpaceBetween;
                        style.gap /= 0;
                    }}>
                        <heading>"Renderer activity"</heading>
                        <muted>"last 30 frames"</muted>
                    </view>
                    <canvas width={32} height={5} draw={Arc::new(move |drawing| {
                        drawing.set_background(canvas_background);
                        drawing.clear().unwrap();
                        drawing.set_foreground(canvas_grid);
                        drawing.line(0, 4, 31, 4, "─").unwrap();
                        drawing.set_foreground(canvas_accent);
                        drawing.line(1, 3, 7, 1, "•").unwrap();
                        drawing.line(7, 1, 13, 3, "•").unwrap();
                        drawing.line(13, 3, 20, 0, "•").unwrap();
                        drawing.line(20, 0, 30, 2, "•").unwrap();
                        drawing.fill_text("FRAME DELTA", 1, 0).unwrap();
                    })} />
                </card>
            </view>
            <alert
                title="Runtime healthy"
                message="Mouse, focus, paste, resize and keyboard events are connected."
                variant={AlertVariant::Success}
            />
        </view>
    }
}

#[allow(clippy::too_many_arguments)]
fn components_page(
    completed: bool,
    set_completed: &StateSetter<bool>,
    notifications: bool,
    set_notifications: &StateSetter<bool>,
    query: String,
    set_query: &StateSetter<String>,
    wrap: TextWrap,
    set_wrap: &StateSetter<TextWrap>,
) -> Node {
    let check = {
        let setter = set_completed.clone();
        ui! {
            <checkbox
                checked={completed}
                label="Show completed"
                on_click={move |_| setter.set(!completed)}
            />
        }
    };
    let radio = ui! { <radio selected label="Incremental mode" /> };
    let toggle = {
        let setter = set_notifications.clone();
        ui! {
            <switch
                on={notifications}
                label="Notifications"
                on_click={move |_| setter.set(!notifications)}
            />
        }
    };
    let query_setter = set_query.clone();
    let wrap_setter = set_wrap.clone();
    let next_wrap = match wrap {
        TextWrap::NoWrap => TextWrap::Soft,
        TextWrap::Soft => TextWrap::Hard,
        TextWrap::Hard => TextWrap::NoWrap,
    };

    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <card style={|style| style.width /= Dimension::Max}>
                <heading>"Component gallery"</heading>
                <muted>"Click the checkbox and switch; focus follows the pointer."</muted>
            </card>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <card style={|style| style.width /= Dimension::Percent(Percent::available(32))}>
                    <heading>"Controls"</heading>
                    <label>"Controlled input"</label>
                    <input
                        value={query}
                        placeholder="filter widgets…"
                        on_change={move |event: TextValueEvent| query_setter.set(event.value)}
                    />
                    <label>"Uncontrolled textarea"</label>
                    <textarea
                        wrap={wrap}
                        style={|style| {
                            style.width /= Dimension::Cells(30);
                            style.height /= Dimension::Cells(8);
                        }}
                        default_value={"Long Unicode text: 这是一个很长的示例文本，含有 emoji 👩‍💻 and an unbreakable-token-for-hard-wrap."}
                    />
                    <row>
                        <button on_click={move |_| wrap_setter.set(next_wrap)}>
                            {format!("wrap: {:?}", wrap)}
                        </button>
                        <muted>"Ctrl+C / Ctrl+X uses host clipboard"</muted>
                    </row>
                    {custom_field_demo()}
                    {check}
                    {radio}
                    {toggle}
                    <row><kbd>"Enter"</kbd><muted>" activate"</muted></row>
                </card>
                <card style={|style| style.width /= Dimension::Percent(Percent::available(32))}>
                    <heading>"Typography"</heading>
                    <label>"Semantic text styles"</label>
                    <paragraph>{Text::new("Body copy wraps on terminal cell boundaries.")
                        .wrap(TextWrap::Hard)}</paragraph>
                    <blockquote>"Small APIs make composition predictable."</blockquote>
                    <code>"state.update(|v| *v += 1)"</code>
                </card>
                <card style={|style| style.width /= Dimension::Percent(Percent::available(32))}>
                    <heading>"Feedback"</heading>
                    <badge text="primary" variant={BadgeVariant::Primary} />
                    <badge text="success" variant={BadgeVariant::Secondary} />
                    <badge text="warning" variant={BadgeVariant::Accent} />
                    <badge text="error" variant={BadgeVariant::Destructive} />
                    <skeleton width={20} />
                    {Text::new("A deliberately clipped status message")
                        .layout_style(icmd::style(|style| style.width /= Dimension::Cells(24)))
                        .overflow(TextOverflow::Ellipsis)}
                </card>
            </view>
        </view>
    }
}

fn data_page(
    theme: &Theme,
    vertical_offset: u64,
    set_vertical_offset: &StateSetter<u64>,
    horizontal_offset: u64,
    set_horizontal_offset: &StateSetter<u64>,
) -> Node {
    let vertical_setter = set_vertical_offset.clone();
    let horizontal_setter = set_horizontal_offset.clone();
    let stream = ui! {
        <scroll_area
            axes={ScrollAxes::Both}
            offset={ScrollOffset::new(horizontal_offset as u32, vertical_offset as u32)}
            scrollbar_visibility={icmd::ScrollbarVisibility::Always}
            on_scroll={move |event: ScrollEvent| {
                vertical_setter.set(event.offset.y as u64);
                horizontal_setter.set(event.offset.x as u64);
            }}
            style={|style| {
                style.width /= Dimension::Max;
                style.height /= Dimension::Cells(13);
            }}
        >
            {(0..24).map(|index| ui! {
                <view style={|style| {
                    style.layout /= Layout::Horizontal;
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.justify /= Justify::SpaceBetween;
                    style.gap /= 0;
                }}>
                    {Text::new(format!("{:02}", index + 1))
                        .foreground(theme.colors.muted_foreground)}
                    {icmd::text(format!("commit/frame/{:04}", 9138 + index))}
                    <badge
                        text={if index % 3 == 0 { "queued" } else { "done" }}
                        variant={if index % 3 == 0 {
                            BadgeVariant::Accent
                        } else {
                            BadgeVariant::Secondary
                        }}
                    />
                </view>
            }).collect::<Node>()}
        </scroll_area>
    };
    let inspector = ui! {
        <card style={|style| style.width /= Dimension::Cells(27)}>
            <heading>"Viewport"</heading>
            <divider />
            <row>{format!("x:{horizontal_offset:02}")}<spacer />{format!("y:{vertical_offset:02}")}</row>
            <column><muted>"wheel"</muted><muted>"arrows / pgup / pgdn"</muted></column>
        </card>
    };
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <card style={|style| style.width /= Dimension::Max}>
                <heading>"Scrollable event stream"</heading>
                <muted>"The content scrolls; page chrome stays in place."</muted>
            </card>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {stream}
                {inspector}
            </view>
        </view>
    }
}

fn theme_page(
    theme: &Theme,
    dark: bool,
    preset: usize,
    set_dark: &StateSetter<bool>,
    set_preset: &StateSetter<usize>,
) -> Node {
    let presets = ui! {
        <row>{(0..ThemePreset::ALL.len())
            .map(|index| {
                let selected = index == preset;
                let foreground = if selected {
                    theme.colors.primary_foreground
                } else {
                    theme.colors.muted_foreground
                };
                let background = if selected {
                    theme.colors.primary
                } else {
                    theme.colors.muted
                };
                let setter = set_preset.clone();
                ui! {
                    <view
                        key={index as u64}
                        style={move |style| {
                            style.layout /= Layout::Horizontal;
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= background;
                            style.text.foreground /= foreground;
                            style.text.attr.bold /= selected;
                        }}
                        on_click={move |_| setter.set(index)}
                    >
                        {ThemePreset::ALL[index].name()}
                    </view>
                }
            })
            .collect::<Node>()}</row>
    };
    let palette = ui! {
        <row>
            <badge text="primary" variant={BadgeVariant::Primary} />
            <badge text="secondary" variant={BadgeVariant::Secondary} />
            <badge text="accent" variant={BadgeVariant::Accent} />
            <badge text="muted" variant={BadgeVariant::Muted} />
            <badge text="danger" variant={BadgeVariant::Destructive} />
        </row>
    };
    let mode_setter = set_dark.clone();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <card style={|style| style.width /= Dimension::Max}>
                <view style={|style| {
                    style.layout /= Layout::Horizontal;
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.justify /= Justify::SpaceBetween;
                    style.gap /= 0;
                }}>
                    <heading>"Theme laboratory"</heading>
                    <button on_click={move |_| mode_setter.set(!dark)}>
                        {if dark { "☾ dark" } else { "☀ light" }}
                    </button>
                </view>
                <paragraph>"Presets favor terminal contrast, compact borders and semantic color over large surfaces."</paragraph>
                <label>"Preset"</label>
                {presets}
            </card>
            <card style={|style| style.width /= Dimension::Max}>
                <heading>{format!(
                    "{} / {}",
                    ThemePreset::ALL[preset % ThemePreset::ALL.len()].name(),
                    if dark { "dark" } else { "light" }
                )}</heading>
                {palette}
                <divider />
                {Text::new("foreground").foreground(theme.colors.foreground)}
                {Text::new("muted foreground").foreground(theme.colors.muted_foreground)}
                <code>"let theme = cx.use_theme();"</code>
            </card>
            <alert
                title="Terminal-aware defaults"
                message="One-line actions, compact spacing, subtle surfaces, visible focus colors."
                variant={AlertVariant::Info}
            />
        </view>
    }
}

fn app(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (tab, set_tab) = cx.use_state(|| 0usize);
    let (dark, set_dark) = cx.use_state(|| true);
    let (preset, set_preset) = cx.use_state(|| 1usize);
    let (completed, set_completed) = cx.use_state(|| true);
    let (notifications, set_notifications) = cx.use_state(|| true);
    let (query, set_query) = cx.use_state(String::new);
    let (wrap, set_wrap) = cx.use_state(|| TextWrap::Hard);
    let (tick, set_tick) = cx.use_state(|| 0usize);
    let (vertical_offset, set_vertical_offset) = cx.use_state(|| 6_u64);
    let (horizontal_offset, set_horizontal_offset) = cx.use_state(|| 18_u64);

    cx.use_effect((), move || {
        let alive = Arc::new(AtomicBool::new(true));
        let worker_alive = alive.clone();
        let worker = thread::spawn(move || {
            while worker_alive.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(120));
                if worker_alive.load(Ordering::Relaxed) {
                    set_tick.update(|value| *value = value.wrapping_add(1));
                }
            }
        });
        move || {
            alive.store(false, Ordering::Relaxed);
            let _ = worker.join();
        }
    });

    let preset_index = preset % ThemePreset::ALL.len();
    let theme = ThemePreset::ALL[preset_index].theme(if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    });
    let keyboard = {
        let set_tab = set_tab.clone();
        let set_dark = set_dark.clone();
        let set_preset = set_preset.clone();
        move |event: KeyboardEvent| match event.key.code {
            KeyCode::Char(ch) if ('1'..='4').contains(&ch) => {
                set_tab.set(ch as usize - '1' as usize)
            }
            KeyCode::Left => set_tab.update(|value| *value = (*value + 3) % 4),
            KeyCode::Right | KeyCode::Tab => set_tab.update(|value| *value = (*value + 1) % 4),
            KeyCode::Char('t') => set_dark.update(|value| *value = !*value),
            KeyCode::Char('p') => {
                set_preset.update(|value| *value = (*value + 1) % ThemePreset::ALL.len())
            }
            _ => {}
        }
    };

    let brand = Text::from_spans([
        icmd::Span::new("◆ ICMD")
            .foreground(theme.colors.primary)
            .bold(),
        icmd::Span::new("  terminal component lab").style(theme.typography.muted.clone()),
    ]);
    let action_set_preset = set_preset.clone();
    let action_set_dark = set_dark.clone();
    let tabs_theme = theme.clone();
    let tabs_set_tab = set_tab.clone();
    let actions = ui! {
        <row>
            <button on_click={move |_| action_set_preset
                .update(|value| *value = (*value + 1) % ThemePreset::ALL.len())}>
                {ThemePreset::ALL[preset_index].name()}
            </button>
            <button on_click={move |_| action_set_dark.set(!dark)}>
                {if dark { "☾ dark" } else { "☀ light" }}
            </button>
        </row>
    };
    let tabs = ui! {
        <row>{(0..TABS.len())
            .map(|index| {
                let selected = index == tab;
                let background = if selected {
                    tabs_theme.colors.muted
                } else {
                    tabs_theme.colors.background
                };
                let foreground = if selected {
                    tabs_theme.colors.primary
                } else {
                    tabs_theme.colors.muted_foreground
                };
                let setter = tabs_set_tab.clone();
                ui! {
                    <view
                        key={index as u64}
                        style={move |style| {
                            style.layout /= Layout::Horizontal;
                            style.background /= background;
                            style.text.foreground /= foreground;
                            style.text.attr.bold /= selected;
                            style.text.attr.underlined /= selected;
                            style.padding /= Edges::symmetric(0, 1);
                            style.border.edges /= Edges::all(false);
                        }}
                        on_click={move |_| setter.set(index)}
                    >
                        {format!("{} {}", index + 1, TABS[index])}
                    </view>
                }
            })
            .collect::<Node>()}</row>
    };
    let page = match tab {
        0 => overview(&theme, tick),
        1 => components_page(
            completed,
            &set_completed,
            notifications,
            &set_notifications,
            query,
            &set_query,
            wrap,
            &set_wrap,
        ),
        2 => data_page(
            &theme,
            vertical_offset,
            &set_vertical_offset,
            horizontal_offset,
            &set_horizontal_offset,
        ),
        _ => theme_page(&theme, dark, preset_index, &set_dark, &set_preset),
    };
    let body = ui! {
        <scroll_area axes={ScrollAxes::Vertical} style={|style| {
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
        }}>{page}</scroll_area>
    };
    let status = ui! {
        <view style={|style| {
            style.layout /= Layout::Horizontal;
            style.width /= Dimension::Max;
            style.align /= Align::Center;
            style.justify /= Justify::SpaceBetween;
            style.gap /= 0;
        }}>
            <row>
                <kbd>"1–4"</kbd><muted>"tabs"</muted>
                <kbd>"T"</kbd><muted>"theme"</muted>
                <kbd>"P"</kbd><muted>"preset"</muted>
            </row>
            <muted>"mouse enabled  •  Ctrl+C quits"</muted>
        </view>
    };
    let background = theme.colors.background;
    let foreground = theme.colors.foreground;
    let shell = ui! {
        <view
            on_keyboard_event={keyboard}
            style={move |style| {
            style.width /= Dimension::Percent(Percent::viewport(100));
            style.height /= Dimension::Percent(Percent::viewport(100));
            style.layout /= Layout::Vertical;
            style.padding /= Edges::all(1);
            style.gap /= 1;
            style.background /= background;
            style.text.foreground /= foreground;
        }}>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 0;
            }}>
                {brand}
                {actions}
            </view>
            {tabs}
            {body}
            {status}
        </view>
    };
    ui! {
        <theme_provider value={theme}>
            {shell}
        </theme_provider>
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    render(
        app.apply(()),
        RuntimeConfig {
            emoji_merging: EmojiMerging::Auto,
            ..Default::default()
        },
    )?;
    Ok(())
}
