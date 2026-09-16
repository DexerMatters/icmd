#![recursion_limit = "512"]

//! A component lab: every widget the framework ships, grouped by intent, with a
//! live theme laboratory on top.
//!
//! The example is organized as a shell (header, sidebar, status bar) around six
//! pages. Each page is a gallery: controls are genuinely wired to state, the
//! selectable-text page drives a real selection region, and the theme page
//! recolors the whole lab in place.
//!
//! Keys (application-global, so they work before anything is focused):
//!   1-6 / Left / Right  switch page
//!   t                   toggle light/dark
//!   p                   next theme preset
//!   l                   toggle live animation
//!   Ctrl+Shift+C        copy the selection
//!   Ctrl+Shift+V        paste into a focused editor
//!   Ctrl+C              quit (the runtime's exit key; widgets never claim it)

use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossterm::{event::KeyCode, style::Color};
use icmd::{
    AlertVariant, Align, AppLifecycle, Attr, BadgeVariant, BorderKind, Component, ComponentContext,
    Dimension, Edges, EventListener, ExitReason, Justify, KeyboardEvent, Layout, Node, Percent,
    Props, RawInputMode, RawInputProps, RuntimeConfig, ScrollAxes, ScrollEvent, ScrollOffset,
    ScrollbarVisibility, Span, StateSetter, Text, TextAlign, TextClipboardEvent, TextOverflow,
    TextSelectionEvent, TextValueEvent, TextWrap, raw_input, render_with, theme_provider, ui,
};
use icmd::{
    EmojiMerging,
    theme::{Theme, ThemeMode, ThemePreset},
};
use icmd::{
    alert, badge, blockquote, button, canvas, card, center, checkbox, code, column, container,
    divider, empty, footer, heading, input, kbd, label, link, muted, paragraph, progress_bar,
    radio, row, scroll_area, section, selection_area, skeleton, spacer, spinner, switch, textarea,
    view,
};

// Page titles paired with the sentence shown under them.
const PAGES: [(&str, &str); 6] = [
    ("Overview", "What the framework gives an application"),
    ("Controls", "Focus, activation and clipboard-aware widgets"),
    ("Typography", "Text, spans, wrapping and overflow"),
    ("Selection", "Read-only regions whose text is selectable"),
    ("Layout", "Boxes, scrolling, sizing and canvas"),
    ("Theme", "Palettes, tokens and nested providers"),
];

// ---------------------------------------------------------------------------
// Small composing helpers. Each one is ordinary `ui!` code, so they double as
// examples of building your own vocabulary on top of the framework.
// ---------------------------------------------------------------------------

/// A key hint: the key in a `kbd` chip, the action in muted text.
fn hint(key: &str, action: &str) -> Node {
    ui! {
        <row>
            <kbd>{key.to_string()}</kbd>
            <muted>{action.to_string()}</muted>
        </row>
    }
}

/// A card with a title, an optional muted subtitle, and content.
fn panel(title: &str, subtitle: &str, body: Node) -> Node {
    let subtitle = subtitle.to_string();
    ui! {
        <card style={|style| style.width /= Dimension::Max}>
            <heading>{title.to_string()}</heading>
            {if subtitle.is_empty() {
                empty()
            } else {
                ui! { <muted>{Text::new(subtitle).wrap(TextWrap::Soft)}</muted> }
            }}
            {body}
        </card>
    }
}

/// A metric tile: a big colored value over a muted caption.
fn stat(value: String, caption: &str, accent: Color, theme: &Theme) -> Node {
    ui! {
        <card style={|style| style.width /= Dimension::Max}>
            {Text::new(value).foreground(accent).bold()}
            {Text::new(caption.to_string()).text_style(theme.typography.muted.clone())}
        </card>
    }
}

/// A section heading used inside a page.
fn section_title(title: &str, subtitle: &str) -> Node {
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 0;
            }}>
                <label>{title.to_string()}</label>
                <muted>{Text::new(subtitle.to_string()).wrap(TextWrap::Soft)}</muted>
            </view>
            <divider />
        </view>
    }
}

/// A colored chip naming one resolved palette token.
fn swatch(name: &str, background: Color, foreground: Color) -> Node {
    ui! {
        <view style={move |style| {
            style.padding /= Edges::symmetric(0, 1);
            style.background /= background;
            style.text.foreground /= foreground;
        }}>
            {format!(" {name} ")}
        </view>
    }
}

/// Chunk swatches into fixed-width rows so a palette stays readable.
fn swatch_rows(entries: &[(&str, Color, Color)]) -> Node {
    entries
        .chunks(3)
        .map(|chunk| {
            let chips = chunk
                .iter()
                .map(|(name, background, foreground)| swatch(name, *background, *foreground))
                .collect::<Node>();
            ui! {
                <row>
                    {chips}
                    <spacer />
                </row>
            }
        })
        .collect::<Node>()
}

/// One sidebar entry. The selected row keeps the accent color and a filled
/// background, so the current page is legible at a glance.
fn nav_item(
    index: usize,
    title: &str,
    selected: bool,
    theme: &Theme,
    set_page: &StateSetter<usize>,
) -> Node {
    let (background, foreground) = if selected {
        (theme.colors.primary, theme.colors.primary_foreground)
    } else {
        (theme.colors.background, theme.colors.foreground)
    };
    let setter = set_page.clone();
    ui! {
        <view
            key={index as u64}
            on_click={move |_| setter.set(index)}
            style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
                style.background /= background;
                style.text.foreground /= foreground;
                style.text.attr.bold /= selected;
            }}
        >
            <row>
                <muted>{format!("{}", index + 1)}</muted>
                {Text::new(title.to_string())}
            </row>
        </view>
    }
}

