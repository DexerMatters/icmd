//! Chapter 03 — Layout and Styling.
//!
//! Turns terminal geometry into a set of deliberate choices: boxes, axes,
//! sizing, surfaces, overlays, overflow, measurement, and breakpoints.

use icmd::theme::Theme;
use icmd::{
    Align, AxisPosition, BorderKind, ButtonVariant, Component, ComponentContext, Dimension, Edges,
    ElementRef, ElementSnapshot, Justify, Layout, Node, Overflow, Percent, Props, Span,
    StateSetter, Text, TextOverflow, TextWrap, Visibility, badge, button, card, center, column,
    container, footer, heading, muted, row, section as layout_section, spacer, ui, view,
};

use super::{ChapterProps, document, masthead, section};
use crate::demos::{NARROW_BREAKPOINT, WIDE_BREAKPOINT, WidthMode};
use crate::docs::{self, ApiRow, CalloutKind};
use crate::metadata::{self, SectionMeta};
use crate::snippets;

/// One miniature deploy console assembled from every layout primitive.
fn layout_console(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    const SERVICES: [&str; 3] = ["api", "worker", "scheduler"];
    let (selected, set_selected) = cx.use_state(|| 0_usize);
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let border = theme.colors.border;
    let buttons = SERVICES
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let choose = set_selected.clone();
            let marker = if index == selected { "▸" } else { " " };
            ui! {
                <button variant={if index == selected { ButtonVariant::Primary } else { ButtonVariant::Secondary }}
                    on_press={move |_| choose.set(index)}>
                    {format!("{marker} {name}")}
                </button>
            }
        })
        .collect::<Node>();
    let active = SERVICES[selected];
    ui! {
        <container style={|style| { style.height /= Dimension::Auto; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
            }}>
                <heading>"Deploy console"</heading>
                <muted>"staging"</muted>
            </row>
            <layout_section>
                <card>
                    <row style={|style| { style.width /= Dimension::Max; style.gap /= 1; }}>
                        {Text::new("build").bold()}
                        <spacer />
                        {Text::new("ready").foreground(primary)}
                    </row>
                </card>
                <center style={|style| { style.height /= Dimension::Cells(3); }}>
                    <muted>"center() aligns both axes"</muted>
                </center>
            </layout_section>
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <column style={|style| { style.width /= Dimension::Cells(16); style.gap /= 1; }}>
                    <muted>"column"</muted>
                    {buttons}
                </column>
                <column style={move |style| {
                    style.width /= Dimension::Max;
                    style.padding /= Edges::all(1);
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                    <muted>{Text::new("the remaining width")}</muted>
                    {Text::new(format!("selected service: {active}")).foreground(primary).bold()}
                </column>
            </view>
            <footer>
                <muted>"footer() is a muted horizontal row"</muted>
            </footer>
        </container>
    }
}

/// A small labelled block used inside the axis specimens.
fn axis_chip(theme: &Theme, label: &str) -> Node {
    let background = theme.colors.muted;
    let foreground = theme.colors.muted_foreground;
    ui! {
        <view style={move |style| {
            style.width /= Dimension::Cells(5);
            style.padding /= Edges::symmetric(0, 1);
            style.background /= background;
            style.text.foreground /= foreground;
            // A fill glyph makes the chip's own box visible, which is what turns
            // `Align::Stretch` into something a reader can see: a stretched chip
            // paints three filled rows instead of one.
            style.fill /= "·";
        }}>
            {Text::new(label).bold()}
        </view>
    }
}

/// A specimen caption: the value name and what it does.
fn axis_caption(theme: &Theme, label: &str, effect: &str) -> Node {
    let caption = theme.colors.muted_foreground;
    let primary = theme.colors.primary;
    ui! {
        {Text::from_spans([
            Span::new(label.to_string()).foreground(primary).bold(),
            Span::new(format!("  {effect}")).foreground(caption),
        ]).wrap(TextWrap::Soft)}
    }
}

/// One labelled main-axis distribution specimen.
///
/// The box is five rows tall with a full border, which leaves three content
/// rows: enough for a cross-axis arrangement to be visible rather than to
/// collapse into the single row a shorter box would leave.
fn justify_specimen(theme: &Theme, label: &str, effect: &str, justify: Justify) -> Node {
    let border = theme.colors.border;
    let card = theme.colors.card;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>
            {axis_caption(theme, label, effect)}
            <row style={move |style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.height /= Dimension::Cells(5);
                style.align /= Align::Center;
                style.justify /= justify;
                style.gap /= 1;
                style.padding /= Edges::symmetric(0, 1);
                style.background /= card;
                style.border.kind /= BorderKind::Single;
                style.border.foreground /= border;
            }}>
                {axis_chip(theme, "A")}
                {axis_chip(theme, "B")}
                {axis_chip(theme, "C")}
            </row>
        </view>
    }
}