// ---------------------------------------------------------------------------
// A user-defined component: a styled wrapper over `raw_input`.
//
// It shows the intended extension path - choose the policy, forward the caller's
// value and observer, and restyle the host - without reimplementing editing.
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct CommandFieldProps {
    pub value: Attr<String>,
    pub on_change: Attr<EventListener<TextValueEvent>>,
}

pub fn command_field(cx: &mut ComponentContext, props: &Props<CommandFieldProps>) -> Node {
    let _ = cx;
    raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::SingleLine),
            value: props.value.clone(),
            placeholder: Attr::Set("run --all".into()),
            max_length: Attr::Set(48),
            on_change: props.on_change.clone(),
            ..RawInputProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(30);
            style.border.foreground /= Color::Yellow;
        })
        .node()
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

fn overview(theme: &Theme, tick: usize) -> Node {
    let progress_value = 38 + (tick as u64 * 3) % 60;
    let canvas_background = theme.colors.card;
    let grid = theme.colors.border;
    let accent = theme.colors.primary;
    let secondary = theme.colors.secondary;

    let hero = ui! {
        <card style={|style| style.width /= Dimension::Max}>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 0;
            }}>
                <heading>"Terminal UI, composed like a web page"</heading>
                <row>
                    <badge text="v0.1" variant={BadgeVariant::Secondary} />
                    <badge text="LIVE" variant={BadgeVariant::Accent} />
                </row>
            </view>
            <paragraph>{Text::new(
                "Retained components, a cell-aware layout engine and incremental ANSI \
                 updates: build dense terminal tools out of the same primitives a web \
                 page uses - boxes, text, focus and events.",
            ).wrap(TextWrap::Soft)}</paragraph>
            <center>{Text::new("declarative  •  Unicode-aware  •  transactional")
                .foreground(theme.colors.accent)}</center>
        </card>
    };

    let tiles = ui! {
        <view style={|style| {
            style.layout /= Layout::Horizontal;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {stat("24".into(), "mounted nodes", theme.colors.primary, theme)}
            {stat("99.9%".into(), "render uptime", theme.colors.secondary, theme)}
            {stat(format!("{}", tick % 1000), "frames presented", theme.colors.accent, theme)}
        </view>
    };

    let runtime = panel(
        "Live runtime",
        "Damage-tracked frames",
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <progress_bar value={progress_value} max={100} width={26} label="commit" />
                <progress_bar value={progress_value / 2} max={100} width={26} label="raster" />
                <spinner frame={tick} label="streaming diff frames" />
                <row>
                    <skeleton width={10} />
                    <muted>"placeholder shimmer"</muted>
                </row>
            </view>
        },
    );

    let activity = panel(
        "Renderer activity",
        "One callback, one cell image",
        ui! {
            // Bars are at most five rows tall from a baseline at row 6, so row
            // 0 stays free for the caption.
            <canvas width={38} height={7} draw={Arc::new(move |drawing| {
                drawing.set_background(canvas_background);
                drawing.clear().unwrap();
                drawing.set_foreground(grid);
                for row in 1..7 {
                    drawing.line(0, row, 37, row, "·").unwrap();
                }
                // A bar chart whose heights follow the live tick.
                drawing.set_foreground(accent);
                for column in 0..12 {
                    let height = 1 + (column * 3 + (tick % 7)) % 5;
                    drawing
                        .fill_rect(
                            (2 + column * 3) as i32,
                            7 - height as i32,
                            1,
                            height as u16,
                            "█",
                        )
                        .unwrap();
                }
                drawing.set_foreground(secondary);
                drawing.line(2, 2, 12, 0, "•").unwrap();
                drawing.line(12, 0, 24, 3, "•").unwrap();
                drawing.line(24, 3, 36, 1, "•").unwrap();
                drawing.set_foreground(secondary);
                drawing.fill_text("fps", 1, 0).unwrap();
            })} />
        },
    );

    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {hero}
            {tiles}
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <view style={|style| style.width /= Dimension::Max}>
                    {runtime}
                </view>
                <view style={|style| style.width /= Dimension::Max}>
                    {activity}
                </view>
            </view>
            <alert
                title="Everything on this page is a component"
                message="Cards, headings, badges, progress bars, spinners, skeletons, alerts and a drawing canvas - composed with row, column, center and spacer."
                variant={AlertVariant::Success}
            />
            <alert
                title="Keyboard first"
                message="Press 1-6 to switch pages, t for light/dark, p for the next preset, l to freeze the animation."
                variant={AlertVariant::Info}
            />
        </view>
    }
}

/// The state a gallery page renders from.
///
/// Grouping it keeps each page's signature small and shows the intended flow:
/// the application owns state, a page receives a view of it and raises changes
/// back through the setters.
struct ControlState<'a> {
    completed: bool,
    set_completed: &'a StateSetter<bool>,
    notifications: bool,
    set_notifications: &'a StateSetter<bool>,
    plan: usize,
    set_plan: &'a StateSetter<usize>,
    query: String,
    set_query: &'a StateSetter<String>,
    draft: String,
    set_draft: &'a StateSetter<String>,
    wrap: TextWrap,
    set_wrap: &'a StateSetter<TextWrap>,
    followed: String,
    set_followed: &'a StateSetter<String>,
}