/// One labelled cross-axis alignment specimen.
///
/// Five rows with a full border leaves three content rows, so `Start`,
/// `Center`, `End`, and `Stretch` each land somewhere visibly different: the
/// chips sit on the top edge, in the middle, on the bottom edge, or fill the
/// box. A three-row box would leave one content row and hide the difference
/// entirely.
fn align_specimen(theme: &Theme, label: &str, effect: &str, align: Align) -> Node {
    let border = theme.colors.border;
    let card = theme.colors.card;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>
            {axis_caption(theme, label, effect)}
            <row style={move |style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.height /= Dimension::Cells(5);
                style.align /= align;
                style.justify /= Justify::Start;
                style.gap /= 1;
                style.padding /= Edges::symmetric(0, 1);
                style.background /= card;
                style.border.kind /= BorderKind::Single;
                style.border.foreground /= border;
            }}>
                {axis_chip(theme, "A")}
                {axis_chip(theme, "B")}
                {axis_chip(theme, "C")}
            </row>
        </view>
    }
}

/// One sizing column: a request, a label, and its committed width.
fn sizing_column(
    theme: &Theme,
    element_ref: ElementRef,
    setter: StateSetter<(u32, u32, u32)>,
    slot: usize,
    label: &str,
    dimension: Dimension,
) -> Node {
    let border = theme.colors.border;
    let card = theme.colors.card;
    ui! {
        <view element_ref={element_ref}
            on_element_change={move |snapshot: Option<ElementSnapshot>| {
                let width = snapshot.map_or(0, |snapshot| snapshot.bounding_rect().width);
                setter.update(move |widths| match slot {
                    0 => widths.0 = width,
                    1 => widths.1 = width,
                    _ => widths.2 = width,
                });
            }}
            style={move |style| {
                style.width /= dimension;
                style.padding /= Edges::symmetric(0, 1);
                style.background /= card;
                style.border.kind /= BorderKind::Single;
                style.border.foreground /= border;
            }}>
            <muted>{Text::new(label).wrap(TextWrap::Soft)}</muted>
        </view>
    }
}

/// A fixed rail, a percentage column, and a filling column, all measured.
fn sizing_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let rail_ref = cx.use_element_ref();
    let percent_ref = cx.use_element_ref();
    let fill_ref = cx.use_element_ref();
    let (widths, set_widths) = cx.use_state(|| (0_u32, 0_u32, 0_u32));
    let theme = cx.use_theme();
    let rail = sizing_column(
        &theme,
        rail_ref,
        set_widths.clone(),
        0,
        "Cells(16)",
        Dimension::Cells(16),
    );
    let percent = sizing_column(
        &theme,
        percent_ref,
        set_widths.clone(),
        1,
        "Percent::available(40)",
        Dimension::Percent(Percent::available(40)),
    );
    let fill = sizing_column(
        &theme,
        fill_ref,
        set_widths,
        2,
        "Dimension::Max",
        Dimension::Max,
    );
    let (rail_width, percent_width, fill_width) = widths;
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.height /= Dimension::Cells(3);
                style.gap /= 1;
            }}>
                {rail}
                {percent}
                {fill}
            </row>
            <muted>{Text::new(format!(
                "measured in this frame: rail {rail_width} · percent {percent_width} · fill {fill_width}",
            ))
            .wrap(TextWrap::Soft)}</muted>
        </column>
    }
}

/// One bordered box with a label, used by the border gallery.
fn border_box(theme: &Theme, kind: BorderKind, label: &str, detail: &str) -> Node {
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let border = theme.colors.border;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::all(1);
            style.background /= card;
            style.text.foreground /= card_foreground;
            style.border.kind /= kind;
            style.border.foreground /= border;
            style.border.background /= card;
        }}>
            {Text::new(label).bold()}
            <muted>{Text::new(detail).wrap(TextWrap::Soft)}</muted>
        </view>
    }
}

/// The four border kinds, two per row.
fn border_gallery(theme: &Theme) -> Node {
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {border_box(theme, BorderKind::Single, "Single", "the default line")}
                {border_box(theme, BorderKind::Rounded, "Rounded", "the docs theme default")}
            </row>
            <row style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {border_box(theme, BorderKind::Double, "Double", "heavier emphasis")}
                {border_box(theme, BorderKind::Heavy, "Heavy", "loudest of the four")}
            </row>
        </column>
    }
}

/// Padding, margin, fill, and inherited text color in bounded surfaces.
fn surface_specimen(theme: &Theme) -> Node {
    let background = theme.colors.card;
    let border = theme.colors.border;
    let accent = theme.colors.accent;
    let muted_background = theme.colors.muted;
    ui! {
        <row style={|style| {
            style.layout /= Layout::Horizontal;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
                style.margin /= Edges::symmetric(1, 2);
                style.padding /= Edges::all(1);
                style.background /= background;
                style.text.foreground /= accent;
                style.border.kind /= BorderKind::Single;
                style.border.foreground /= border;
            }}>
                {Text::new("padding 1 · margin 1/2")}
                {Text::new("text.foreground inherits to children").wrap(TextWrap::Soft)}
            </view>
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.padding /= Edges::all(1);
                style.background /= muted_background;
                style.fill /= "·";
                style.border.kind /= BorderKind::Single;
                style.border.foreground /= border;
            }}>
                {Text::new("background + Fill")}
                <muted>{Text::new("the fill glyph paints the empty cells").wrap(TextWrap::Soft)}</muted>
            </view>
        </row>
    }
}

/// A notification badge absolutely positioned over a card.
fn notification_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (unread, set_unread) = cx.use_state(|| 3_u32);
    let theme = cx.use_theme();
    let card_background = theme.colors.card;
    let border = theme.colors.border;
    let more = set_unread.clone();
    let burst = set_unread.clone();
    let clear = set_unread;
    let visibility = if unread == 0 {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    let label = if unread == 0 {
        String::from("inbox zero")
    } else {
        format!("{unread} unread")
    };
    ui! {
        <card style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
            style.padding /= Edges::all(1);
            style.background /= card_background;
            style.border.foreground /= border;
        }}>
            <row style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.height /= Dimension::Cells(4);
            }}>
                <card style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                    style.padding /= Edges::all(1);
                }}>
                    {Text::new("Inbox").bold()}
                    <muted>{Text::new(label).wrap(TextWrap::Soft)}</muted>
                </card>
                <badge text={unread.to_string()} variant={icmd::BadgeVariant::Destructive}
                    style={move |style| {
                        style.layout /= Layout::Absolute;
                        style.line /= AxisPosition::Cells(0);
                        style.column /= AxisPosition::Cells(22);
                        style.z_index /= 10;
                        style.visibility /= visibility;
                    }} />
            </row>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| more.update(|value| *value += 1)}>"new message"</button>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| burst.update(|value| *value += 9)}>"nine more"</button>
                <button variant={ButtonVariant::Destructive}
                    on_press={move |_| clear.set(0)}>"read all"</button>
            </row>
            <muted>{Text::new(
                "the badge is out of flow, painted above the card by z_index, and Hidden when nothing is unread.",
            )
            .wrap(TextWrap::Soft)}</muted>
        </card>
    }
}

/// Visible, invisible, and hidden boxes, each labelled with the space it keeps.
fn visibility_specimen(theme: &Theme) -> Node {
    const STATES: [(Visibility, &str, &str); 3] = [
        (Visibility::Visible, "Visible", "paints and takes space"),
        (
            Visibility::Invisible,
            "Invisible",
            "takes space, paints nothing",
        ),
        (Visibility::Hidden, "Hidden", "no space, no paint"),
    ];
    let card = theme.colors.card;
    let border = theme.colors.border;
    STATES
        .iter()
        .map(|(visibility, label, detail)| {
            let visibility = *visibility;
            let label = *label;
            let detail = *detail;
            ui! {
                <row style={|style| {
                    style.layout /= Layout::Horizontal;
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.gap /= 1;
                }}>
                    {Text::new(label).bold()}
                    <view style={move |style| {
                        style.layout /= Layout::Horizontal;
                        style.width /= Dimension::Cells(10);
                        style.padding /= Edges::symmetric(0, 1);
                        style.background /= card;
                        style.visibility /= visibility;
                        style.border.kind /= BorderKind::Single;
                        style.border.foreground /= border;
                    }}>
                        {Text::new("[box]")}
                    </view>
                    <muted>{Text::new(detail).wrap(TextWrap::Soft)}</muted>
                </row>
            }
        })
        .collect::<Node>()
}