fn controls_page(theme: &Theme, state: ControlState<'_>) -> Node {
    let ControlState {
        completed,
        set_completed,
        notifications,
        set_notifications,
        plan,
        set_plan,
        query,
        set_query,
        draft,
        set_draft,
        wrap,
        set_wrap,
        followed,
        set_followed,
    } = state;
    let check_setter = set_completed.clone();
    let toggle_setter = set_notifications.clone();
    let query_setter = set_query.clone();
    let draft_setter = set_draft.clone();
    let followed_setter = set_followed.clone();
    let wrap_setter = set_wrap.clone();
    let next_wrap = match wrap {
        TextWrap::NoWrap => TextWrap::Soft,
        TextWrap::Soft => TextWrap::Hard,
        TextWrap::Hard => TextWrap::NoWrap,
    };

    let plans = ["Incremental", "Full redraw", "Raster only"];
    // `ui!` moves the values it captures into the closures it builds, so
    // anything echoed on the page is formatted before a panel consumes it.
    let query_echo = format!("{query:?}");
    let draft_echo = format!("{draft:?}");
    let plan_echo = plans[plan.min(plans.len() - 1)].to_string();
    let wrap_echo = format!("{wrap:?}");
    let radios = plans
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let setter = set_plan.clone();
            ui! {
                <radio
                    selected={index == plan}
                    label={name.to_string()}
                    on_select={move |_| setter.set(index)}
                />
            }
        })
        .collect::<Node>();

    let editors = panel(
        "Text editors",
        "Controlled, uncontrolled, wrapped and read-only",
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <label>"Controlled input"</label>
                <input
                    value={query.clone()}
                    placeholder="filter widgets…"
                    max_length={32}
                    on_change={move |event: TextValueEvent| query_setter.set(event.value)}
                />
                <label>"Custom wrapper over raw_input"</label>
                <command_field
                    value={draft.clone()}
                    on_change={move |event: TextValueEvent| draft_setter.set(event.value)}
                />
                <label>"Textarea, wrap policy is caller state"</label>
                <textarea
                    wrap={wrap}
                    placeholder="Long Unicode text lives here…"
                    style={|style| {
                        style.width /= Dimension::Cells(34);
                        style.height /= Dimension::Cells(7);
                    }}
                    default_value={"这是一个很长的示例文本，含有 emoji 👩‍💻 and an unbreakable-token-for-hard-wrap."}
                />
                <row>
                    <button on_press={move |_| wrap_setter.set(next_wrap)}>
                        {format!("wrap: {:?}", wrap)}
                    </button>
                    <button variant={icmd::ButtonVariant::Secondary} on_press={move |_| {}}>
                        "Secondary"
                    </button>
                    <button variant={icmd::ButtonVariant::Destructive} on_press={move |_| {}}>
                        "Destructive"
                    </button>
                </row>
                <label>"Read-only: selection and copy still work"</label>
                <input
                    read_only
                    default_value={"locked value".to_string()}
                    on_clipboard={move |event: TextClipboardEvent| {
                        let _ = event.text;
                    }}
                />
            </view>
        },
    );

    let controls = panel(
        "Controls",
        "Controlled widgets report the state they propose",
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <checkbox
                    checked={completed}
                    label="Show completed"
                    on_change={move |next: bool| check_setter.set(next)}
                />
                <checkbox disabled label="Disabled checkbox" />
                <switch
                    on={notifications}
                    label="Notifications"
                    on_change={move |next: bool| toggle_setter.set(next)}
                />
                <label>"Redraw plan"</label>
                {radios}
                <row>
                    <kbd>"Enter"</kbd>
                    <muted>"activates the focused control"</muted>
                </row>
            </view>
        },
    );

    let state = panel(
        "Live state",
        "The page renders from the values below",
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
            }}>
                <row><muted>"query"</muted><spacer />{query_echo}</row>
                <row><muted>"draft"</muted><spacer />{draft_echo}</row>
                <row><muted>"completed"</muted><spacer />{format!("{completed}")}</row>
                <row><muted>"notifications"</muted><spacer />{format!("{notifications}")}</row>
                <row><muted>"plan"</muted><spacer />{plan_echo}</row>
                <row><muted>"wrap"</muted><spacer />{wrap_echo}</row>
                <divider />
                <label>"Semantic activation"</label>
                <row>
                    <muted>"on_change / on_select / on_press / on_follow"</muted>
                <muted>"Copy with Shift+Ctrl+C, paste with Shift+Ctrl+V."</muted>
                </row>
                <link
                    href="https://github.com/example/icmd"
                    on_follow={move |target: String| followed_setter.set(target)}
                >
                    "Follow a link"
                </link>
                <muted>{if followed.is_empty() {
                    "Click the link, or focus it and press Enter".to_string()
                } else {
                    format!("followed: {followed}")
                }}</muted>
            </view>
        },
    );

    let _ = theme;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {section_title("Controls", "Every widget keeps its own policy and reports intent")}
            {editors}
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <view style={|style| style.width /= Dimension::Max}>
                    {controls}
                </view>
                <view style={|style| style.width /= Dimension::Max}>
                    {state}
                </view>
            </view>
        </view>
    }
}

fn typography_page(theme: &Theme, wrap: TextWrap) -> Node {
    let attributes = Text::from_spans([
        Span::new("bold ").bold(),
        Span::new("italic ").italic(),
        Span::new("underline ").underlined(),
        Span::new("dim ").dim(),
    ]);
    let attributes_more = Text::from_spans([
        Span::new("reverse ").reverse(),
        Span::new("strike ").crossed_out(),
        Span::new("accent ").foreground(theme.colors.accent),
        Span::new("primary").foreground(theme.colors.primary).bold(),
    ]);

    let aligned = ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::symmetric(0, 1);
            style.background /= theme.colors.muted;
            style.text.foreground /= theme.colors.muted_foreground;
        }}>
            {Text::new("start-aligned line").align(TextAlign::Start)}
            {Text::new("centered line").align(TextAlign::Center)}
            {Text::new("end-aligned line").align(TextAlign::End)}
        </view>
    };

    let clipped = ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>
            {Text::new("Clipped: the quick brown fox jumps over the lazy dog")
                .layout_style(icmd::style(|style| style.width /= Dimension::Cells(26)))
                .overflow(TextOverflow::Clip)}
            {Text::new("Ellipsis: the quick brown fox jumps over the lazy dog")
                .layout_style(icmd::style(|style| style.width /= Dimension::Cells(26)))
                .overflow(TextOverflow::Ellipsis)}
        </view>
    };

    let wrapping = ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Cells(34);
            style.gap /= 0;
            style.padding /= Edges::symmetric(0, 1);
            style.border.kind /= theme.borders.kind;
            style.border.foreground /= theme.colors.border;
            style.border.background /= theme.colors.card;
            style.background /= theme.colors.card;
            style.text.foreground /= theme.colors.card_foreground;
        }}>
            {Text::new(format!(
                "wrap={wrap:?}: wrapping breaks on terminal cells, not bytes - 这是一个很长的示例文本 with an unbreakable-token-for-hard-wrap."
            ))
            .wrap(wrap)}
        </view>
    };

    let specimens = panel(
        "Semantic styles",
        "The theme supplies every text role",
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
            }}>
                <heading>"Heading"</heading>
                <label>"Label"</label>
                <paragraph>"Body copy wraps on cell boundaries."</paragraph>
                <muted>"Muted caption"</muted>
                <code>"inline code"</code>
                <blockquote>"Composition beats configuration."</blockquote>
                <row><kbd>"K"</kbd><muted>"kbd chip"</muted></row>
                {attributes}
                {attributes_more}
            </view>
        },
    );

    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {section_title("Typography", "Text is a first-class element, not a string")}
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <view style={|style| style.width /= Dimension::Max}>
                    {specimens}
                </view>
                <view style={|style| style.width /= Dimension::Max}>
                    {panel(
                        "Alignment, overflow and wrapping",
                        "Cells decide; alignment is a layout concern",
                        ui! {
                            <view style={|style| {
                                style.layout /= Layout::Vertical;
                                style.width /= Dimension::Max;
                                style.gap /= 1;
                            }}>
                                {aligned}
                                {clipped}
                                {wrapping}
                                <muted>"Press the wrap button on the Controls page to change the policy."</muted>
                            </view>
                        },
                    )}
                </view>
            </view>
        </view>
    }
}

/// The selection page's state.
struct SelectionState<'a> {
    selection: String,
    set_selection: &'a StateSetter<String>,
    copied: String,
    set_copied: &'a StateSetter<String>,
    keyboard: bool,
    set_keyboard: &'a StateSetter<bool>,
    region_disabled: bool,
    set_region_disabled: &'a StateSetter<bool>,
    tick: usize,
}

fn selection_page(theme: &Theme, state: SelectionState<'_>) -> Node {
    let SelectionState {
        selection,
        set_selection,
        copied,
        set_copied,
        keyboard,
        set_keyboard,
        region_disabled,
        set_region_disabled,
        tick,
    } = state;
    let selection_setter = set_selection.clone();
    let clipboard_setter = set_copied.clone();
    let keyboard_setter = set_keyboard.clone();
    let disabled_setter = set_region_disabled.clone();

    // The region makes every text leaf below it selectable: headings, paragraphs,
    // code blocks and even text inside a nested scroll area.
    let article = ui! {
        <selection_area
            disabled={region_disabled}
            enable_keyboard={keyboard}
            on_selection_change={move |event: TextSelectionEvent| selection_setter.set(format!(
                "{}..{} ({} chars)",
                event.range.start,
                event.range.end,
                event.text.chars().count()
            ))}
            on_clipboard={move |event: TextClipboardEvent| clipboard_setter.set(event.text)}
        >
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <heading>"Release notes"</heading>
                <paragraph>{Text::new(
                    "Drag across this article: the selection spans every text leaf inside \
                     the region, and Ctrl+Shift+C copies it with a newline between blocks.",
                ).wrap(TextWrap::Soft)}</paragraph>
                <code>"cargo run --example demo"</code>
                <blockquote>"A region adds selection to text the framework already renders."</blockquote>
                <muted>"Shift+click or Shift+arrows extend; Ctrl+A selects; Ctrl+Shift+C copies."</muted>
            </view>
        </selection_area>
    };

    let log = (0..18)
        .map(|index| {
            let style = if index % 3 == 0 {
                theme.typography.muted.clone()
            } else {
                theme.typography.body.clone()
            };
            ui! {
                <row>
                    {Text::new(format!("{:02}", index + 1)).text_style(theme.typography.muted.clone())}
                    {Text::new(format!("commit/frame/{:04}", 9138 + index + tick % 13))
                        .text_style(style)}
                </row>
            }
        })
        .collect::<Node>();

    let readouts = panel(
        "Selection state",
        "Reported by the region through ordinary listeners",
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
            }}>
                <row><muted>"range"</muted><spacer />{if selection.is_empty() {
                    "—".to_string()
                } else {
                    selection.clone()
                }}</row>
                <row><muted>"copied"</muted><spacer />{if copied.is_empty() {
                    "press Ctrl+Shift+C with a selection".to_string()
                } else {
                    format!("{:?}", copied)
                }}</row>
                <divider />
                <switch
                    on={keyboard}
                    label="Keyboard selection"
                    on_change={move |next: bool| keyboard_setter.set(next)}
                />
                <switch
                    on={region_disabled}
                    label="Disable the region"
                    on_change={move |next: bool| disabled_setter.set(next)}
                />
                <muted>"A disabled region takes no focus and selects nothing."</muted>
            </view>
        },
    );

    let comparison = panel(
        "Editors versus regions",
        "The same engine drives both",
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <label>"Read-only textarea"</label>
                <textarea
                    read_only
                    default_value={"Select me too: a read-only editor keeps the caret, word motion and the clipboard contract.".to_string()}
                    style={|style| {
                        style.width /= Dimension::Cells(40);
                        style.height /= Dimension::Cells(4);
                    }}
                />
                <muted>{Text::new("Editors expose an insertion point; a region exposes only a selection.")
                    .wrap(TextWrap::Soft)}</muted>
            </view>
        },
    );

    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {section_title("Selectable text", "Point, drag and copy - no editing surface required")}
            {article}
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <view style={|style| style.width /= Dimension::Max}>
                    {readouts}
                </view>
                <view style={|style| style.width /= Dimension::Max}>
                    {comparison}
                </view>
            </view>
            {panel(
                "A region over a scroll area",
                "Painted runs are recorded after scrolling, so hit testing follows",
                ui! {
                    <scroll_area
                        axes={ScrollAxes::Vertical}
                        scrollbar_visibility={ScrollbarVisibility::Auto}
                        style={|style| {
                            style.width /= Dimension::Max;
                            style.height /= Dimension::Cells(9);
                        }}
                    >
                        <selection_area enable_keyboard={keyboard}>
                            <view style={|style| {
                                style.layout /= Layout::Vertical;
                                style.width /= Dimension::Max;
                                style.gap /= 0;
                            }}>{log}</view>
                        </selection_area>
                    </scroll_area>
                },
            )}
        </view>
    }
}