/// Box overflow and text overflow inside bounded regions.
fn overflow_specimen(theme: &Theme) -> Node {
    let card = theme.colors.card;
    let border = theme.colors.border;
    let long = "this sentence is deliberately longer than its box and must do something";
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 2;
            }}>
                <view style={move |style| {
                    style.width /= Dimension::Cells(22);
                    style.height /= Dimension::Cells(5);
                    style.padding /= Edges::all(1);
                    style.background /= card;
                    style.overflow /= Overflow::Clip;
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                    <view style={|style| {
                        style.width /= Dimension::Cells(40);
                        style.height /= Dimension::Cells(1);
                    }}>
                        {Text::new(long).wrap(TextWrap::NoWrap)}
                    </view>
                </view>
                <view style={move |style| {
                    style.width /= Dimension::Cells(22);
                    style.height /= Dimension::Cells(5);
                    style.padding /= Edges::all(1);
                    style.background /= card;
                    style.overflow /= Overflow::Visible;
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                    <view style={|style| {
                        style.width /= Dimension::Cells(40);
                        style.height /= Dimension::Cells(1);
                    }}>
                        {Text::new(long).wrap(TextWrap::NoWrap)}
                    </view>
                </view>
            </row>
            <muted>{Text::new(
                "left: Overflow::Clip stops at the border · right: Overflow::Visible paints past it",
            )
            .wrap(TextWrap::Soft)}</muted>
            <row style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                <view style={move |style| {
                    style.width /= Dimension::Cells(22);
                    style.padding /= Edges::all(1);
                    style.background /= card;
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                    {Text::new(long).wrap(TextWrap::NoWrap).overflow(TextOverflow::Clip)}
                </view>
                <view style={move |style| {
                    style.width /= Dimension::Cells(22);
                    style.padding /= Edges::all(1);
                    style.background /= card;
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                    {Text::new(long).wrap(TextWrap::NoWrap).overflow(TextOverflow::Ellipsis)}
                </view>
            </row>
            <muted>{Text::new(
                "TextOverflow::Clip cuts at the edge · TextOverflow::Ellipsis marks the cut",
            )
            .wrap(TextWrap::Soft)}</muted>
        </column>
    }
}

/// Human-readable name for a child layout direction.
fn layout_name(layout: Layout) -> &'static str {
    match layout {
        Layout::Vertical => "vertical",
        Layout::Horizontal => "horizontal",
        Layout::Absolute => "absolute",
    }
}

/// Human-readable name for an overflow policy.
fn overflow_name(overflow: Overflow) -> &'static str {
    match overflow {
        Overflow::Clip => "clip",
        Overflow::Visible => "visible",
    }
}

/// Formats the committed fields the measure readout shows.
fn describe_snapshot(snapshot: &ElementSnapshot) -> String {
    let bounds = snapshot.bounding_rect();
    let content = snapshot.content_rect();
    let style = snapshot.resolved_style();
    let visible = snapshot.visible_rect().map_or_else(
        || String::from("clipped"),
        |rect| format!("{}x{}", rect.width, rect.height),
    );
    let scroll = snapshot.scroll().map_or_else(
        || String::from("this box does not scroll"),
        |scroll| {
            format!(
                "scroll max y {} · content h {}",
                scroll.max_offset.y, scroll.content_height
            )
        },
    );
    format!(
        "bounds {}x{} at ({}, {}) · content {}x{} · visible {visible} · layout {} · gap {} · padding {}/{}/{}/{} · overflow {}/{} · z {} · {scroll}",
        bounds.width,
        bounds.height,
        bounds.line,
        bounds.column,
        content.width,
        content.height,
        layout_name(style.layout),
        style.gap,
        style.padding.top,
        style.padding.right,
        style.padding.bottom,
        style.padding.left,
        overflow_name(style.overflow_x),
        overflow_name(style.overflow_y),
        style.z_index,
    )
}

/// A live readout of committed geometry and resolved style.
fn measure_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let measured = cx.use_element_ref();
    let (report, set_report) = cx.use_state(|| String::from("waiting for the first commit"));
    let measured_for_element = measured.clone();
    let measured_for_press = measured;
    let writer = set_report.clone();
    let theme = cx.use_theme();
    let muted_foreground = theme.colors.muted_foreground;
    ui! {
        <card element_ref={measured_for_element}
            on_element_change={move |snapshot: Option<ElementSnapshot>| {
                writer.set(snapshot.as_ref().map_or_else(
                    || String::from("not committed"),
                    describe_snapshot,
                ));
            }}
            style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
                style.padding /= Edges::all(1);
                style.border.foreground /= theme.colors.border;
            }}>
            {Text::new("COMMITTED GEOMETRY").bold()}
            <muted>{Text::new(report).wrap(TextWrap::Soft)}</muted>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| {
                        set_report.set(measured_for_press.current().as_ref().map_or_else(
                            || String::from("no snapshot yet"),
                            describe_snapshot,
                        ));
                    }}>"read current() now"</button>
                {Text::new("resize the terminal and the numbers change").foreground(muted_foreground)}
            </row>
        </card>
    }
}

/// Measures its own width and shows the treatment that width selects.
fn responsive_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let measured = cx.use_element_ref();
    let (width, set_width) = cx.use_state(|| 0_u16);
    let measured_for_element = measured.clone();
    let theme = cx.use_theme();
    let border = theme.colors.border;
    let card = theme.colors.card;
    let muted_foreground = theme.colors.muted_foreground;
    let mode = WidthMode::for_width(width, false);
    let rail_is_side = mode != WidthMode::Narrow;
    let rail = ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Cells(8);
            style.gap /= 0;
            style.padding /= Edges::symmetric(0, 1);
            style.background /= card;
            style.border.kind /= BorderKind::Single;
            style.border.foreground /= border;
        }}>
            {Text::new("rail").foreground(muted_foreground).bold()}
            {Text::new("02")}
            {Text::new("03")}
        </view>
    };
    let content = ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::symmetric(0, 1);
        }}>
            {Text::new("content column").bold()}
            <muted>{Text::new("prose keeps a bounded reading width").wrap(TextWrap::Soft)}</muted>
        </view>
    };
    let body = if rail_is_side {
        ui! {
            <row style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {rail}
                {content}
            </row>
        }
    } else {
        ui! {
            <column style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {rail}
                {content}
            </column>
        }
    };
    ui! {
        <column element_ref={measured_for_element}
            on_element_change={move |snapshot: Option<ElementSnapshot>| {
                let next = snapshot.map_or(0_u16, |snapshot| {
                    u16::try_from(snapshot.bounding_rect().width).unwrap_or(u16::MAX)
                });
                set_width.set(next);
            }}
            style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
            {Text::from_spans([
                Span::new("measured width ").bold(),
                Span::new(format!("{width} cells")).bold(),
                Span::new(format!(" · {}", mode.label())),
            ])}
            {body}
            <muted>{Text::new(format!(
                "thresholds: {WIDE_BREAKPOINT}+ wide · {NARROW_BREAKPOINT}–{} medium · below {NARROW_BREAKPOINT} narrow",
                WIDE_BREAKPOINT - 1,
            ))
            .wrap(TextWrap::Soft)}</muted>
        </column>
    }
}