/// The layout page's state.
struct LayoutState<'a> {
    vertical_offset: u64,
    set_vertical_offset: &'a StateSetter<u64>,
    horizontal_offset: u64,
    set_horizontal_offset: &'a StateSetter<u64>,
    bars: bool,
    set_bars: &'a StateSetter<bool>,
    tick: usize,
}

fn layout_page(theme: &Theme, state: LayoutState<'_>) -> Node {
    let LayoutState {
        vertical_offset,
        set_vertical_offset,
        horizontal_offset,
        set_horizontal_offset,
        bars,
        set_bars,
        tick,
    } = state;
    let vertical_setter = set_vertical_offset.clone();
    let horizontal_setter = set_horizontal_offset.clone();
    let bar_setter = set_bars.clone();

    let stream = (0..30)
        .map(|index| {
            let queued = index % 3 == 0;
            ui! {
                <view style={|style| {
                    style.layout /= Layout::Horizontal;
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.justify /= Justify::SpaceBetween;
                    style.gap /= 1;
                }}>
                    {Text::new(format!("{:02}", index + 1))
                        .foreground(theme.colors.muted_foreground)}
                    {icmd::text(format!("commit/frame/{:04}", 9138 + index))}
                    <badge
                        text={if queued { "queued" } else { "done" }}
                        variant={if queued { BadgeVariant::Accent } else { BadgeVariant::Secondary }}
                    />
                </view>
            }
        })
        .collect::<Node>();

    let data = panel(
        "Scrollable data",
        "Two axes, one scroll build, page chrome untouched",
        ui! {
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <scroll_area
                    axes={ScrollAxes::Both}
                    offset={ScrollOffset::new(horizontal_offset as u32, vertical_offset as u32)}
                    scrollbar_visibility={ScrollbarVisibility::Always}
                    on_scroll={move |event: ScrollEvent| {
                        vertical_setter.set(event.offset.y as u64);
                        horizontal_setter.set(event.offset.x as u64);
                    }}
                    style={|style| {
                        style.width /= Dimension::Max;
                        style.height /= Dimension::Cells(12);
                    }}
                >
                    {stream}
                </scroll_area>
                <card style={|style| style.width /= Dimension::Cells(24)}>
                    <heading>"Viewport"</heading>
                    <divider />
                    <row><muted>"x"</muted><spacer />{format!("{horizontal_offset:02}")}</row>
                    <row><muted>"y"</muted><spacer />{format!("{vertical_offset:02}")}</row>
                    <muted>"wheel, arrows, pgup/pgdn"</muted>
                </card>
            </view>
        },
    );

    let boxes = panel(
        "Boxes",
        "container, section, card, row, column, center, spacer",
        ui! {
            <column style={|style| {
                style.width /= Dimension::Max;
                style.gap /= 0;
            }}>
                <container style={|style| {
                    style.height /= Dimension::Cells(3);
                    style.padding /= Edges::symmetric(0, 1);
                    style.background /= theme.colors.card;
                    style.text.foreground /= theme.colors.card_foreground;
                }}>
                    <row>
                        <view style={|style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= theme.colors.primary;
                            style.text.foreground /= theme.colors.primary_foreground;
                        }}>"primary"</view>
                        <view style={|style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= theme.colors.secondary;
                            style.text.foreground /= theme.colors.secondary_foreground;
                        }}>"secondary"</view>
                        <spacer />
                        <view style={|style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.border.kind /= theme.borders.kind;
                            style.border.foreground /= theme.colors.border;
                        }}>"bordered"</view>
                    </row>
                </container>
                <center>
                    <muted>"centered inside a full-width row"</muted>
                </center>
                <footer>
                    <muted>"footer"</muted>
                    <spacer />
                    <kbd>"q"</kbd>
                    <muted>"quit"</muted>
                </footer>
            </column>
        },
    );

    let sizing = panel(
        "Sizing",
        "Cells, percent of the free space, or everything left",
        ui! {
            <row>
                <view style={|style| {
                    style.width /= Dimension::Cells(10);
                    style.background /= theme.colors.muted;
                    style.text.foreground /= theme.colors.muted_foreground;
                }}>"10 cells"</view>
                <view style={|style| {
                    style.width /= Dimension::Percent(Percent::available(40));
                    style.background /= theme.colors.accent;
                    style.text.foreground /= theme.colors.accent_foreground;
                }}>"40%"</view>
                <view style={|style| {
                    style.width /= Dimension::Max;
                    style.background /= theme.colors.primary;
                    style.text.foreground /= theme.colors.primary_foreground;
                }}>"max"</view>
            </row>
        },
    );

    let bars_now = bars;
    let canvas_background = theme.colors.card;
    let canvas_grid = theme.colors.border;
    let canvas_accent = theme.colors.primary;
    let canvas_secondary = theme.colors.secondary;
    let chart = ui! {
        // Rows: 0 and 8 are the frame, 1 is the caption, 2..=7 are the plot.
        // The plot stops one row above the frame so a bar can never overwrite
        // the border it is sitting on.
        <canvas width={40} height={9} draw={Arc::new(move |drawing| {
            drawing.set_background(canvas_background);
            drawing.clear().unwrap();
            drawing.set_foreground(canvas_grid);
            drawing.stroke_rect(0, 0, 40, 9).unwrap();
            drawing.set_foreground(canvas_secondary);
            drawing.fill_text("frame delta", 2, 1).unwrap();
            if bars_now {
                drawing.set_foreground(canvas_accent);
                for column in 0..13 {
                    // 1..=6 rows tall, growing up from the baseline at row 7.
                    let height = 1 + (column * 5 + (tick % 9)) % 6;
                    drawing
                        .fill_rect(
                            (2 + column * 3) as i32,
                            8 - height as i32,
                            1,
                            height as u16,
                            "█",
                        )
                        .unwrap();
                }
            } else {
                drawing.set_foreground(canvas_secondary);
                let mut previous = 5i32;
                for column in 0..38 {
                    let level = (2 + (column * 7 + (tick % 11)) % 6) as i32;
                    drawing
                        .line(column as i32 + 1, previous, column as i32 + 2, level, "•")
                        .unwrap();
                    previous = level;
                }
            }
        })} />
    };

    let drawing = panel(
        "Canvas",
        "Imperative drawing inside a declarative tree",
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {chart}
                <row>
                    <button on_press={move |_| bar_setter.set(!bars)}>
                        {if bars { "bars" } else { "sparkline" }}
                    </button>
                    <muted>"one callback, redrawn every frame"</muted>
                </row>
            </view>
        },
    );

    ui! {
        <section>
            {section_title("Layout and data", "Boxes compose; scrolling is a container concern")}
            {boxes}
            {sizing}
            {data}
            {drawing}
        </section>
    }
}

fn theme_page(
    theme: &Theme,
    dark: bool,
    preset: usize,
    set_dark: &StateSetter<bool>,
    set_preset: &StateSetter<usize>,
) -> Node {
    let presets = (0..ThemePreset::ALL.len())
        .map(|index| {
            let selected = index == preset;
            let background = if selected {
                theme.colors.primary
            } else {
                theme.colors.muted
            };
            let foreground = if selected {
                theme.colors.primary_foreground
            } else {
                theme.colors.muted_foreground
            };
            let setter = set_preset.clone();
            ui! {
                <view
                    key={index as u64}
                    on_click={move |_| setter.set(index)}
                    style={move |style| {
                        style.padding /= Edges::symmetric(0, 1);
                        style.background /= background;
                        style.text.foreground /= foreground;
                        style.text.attr.bold /= selected;
                    }}
                >
                    {ThemePreset::ALL[index].name().to_string()}
                </view>
            }
        })
        .collect::<Node>();

    let palette = [
        (
            "background",
            theme.colors.background,
            theme.colors.foreground,
        ),
        ("card", theme.colors.card, theme.colors.card_foreground),
        (
            "popover",
            theme.colors.popover,
            theme.colors.popover_foreground,
        ),
        (
            "primary",
            theme.colors.primary,
            theme.colors.primary_foreground,
        ),
        (
            "secondary",
            theme.colors.secondary,
            theme.colors.secondary_foreground,
        ),
        ("muted", theme.colors.muted, theme.colors.muted_foreground),
        (
            "accent",
            theme.colors.accent,
            theme.colors.accent_foreground,
        ),
        (
            "destructive",
            theme.colors.destructive,
            theme.colors.destructive_foreground,
        ),
        ("border", theme.colors.border, theme.colors.card_foreground),
        ("input", theme.colors.input, theme.colors.foreground),
        ("ring", theme.colors.ring, theme.colors.background),
    ];

    let border_card = |kind: BorderKind| {
        ui! {
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Cells(16);
                style.height /= Dimension::Cells(3);
                style.border.kind /= kind;
                style.border.edges /= Edges::all(true);
                style.border.foreground /= theme.colors.border;
                style.border.background /= theme.colors.card;
                style.background /= theme.colors.card;
                style.text.foreground /= theme.colors.card_foreground;
                style.padding /= Edges::symmetric(0, 1);
            }}>
                <label>{format!("{kind:?}")}</label>
                <muted>"border.kind"</muted>
            </view>
        }
    };
    let borders = [
        [BorderKind::Single, BorderKind::Rounded],
        [BorderKind::Double, BorderKind::Heavy],
    ]
    .iter()
    .map(|pair| {
        let cards = pair.iter().map(|kind| border_card(*kind)).collect::<Node>();
        ui! {
            <row>
                {cards}
                <spacer />
            </row>
        }
    })
    .collect::<Node>();

    let spacing = [
        ("xs", theme.spacing.xs),
        ("sm", theme.spacing.sm),
        ("md", theme.spacing.md),
        ("lg", theme.spacing.lg),
        ("xl", theme.spacing.xl),
    ]
    .iter()
    .map(|(name, value)| {
        let name = *name;
        let value = *value;
        ui! {
            <row>
                <view style={move |style| style.width /= Dimension::Cells(3)}>
                    <muted>{name.to_string()}</muted>
                </view>
                <view style={move |style| {
                    style.width /= Dimension::Cells(value.max(1));
                    style.background /= theme.colors.accent;
                }}>" "</view>
                <muted>{format!(" {value}")}</muted>
            </row>
        }
    })
    .collect::<Node>();

    // A theme is a plain value: derive or adjust one, then provide it to a
    // subtree. Providers nest, so this preview recolors only itself.
    let custom = ThemePreset::Nord.theme(ThemeMode::Dark).customize(|theme| {
        theme.borders.kind = BorderKind::Heavy;
        theme.colors.primary = Color::Magenta;
        theme.colors.accent = Color::Cyan;
        theme.spacing.sm = 1;
    });
    let nested = ui! {
        <theme_provider value={custom}>
            <card style={|style| style.width /= Dimension::Max}>
                <heading>"Nested provider"</heading>
                <muted>"Nord, Heavy borders, magenta primary - scoped to this card"</muted>
                <row>
                    <button on_press={move |_| {}}>"Action"</button>
                    <badge text="accent" variant={BadgeVariant::Accent} />
                    <kbd>"K"</kbd>
                </row>
                <progress_bar value={72} max={100} width={20} label="derived" />
            </card>
        </theme_provider>
    };

    let mode_setter = set_dark.clone();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {section_title("Theme", "Nineteen resolved tokens, eight presets")}
            <card style={|style| style.width /= Dimension::Max}>
                <view style={|style| {
                    style.layout /= Layout::Horizontal;
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.justify /= Justify::SpaceBetween;
                    style.gap /= 0;
                }}>
                    <heading>{format!(
                        "{} / {}",
                        ThemePreset::ALL[preset].name(),
                        if dark { "dark" } else { "light" }
                    )}</heading>
                    <button on_press={move |_| mode_setter.set(!dark)}>
                        {if dark { "☾ dark" } else { "☀ light" }}
                    </button>
                </view>
                <label>"Preset"</label>
                <row>
                    {presets}
                    <spacer />
                </row>
                <muted>"Press p to cycle presets, t to switch mode."</muted>
            </card>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <view style={|style| style.width /= Dimension::Max}>
                    {panel(
                        "Palette",
                        "Every role the components read",
                        ui! {
                            <view style={|style| {
                                style.layout /= Layout::Vertical;
                                style.width /= Dimension::Max;
                                style.gap /= 0;
                            }}>{swatch_rows(&palette)}</view>
                        },
                    )}
                </view>
                <view style={|style| style.width /= Dimension::Max}>
                    {panel(
                        "Tokens",
                        "Borders and spacing are resolved values too",
                        ui! {
                            <view style={|style| {
                                style.layout /= Layout::Vertical;
                                style.width /= Dimension::Max;
                                style.gap /= 1;
                            }}>
                                <row>{borders}</row>
                                {spacing}
                            </view>
                        },
                    )}
                </view>
            </view>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <view style={|style| style.width /= Dimension::Max}>
                    {panel(
                        "Typography roles",
                        "heading, label, body, muted, code",
                        ui! {
                            <view style={|style| {
                                style.layout /= Layout::Vertical;
                                style.width /= Dimension::Max;
                                style.gap /= 0;
                            }}>
                                <heading>"heading"</heading>
                                <label>"label"</label>
                                <paragraph>"body paragraph"</paragraph>
                                <muted>"muted"</muted>
                                <code>"code"</code>
                            </view>
                        },
                    )}
                </view>
                <view style={|style| style.width /= Dimension::Max}>
                    {nested}
                </view>
            </view>
        </view>
    }
}