/// Chapter 03.
pub(super) fn layout_and_styling(cx: &mut ComponentContext, props: &Props<ChapterProps>) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(2);
    let sections: &[SectionMeta] = meta.sections;

    let boxes = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body("Layout is a small vocabulary rather than a grid system. `view` is the bare box; `container` is a full-size background; `column` and `row` stack children on one axis with a theme gap; `section` groups a page; `card` adds a surface and border; `center` aligns on both axes; `spacer` is one deliberate cell; `footer` is a muted row. Every screen is a sentence in those words.")}
            {docs::live_example(
                &theme,
                "one miniature screen",
                "Select a service; every box around the choice is a built-in primitive rather than a custom container.",
                ui! { {layout_console.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "the same screen as source", snippets::LAYOUT_ANATOMY)}
            {docs::notice(&theme, "Semantic boxes carry the defaults that make a screen consistent: `container` and `card` set a background, `section` sets medium spacing, and `center` sets both alignments. Overriding one style field keeps every other default from the theme.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "view", purpose: "bare element box", defaults: "no styling at all", events: "any DOM event" },
                ApiRow { name: "container", purpose: "full-size background surface", defaults: "vertical, xs gap, fill", events: "none" },
                ApiRow { name: "column", purpose: "vertical stack", defaults: "small gap", events: "none" },
                ApiRow { name: "row", purpose: "horizontal stack", defaults: "small gap", events: "none" },
                ApiRow { name: "section", purpose: "page-level group", defaults: "full width, medium gap", events: "none" },
                ApiRow { name: "card", purpose: "bordered surface", defaults: "card colors, symmetric padding", events: "none" },
                ApiRow { name: "center", purpose: "center on both axes", defaults: "full width, centered", events: "none" },
                ApiRow { name: "spacer", purpose: "one fixed cell of separation", defaults: "1x1 cells", events: "none" },
                ApiRow { name: "footer", purpose: "muted horizontal row", defaults: "muted typography", events: "none" },
            ])}
        },
    );

    let axes = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body_pair(
                "Distribution and alignment are two independent decisions on perpendicular axes. `layout` picks the main axis: `Vertical` stacks top to bottom and `Horizontal` places left to right. `justify` distributes children along that main axis; `align` positions them across it. `gap` is the minimum space between siblings.",
                "Read a specimen as a sentence: a horizontal row justified to SpaceBetween pushes its chips apart, while the same row aligned to Center lifts them to the middle of the box. Changing one field never changes the other.",
            )}
            {docs::two_column(
                &theme,
                data.wide(),
                ui! {
                    {docs::specimen(&theme, "justify · the main axis", ui! {
                        {justify_specimen(&theme, "Start", "packed at the leading edge", Justify::Start)}
                        {justify_specimen(&theme, "Center", "packed in the middle", Justify::Center)}
                        {justify_specimen(&theme, "End", "packed at the trailing edge", Justify::End)}
                        {justify_specimen(&theme, "SpaceBetween", "spread edge to edge", Justify::SpaceBetween)}
                    })}
                },
                ui! {
                    {docs::specimen(&theme, "align · the cross axis", ui! {
                        {align_specimen(&theme, "Start", "chips on the top edge", Align::Start)}
                        {align_specimen(&theme, "Center", "chips in the middle", Align::Center)}
                        {align_specimen(&theme, "End", "chips on the bottom edge", Align::End)}
                        {align_specimen(&theme, "Stretch", "chips fill the taller box", Align::Stretch)}
                    })}
                },
            )}
            {docs::watch_for(&theme, "`justify` is ignored when a child asks for `Dimension::Max` on the main axis: the filling child consumes the free space that distribution would have used. Put a filler in only when you mean it.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Layout", purpose: "main axis and out-of-flow placement", defaults: "Layout::Vertical", events: "none" },
                ApiRow { name: "Justify", purpose: "distribution along the main axis", defaults: "Justify::Start", events: "none" },
                ApiRow { name: "Align", purpose: "alignment across the cross axis", defaults: "Align::Start", events: "none" },
                ApiRow { name: "gap", purpose: "minimum cells between siblings", defaults: "theme spacing", events: "none" },
            ])}
        },
    );

    let sizing = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body("A size request is a promise about one axis. `Auto` asks for the content's own size, `Cells(n)` asks for a fixed number of columns or rows, `Percent(p)` asks for a fraction, and `Max` takes whatever is left after the others. Rows and columns resolve left to right, which is why a fixed request is more predictable than a chain of percentages.")}
            {docs::specimen(&theme, "fixed rail, percentage column, filling column", ui! { {sizing_demo.apply(())} })}
            {docs::notice(&theme, "`Percent::available` is measured after fixed siblings, margins, and gaps, so percentages divide the space that is actually left. `Percent::viewport` is measured against the whole terminal instead; reserve it for overlays and full-height panels that must relate to the screen rather than to their parent.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Dimension::Auto", purpose: "size to content", defaults: "the default request", events: "none" },
                ApiRow { name: "Dimension::Cells", purpose: "fixed cells on one axis", defaults: "—", events: "none" },
                ApiRow { name: "Dimension::Percent", purpose: "fraction of available space or viewport", defaults: "—", events: "none" },
                ApiRow { name: "Dimension::Max", purpose: "take the remaining space", defaults: "shared equally when several", events: "none" },
                ApiRow { name: "Percent::available", purpose: "percentage of the remaining space", defaults: "PercentBasis::Available", events: "none" },
                ApiRow { name: "Percent::viewport", purpose: "percentage of the terminal", defaults: "PercentBasis::Viewport", events: "none" },
            ])}
        },
    );

    let spacing = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body("Spacing is two ideas that are easy to confuse. `margin` is outside the box and pushes siblings; `padding` is inside it and pushes content away from the border. `gap` belongs to the parent and spaces children. A `background` colors the box, `fill` paints the empty cells with a validated glyph, and a border has a kind, a set of edges, and its own colors.")}
            {docs::two_column(
                &theme,
                data.wide(),
                ui! { {docs::specimen(&theme, "the four border kinds", border_gallery(&theme))} },
                ui! { {docs::specimen(&theme, "padding, fill, inherited text", surface_specimen(&theme))} },
            )}
            {docs::notice(&theme, "Border edges are per side, so a divider is just a box with one edge drawn. Text styling is inherited: setting `style.text.foreground` on a surface colors every descendant that does not override it, which keeps a card readable without repeating colors.")}
            {docs::callout(&theme, CalloutKind::Production, "Style chrome from theme tokens — `theme.borders.kind`, `theme.borders.foreground`, and `theme.spacing` — rather than literal glyphs and colors. A theme swap then restyles the whole application, and the eight presets keep working.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Edges", purpose: "four per-side values, top right bottom left", defaults: "all zero", events: "none" },
                ApiRow { name: "BorderKind", purpose: "line character set", defaults: "BorderKind::Single", events: "none" },
                ApiRow { name: "Fill", purpose: "validated glyph for background cells", defaults: "no fill", events: "none" },
                ApiRow { name: "Style", purpose: "layout, spacing, border, and text in one tree", defaults: "everything unset", events: "none" },
                ApiRow { name: "card", purpose: "the themed surface most content sits on", defaults: "card colors and border", events: "none" },
            ])}
        },
    );

    let positioning = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body("Flow layout places siblings in order; an overlay has to leave the flow. `Layout::Absolute` takes a box out of the arrangement, and `line` and `column` place it with `AxisPosition`: `Start`, `Cells(n)`, `Percent`, `Center`, or `End`. `z_index` orders the paint, so a badge can sit above a card it overlaps.")}
            {docs::live_example(
                &theme,
                "a badge above the card",
                "Add messages and the badge repaints above the card; read all and Hidden removes it from the paint without disturbing the flow.",
                ui! { {notification_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "absolute placement and paint order", snippets::NOTIFICATION_BADGE)}
            {docs::specimen(&theme, "visibility keeps or drops space", ui! { {visibility_specimen(&theme)} })}
            {docs::notice(&theme, "`Visibility::Visible` paints and occupies space, `Invisible` occupies the same space but paints nothing, and `Hidden` removes both the paint and the layout box. Absolute children are positioned against their containing box, so an overlay belongs inside the surface it decorates.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Layout::Absolute", purpose: "take a box out of flow", defaults: "in flow", events: "none" },
                ApiRow { name: "AxisPosition", purpose: "line and column placement", defaults: "AxisPosition::Start", events: "none" },
                ApiRow { name: "z_index", purpose: "paint order offset", defaults: "0, painted in tree order", events: "none" },
                ApiRow { name: "Visibility", purpose: "paint and layout participation", defaults: "Visibility::Visible", events: "none" },
            ])}
        },
    );

    let overflow = section(
        &theme,
        data,
        5,
        &sections[5],
        ui! {
            {docs::body_pair(
                "Box overflow and text overflow are separate policies. A box decides whether its content may paint outside its own bounds with `Overflow::Clip` or `Overflow::Visible`. A text leaf decides how a line that still does not fit ends with `TextOverflow::Clip` or `TextOverflow::Ellipsis`.",
                "Clipping is also an ownership question. A bounded region that scrolls must clip on the axes it scrolls; `scroll_area` sets that clip and owns the offset, so a nested scroll region rather than a styled box is the right tool when content is longer than its frame.",
            )}
            {docs::specimen(&theme, "bounded regions and their policies", overflow_specimen(&theme))}
            {docs::notice(&theme, "Text still overflows if wrapping cannot help: `NoWrap` keeps one line, `Soft` breaks at word boundaries, and `Hard` breaks at the exact column. Choose the wrap first, then choose what the remaining overflow does.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Overflow", purpose: "whether a box may paint outside itself", defaults: "Overflow::Clip", events: "none" },
                ApiRow { name: "overflow_x", purpose: "horizontal policy, overriding overflow", defaults: "inherits overflow", events: "none" },
                ApiRow { name: "overflow_y", purpose: "vertical policy, overriding overflow", defaults: "inherits overflow", events: "none" },
                ApiRow { name: "TextOverflow", purpose: "how a truncated line ends", defaults: "TextOverflow::Clip", events: "none" },
                ApiRow { name: "TextWrap", purpose: "where a long line reflows", defaults: "TextWrap::NoWrap", events: "none" },
                ApiRow { name: "scroll_area", purpose: "clip and scroll a bounded region", defaults: "vertical, runtime-owned offset", events: "on_scroll" },
            ])}
        },
    );

    let measure = section(
        &theme,
        data,
        6,
        &sections[6],
        ui! {
            {docs::body("Geometry is knowable only after commit. An `ElementRef` attaches to a host element, and the commit stage publishes an `ElementSnapshot` whenever that host's geometry or resolved style changes. The snapshot carries the bounding rectangle, the content rectangle, the visible portion after clipping, the resolved style, and scroll state when the host scrolls.")}
            {docs::live_example(
                &theme,
                "committed geometry, live",
                "Resize the terminal; the readout follows the snapshot. The button reads the same ref outside a listener.",
                ui! { {measure_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "a geometry readout driven by snapshots", snippets::GEOMETRY_READOUT)}
            {docs::callout(&theme, CalloutKind::Info, "A snapshot is evidence about the last frame, not an input to the next one. Read it for alignment, diagnostics, or a scroll target after commit; do not compute layout from it during render, or the first frame has nothing to show.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ElementRef", purpose: "stable handle to one host's committed state", defaults: "current() is None before commit", events: "on_element_change" },
                ApiRow { name: "ElementSnapshot", purpose: "rectangles, resolved style, scroll state", defaults: "replaced only when it changes", events: "none" },
                ApiRow { name: "ElementRect", purpose: "viewport-relative line, column, size", defaults: "—", events: "none" },
                ApiRow { name: "ResolvedElementStyle", purpose: "effective layout, spacing, overflow, colors", defaults: "after inheritance", events: "none" },
                ApiRow { name: "ElementScrollState", purpose: "offset, max offset, content extent", defaults: "only on a scroll host", events: "none" },
            ])}
        },
    );

    let responsive = section(
        &theme,
        data,
        7,
        &sections[7],
        ui! {
            {docs::body("This guide is itself the example. Its shell measures the terminal root and selects one of three treatments: at 110 cells or wider the expanded index sits beside the document, from 72 to 109 cells a compact numbered rail replaces it, and below 72 cells the index disappears so header and search carry navigation.")}
            {docs::ref_row(&theme, "110+ cells", "expanded index beside a wide document column", "wide")}
            {docs::ref_row(&theme, "72–109 cells", "compact numbered rail with section dots", "medium")}
            {docs::ref_row(&theme, "below 72 cells", "content only; navigation lives in header and search", "narrow")}
            {docs::live_example(
                &theme,
                "which band does this width select?",
                "Resize the terminal. The rail moves beside the content while the measured width is at least 72 cells and stacks above it below that.",
                ui! { {responsive_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "the shell right now", if data.wide() { "at least 110 cells: the expanded index is showing" } else { "under 110 cells: rail or content-only treatment" })} }),
            )}
            {docs::notice(&theme, "Give a rail a fixed cell width and let the document take `Dimension::Max`. Cap the reading column so prose does not stretch to a 200-cell line. Hide low-priority data columns with `Visibility::Hidden` instead of squeezing every column until none is readable, and keep fixed controls at a stable width so they do not reflow while text changes.")}
            {docs::watch_for(&theme, "The demo measures the reading column, not the terminal root, so it reports a narrower band than the shell does on a wide screen. Breakpoints only mean something against the box they are measured on.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Dimension", purpose: "express fixed, flexible, and percentage widths", defaults: "Dimension::Auto", events: "none" },
                ApiRow { name: "ElementRef", purpose: "measure the box a breakpoint applies to", defaults: "unattached until commit", events: "on_element_change" },
                ApiRow { name: "Visibility", purpose: "drop a low-priority column without losing the frame", defaults: "Visibility::Visible", events: "none" },
                ApiRow { name: "Percent", purpose: "share a row without hard-coding cells", defaults: "PercentBasis::Available", events: "none" },
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
                {boxes}
                {axes}
                {sizing}
                {spacing}
                {positioning}
                {overflow}
                {measure}
                {responsive}
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}

#[cfg(test)]
mod tests {
    use icmd::Size;

    use crate::screen::Screen;

    /// The chip glyph the axis specimens paint, including its fill.
    const CHIP: &str = "\u{b7}A\u{b7}\u{b7}\u{b7}";
    /// A stretched chip's continuation rows carry fill only.
    const FILLED: &str = "\u{b7}\u{b7}\u{b7}\u{b7}\u{b7}";

    /// Finds the first chip at or below `from_line` inside one column range.
    ///
    /// Columns are character indices, so a row full of multi-byte box drawing
    /// can be sliced without splitting a glyph.
    fn chip_in(
        screen: &Screen,
        from_line: usize,
        range: (usize, usize),
        needle: &str,
    ) -> Option<(usize, usize)> {
        let needle: Vec<char> = needle.chars().collect();
        screen
            .rows()
            .iter()
            .enumerate()
            .skip(from_line)
            .find_map(|(line, row)| {
                let chars: Vec<char> = row.chars().collect();
                let start = range.0.min(chars.len());
                let end = range.1.min(chars.len());
                let haystack = chars.get(start..end)?;
                haystack
                    .windows(needle.len())
                    .position(|window| window == needle.as_slice())
                    .map(|column| (line, start + column))
            })
    }

    /// Counts the rows whose column range contains `needle`.
    fn rows_with_in(screen: &Screen, range: (usize, usize), needle: &str) -> usize {
        let needle: Vec<char> = needle.chars().collect();
        screen
            .rows()
            .iter()
            .filter(|row| {
                let chars: Vec<char> = row.chars().collect();
                let start = range.0.min(chars.len());
                let end = range.1.min(chars.len());
                chars.get(start..end).is_some_and(|slice| {
                    slice.windows(needle.len()).any(|w| w == needle.as_slice())
                })
            })
            .count()
    }