// ---------------------------------------------------------------------------
// Shell
// ---------------------------------------------------------------------------

fn app(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (page, set_page) = cx.use_state(|| 0usize);
    let (dark, set_dark) = cx.use_state(|| true);
    let (preset, set_preset) = cx.use_state(|| 5usize);
    let (completed, set_completed) = cx.use_state(|| true);
    let (notifications, set_notifications) = cx.use_state(|| true);
    let (plan, set_plan) = cx.use_state(|| 0usize);
    let (query, set_query) = cx.use_state(String::new);
    let (draft, set_draft) = cx.use_state(|| "run --all".to_string());
    let (wrap, set_wrap) = cx.use_state(|| TextWrap::Hard);
    let (followed, set_followed) = cx.use_state(String::new);
    let (selection, set_selection) = cx.use_state(String::new);
    let (copied, set_copied) = cx.use_state(String::new);
    let (keyboard, set_keyboard) = cx.use_state(|| true);
    let (region_disabled, set_region_disabled) = cx.use_state(|| false);
    let (bars, set_bars) = cx.use_state(|| true);
    let (tick, set_tick) = cx.use_state(|| 0usize);
    let (live, set_live) = cx.use_state(|| true);
    let (vertical_offset, set_vertical_offset) = cx.use_state(|| 4_u64);
    let (horizontal_offset, set_horizontal_offset) = cx.use_state(|| 12_u64);

    // A cloneable session handle. The Quit button requests a graceful exit,
    // which runs the exit hooks registered in `main` and restores the terminal.
    let exit = cx.use_handle();

    // The animation flag lives in a ref so the worker thread and the switch
    // share one value across renders.
    let live_ref = cx.use_ref(|| true);
    cx.use_effect((), {
        let live_ref = live_ref.clone();
        move || {
            let alive = Arc::new(AtomicBool::new(true));
            let worker_alive = alive.clone();
            let worker = thread::spawn(move || {
                while worker_alive.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(150));
                    let running = *live_ref.lock().expect("live flag poisoned");
                    if running && worker_alive.load(Ordering::Relaxed) {
                        set_tick.update(|value| *value = value.wrapping_add(1));
                    }
                }
            });
            move || {
                alive.store(false, Ordering::Relaxed);
                let _ = worker.join();
            }
        }
    });

    let preset_index = preset % ThemePreset::ALL.len();
    let theme = ThemePreset::ALL[preset_index].theme(if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    });

    let keyboard_handler = {
        let set_page = set_page.clone();
        let set_dark = set_dark.clone();
        let set_preset = set_preset.clone();
        let set_live = set_live.clone();
        let live_ref = live_ref.clone();
        move |event: KeyboardEvent| match event.key.code {
            KeyCode::Char(ch) if ('1'..='6').contains(&ch) => {
                set_page.set(ch as usize - '1' as usize)
            }
            KeyCode::Right => set_page.update(|value| *value = (*value + 1) % PAGES.len()),
            KeyCode::Left => {
                set_page.update(|value| *value = (*value + PAGES.len() - 1) % PAGES.len())
            }
            KeyCode::Char('t') => set_dark.update(|value| *value = !*value),
            KeyCode::Char('p') => {
                set_preset.update(|value| *value = (*value + 1) % ThemePreset::ALL.len())
            }
            KeyCode::Char('l') => {
                let next = !*live_ref.lock().expect("live flag poisoned");
                *live_ref.lock().expect("live flag poisoned") = next;
                set_live.set(next);
            }
            _ => {}
        }
    };

    let brand = Text::from_spans([
        Span::new("◆ ICMD").foreground(theme.colors.primary).bold(),
        Span::new("  component lab").style(theme.typography.muted.clone()),
    ]);

    let live_indicator = if live {
        ui! { <spinner frame={tick} label={format!("frame {}", tick % 1000)} /> }
    } else {
        ui! { <muted>{format!("paused at frame {}", tick % 1000)}</muted> }
    };

    let actions = {
        let set_preset = set_preset.clone();
        let set_dark = set_dark.clone();
        let set_live = set_live.clone();
        let live_ref = live_ref.clone();
        ui! {
            <row>
                <button on_press={move |_| set_preset
                    .update(|value| *value = (*value + 1) % ThemePreset::ALL.len())}>
                    {ThemePreset::ALL[preset_index].name()}
                </button>
                <button variant={icmd::ButtonVariant::Secondary}
                    on_press={move |_| set_dark.set(!dark)}>
                    {if dark { "☾ dark" } else { "☀ light" }}
                </button>
                <switch
                    on={live}
                    label="live"
                    on_change={move |next: bool| {
                        *live_ref.lock().expect("live flag poisoned") = next;
                        set_live.set(next);
                    }}
                />
                <button variant={icmd::ButtonVariant::Destructive}
                    on_press={move |_| exit.request_exit()}>
                    "quit"
                </button>
            </row>
        }
    };

    let sidebar = {
        let items = (0..PAGES.len())
            .map(|index| nav_item(index, PAGES[index].0, index == page, &theme, &set_page))
            .collect::<Node>();
        ui! {
            <card style={|style| style.width /= Dimension::Cells(26)}>
                <label>"Pages"</label>
                <divider />
                {items}
                <divider />
                <label>"Keys"</label>
                {hint("1-6", "switch page")}
                {hint("←/→", "cycle pages")}
                {hint("t", "light / dark")}
                {hint("p", "next preset")}
                {hint("l", "freeze animation")}
                {hint("⇧Ctrl+C", "copy")}
                {hint("⇧Ctrl+V", "paste")}
                <divider />
                <muted>"Click a control to focus it; global keys keep working."</muted>
            </card>
        }
    };

    let content = match page {
        0 => overview(&theme, tick),
        1 => controls_page(
            &theme,
            ControlState {
                completed,
                set_completed: &set_completed,
                notifications,
                set_notifications: &set_notifications,
                plan,
                set_plan: &set_plan,
                query,
                set_query: &set_query,
                draft,
                set_draft: &set_draft,
                wrap,
                set_wrap: &set_wrap,
                followed,
                set_followed: &set_followed,
            },
        ),
        2 => typography_page(&theme, wrap),
        3 => selection_page(
            &theme,
            SelectionState {
                selection,
                set_selection: &set_selection,
                copied,
                set_copied: &set_copied,
                keyboard,
                set_keyboard: &set_keyboard,
                region_disabled,
                set_region_disabled: &set_region_disabled,
                tick,
            },
        ),
        4 => layout_page(
            &theme,
            LayoutState {
                vertical_offset,
                set_vertical_offset: &set_vertical_offset,
                horizontal_offset,
                set_horizontal_offset: &set_horizontal_offset,
                bars,
                set_bars: &set_bars,
                tick,
            },
        ),
        _ => theme_page(&theme, dark, preset_index, &set_dark, &set_preset),
    };

    let body = ui! {
        <scroll_area axes={ScrollAxes::Vertical} style={|style| {
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
        }}>{content}</scroll_area>
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
                {hint("1-6", PAGES[page].0)}
                {hint("t", "theme")}
                {hint("p", "preset")}
                {hint("l", "live")}
            </row>
            <row>
                <muted>{format!("{} • {} • mouse on", ThemePreset::ALL[preset_index].name(), if dark { "dark" } else { "light" })}</muted>
                <kbd>"Ctrl+C"</kbd>
                <muted>"quit"</muted>
            </row>
        </view>
    };

    let background = theme.colors.background;
    let foreground = theme.colors.foreground;
    let shell = ui! {
        <view
            on_app_key={keyboard_handler}
            style={move |style| {
                style.width /= Dimension::Percent(Percent::viewport(100));
                style.height /= Dimension::Percent(Percent::viewport(100));
                style.layout /= Layout::Vertical;
                style.padding /= Edges::all(1);
                style.gap /= 1;
                style.background /= background;
                style.text.foreground /= foreground;
            }}
        >
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 0;
            }}>
                <row>{brand}{live_indicator}</row>
                {actions}
            </view>
            <divider />
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {sidebar}
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                }}>{body}</view>
            </view>
            <divider />
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
    // Application lifecycle hooks. The `on_exit` hook runs after the terminal
    // has been restored, so it can write to the normal screen; `on_unmount`
    // would run while the alternate screen is still active.
    let lifecycle = AppLifecycle::new().on_exit(|session| {
        let reason = match session.exit {
            Some(ExitReason::ExitKey) => "exit key",
            Some(ExitReason::Requested) => "exit requested",
            Some(ExitReason::RuntimeClosed) => "runtime closed",
            Some(ExitReason::Failed) => "failure",
            Some(ExitReason::Aborted) => "aborted",
            None => "unknown",
        };
        println!(
            "icmd demo exited after {} frame(s): {reason}",
            session.frames_presented
        );
    });
    render_with(
        app.apply(()),
        RuntimeConfig {
            emoji_merging: EmojiMerging::Auto,
            ..Default::default()
        },
        lifecycle,
    )?;
    Ok(())
}