    /// The two specimen columns, as `(left, right)` column ranges.
    ///
    /// Both columns carry the same value labels, so a search has to be scoped to
    /// one of them or it will find the neighbour's chip.
    fn columns(screen: &Screen) -> ((usize, usize), (usize, usize)) {
        let left = screen
            .find("Center  packed in the middle")
            .expect("the justify column paints its label")
            .1;
        let right = screen
            .find("Center  chips in the middle")
            .expect("the align column paints its label")
            .1;
        assert!(right > left, "the two specimen columns must not overlap");
        ((left, right), (right, usize::MAX))
    }

    /// The four cross-axis specimens must place their chips at visibly
    /// different depths.
    ///
    /// A three-row specimen box with a full border leaves a single content row,
    /// which made `Start`, `Center`, `End`, and `Stretch` indistinguishable.
    #[test]
    fn the_align_specimens_place_their_chips_at_different_depths() {
        let screen = crate::screen::render_section(2, 1, Size::new(120, 70));
        let (_, right) = columns(&screen);
        let mut depths = Vec::new();
        for label in [
            "Start  chips on the top edge",
            "Center  chips in the middle",
            "End  chips on the bottom edge",
        ] {
            let (label_line, _) = screen
                .find(label)
                .unwrap_or_else(|| panic!("missing specimen `{label}`:\n{}", screen.text()));
            let (chip_line, _) = chip_in(&screen, label_line, right, CHIP)
                .unwrap_or_else(|| panic!("`{label}` painted no chip:\n{}", screen.text()));
            depths.push(chip_line - label_line);
        }
        assert_eq!(
            depths,
            vec![2, 3, 4],
            "Start, Center, and End must sit on the top, middle, and bottom content rows"
        );
        let stretched_rows = rows_with_in(&screen, right, FILLED);
        assert!(
            stretched_rows >= 2,
            "a stretched chip must fill more than one row, found {stretched_rows}:\n{}",
            screen.text()
        );
    }

    /// The justify specimens differ across the main axis in the other column.
    #[test]
    fn the_justify_specimens_place_their_chips_at_different_columns() {
        let screen = crate::screen::render_section(2, 1, Size::new(120, 70));
        let (left, right) = columns(&screen);
        let mut columns_found = Vec::new();
        for label in [
            "Start  packed at the leading edge",
            "Center  packed in the middle",
            "End  packed at the trailing edge",
        ] {
            let (label_line, _) = screen
                .find(label)
                .unwrap_or_else(|| panic!("missing specimen `{label}`:\n{}", screen.text()));
            let (_, chip_column) = chip_in(&screen, label_line, (left.0, right.0), CHIP)
                .unwrap_or_else(|| panic!("`{label}` painted no chip:\n{}", screen.text()));
            columns_found.push(chip_column - left.0);
        }
        assert!(
            columns_found[0] < columns_found[1] && columns_found[1] < columns_found[2],
            "justify must move the chips across the main axis: {columns_found:?}"
        );
    }
}
