//! Chapter 07 — Scroll, Selection, and Raster Media.
//!
//! Interaction that depends on committed geometry: scroll offsets and
//! scrollbars, selection inside a scroller, and images on a cell grid.

use icmd::advanced::ResourceLimits;
use icmd::theme::Theme;
use icmd::widgets::raster_image;
use icmd::{
    Align, BadgeVariant, BorderKind, ButtonVariant, Component, ComponentContext, Dimension, Edges,
    ImageAlign, ImageFit, ImageLoading, ImageMode, ImageSource, Justify, Layout, Node, Overflow,
    Props, RasterImage, ScrollAxes, ScrollEvent, ScrollOffset, ScrollbarVisibility, Span, Text,
    TextSelectionEvent, TextWrap, badge, button, card, divider, empty, muted, paragraph, row,
    scroll_area, selection_area, ui, view,
};

use super::{ChapterProps, document, masthead, route_card, section};
use crate::docs::{self, ApiRow, CalloutKind};
use crate::metadata::{self, SectionMeta};
use crate::snippets;

/// Build-log lines, wider than most boxes so the horizontal axis is real.
fn log_lines(count: usize) -> Node {
    (1..=count)
        .map(|line| {
            ui! {
                <muted>{Text::new(format!(
                    "step {line:02} · compile · link · commit · sign · publish · verify · notify",
                ))
                .wrap(TextWrap::NoWrap)}</muted>
            }
        })
        .collect::<Node>()
}

/// A numbered grid, wide and tall enough to scroll on both axes.
fn grid_lines(count: usize) -> Node {
    (1..=count)
        .map(|line| {
            ui! {
                <muted>{Text::new(format!("row {line:02} │ {}end", "▪ ".repeat(24)))
                    .wrap(TextWrap::NoWrap)}</muted>
            }
        })
        .collect::<Node>()
}

/// Release notes: a version line and a sentence, in paint order.
fn release_notes() -> Node {
    const NOTES: [(&str, &str); 5] = [
        (
            "0.4.2",
            "Selection follows painted content, so a drag started after a scroll resolves the cells the reader sees.",
        ),
        (
            "0.4.1",
            "Scroll areas report the position they accepted through on_scroll, which makes a controlled offset possible.",
        ),
        (
            "0.4.0",
            "The image manager keys file sources by canonical path, so two spellings share one decoded entry.",
        ),
        (
            "0.3.9",
            "Skeletons and spinners are ordinary state, and nothing advances a frame on its own.",
        ),
        (
            "0.3.8",
            "A failed decode keeps its reserved box and paints a placeholder instead of collapsing the layout.",
        ),
    ];
    let notes = NOTES
        .iter()
        .map(|(version, note)| {
            ui! {
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                }}>
                    {Text::new(*version).bold()}
                    <paragraph>{Text::new(*note).wrap(TextWrap::Soft)}</paragraph>
                </view>
            }
        })
        .collect::<Node>();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>{notes}</view>
    }
}

/// A one-line excerpt of selected text, collapsed and truncated.
fn excerpt(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= 56 {
        collapsed
    } else {
        let head: String = collapsed.chars().take(56).collect();
        format!("{head}…")
    }
}

/// Tall and wide content in bounded boxes, one per axis, plus a plain clip.
fn scroll_specimen(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let border = theme.colors.border;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <muted>"vertical axis · ScrollAxes::Vertical · Auto scrollbar · wheel_step 2"</muted>
            <scroll_area
                axes={ScrollAxes::Vertical}
                scrollbar_visibility={ScrollbarVisibility::Auto}
                wheel_step={2_u16}
                style={move |style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(5);
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                }}>{log_lines(12)}</view>
            </scroll_area>
            <muted>"horizontal axis · ScrollAxes::Horizontal · Always scrollbar"</muted>
            <scroll_area
                axes={ScrollAxes::Horizontal}
                scrollbar_visibility={ScrollbarVisibility::Always}
                style={move |style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(4);
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.gap /= 0;
                }}>{log_lines(3)}</view>
            </scroll_area>
            <muted>"both axes · ScrollAxes::Both · Hidden scrollbar · wheel_step 3"</muted>
            <scroll_area
                axes={ScrollAxes::Both}
                scrollbar_visibility={ScrollbarVisibility::Hidden}
                wheel_step={3_u16}
                style={move |style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(5);
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.gap /= 0;
                }}>{grid_lines(12)}</view>
            </scroll_area>
            <muted>"no scroller at all · a fixed height with Overflow::Clip simply trims the extra lines"</muted>
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.height /= Dimension::Cells(3);
                style.overflow /= Overflow::Clip;
                style.border.kind /= BorderKind::Single;
                style.border.foreground /= border;
            }}>{log_lines(6)}</view>
        </view>
    }
}

/// A two-axis build log whose offset the application owns, plus a readout.
fn offsets_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let (view_state, set_view) =
        cx.use_state(|| (ScrollOffset::default(), ScrollOffset::default()));
    let (offset, max_offset) = view_state;
    let on_scroll = set_view.clone();
    let reset = set_view;
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let border = theme.colors.border;
    ui! {
        <view style={|style| {
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
                {Text::from_spans([
                    Span::new("offset ").foreground(primary).bold(),
                    Span::new(format!("x {}  y {}", offset.x, offset.y)).bold(),
                    Span::new(format!("   max x {}  y {}", max_offset.x, max_offset.y))
                        .foreground(muted_foreground),
                ])}
                <button variant={ButtonVariant::Secondary}
                    on_press={move |_| reset.update(|value| *value = (ScrollOffset::default(), value.1))}>"reset offset"</button>
            </row>
            <muted>"controlled · offset is supplied, and on_scroll reports the position the runtime accepted"</muted>
            <scroll_area
                axes={ScrollAxes::Both}
                offset={offset}
                wheel_step={3_u16}
                on_scroll={move |event: ScrollEvent| on_scroll.set((event.offset, event.max_offset))}
                style={move |style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(6);
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.gap /= 0;
                }}>{grid_lines(14)}</view>
            </scroll_area>
            <muted>"runtime-owned · no offset prop, so the runtime keeps the position and this component never sees it"</muted>
            <scroll_area
                axes={ScrollAxes::Both}
                wheel_step={3_u16}
                style={move |style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(4);
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.gap /= 0;
                }}>{grid_lines(10)}</view>
            </scroll_area>
        </view>
    }
}

/// A fixed header above a scrolling body inside one card.
fn nesting_specimen(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let border = cx.use_theme().colors.border;
    ui! {
        <card style={move |style| {
            style.gap /= 0;
            style.border.foreground /= border;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new("build 42 · 128 targets").bold()}
                <badge text={"3 failed"} variant={BadgeVariant::Destructive} />
            </row>
            <divider />
            <scroll_area
                axes={ScrollAxes::Vertical}
                scrollbar_visibility={ScrollbarVisibility::Auto}
                style={|style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(5);
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                }}>{log_lines(12)}</view>
            </scroll_area>
        </card>
    }
}

/// Release notes inside a scroller, with the selected range reported below.
fn selection_scroll_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let (report, set_report) = cx.use_state(|| None::<TextSelectionEvent>);
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let border = theme.colors.border;
    let readout = match &report {
        Some(event) => format!(
            "{} characters · bytes {}..{}",
            event.text.chars().count(),
            event.range.start,
            event.range.end,
        ),
        None => String::from("nothing selected yet"),
    };
    let preview = report
        .as_ref()
        .map(|event| excerpt(&event.text))
        .unwrap_or_default();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <muted>"scroll the notes first, then drag across a line; the range is in document bytes"</muted>
            <scroll_area
                axes={ScrollAxes::Vertical}
                scrollbar_visibility={ScrollbarVisibility::Always}
                style={move |style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(6);
                    style.border.kind /= BorderKind::Single;
                    style.border.foreground /= border;
                }}>
                <selection_area
                    on_selection_change={move |event: TextSelectionEvent| set_report.set(Some(event))}
                    style={|style| {
                        style.layout /= Layout::Vertical;
                        style.width /= Dimension::Max;
                        style.gap /= 0;
                    }}>{release_notes()}</selection_area>
            </scroll_area>
            {Text::from_spans([
                Span::new("selection ").foreground(primary).bold(),
                Span::new(readout).foreground(muted_foreground),
            ]).wrap(TextWrap::Soft)}
            {if preview.is_empty() {
                empty()
            } else {
                ui! { <muted>{Text::new(preview).wrap(TextWrap::Soft)}</muted> }
            }}
        </view>
    }
}

/// Loaded and file-backed sources, with and without explicit dimensions.
fn sources_specimen(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let image = cx.use_memo((), || crate::assets::guide_image().ok());
    let Some(image) = image else {
        return ui! { <muted>"the bundled guide.png could not be decoded"</muted> };
    };
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <muted>{Text::new("loaded · ImageSource::loaded · the 144 x 96 pixel source derives an 18 x 6 cell box").wrap(TextWrap::Soft)}</muted>
            <raster_image
                src={ImageSource::loaded(image.clone())}
                mode={ImageMode::Symbols} />
            <muted>{Text::new("file · ImageSource::file with width 18 and height 6 · this path does not exist, so the same box paints its alt label").wrap(TextWrap::Soft)}</muted>
            <raster_image
                src={ImageSource::file("/srv/artifacts/guide.png")}
                width={18_u16}
                height={6_u16}
                loading={ImageLoading::Eager}
                alt={"guide.png"}
                mode={ImageMode::Symbols} />
            <muted>{Text::new("file · no explicit size · refused before any load, so the widget paints a 1 x 1 × instead; a label needs a box with room for it").wrap(TextWrap::Soft)}</muted>
            <raster_image
                src={ImageSource::file("/srv/artifacts/guide.png")}
                mode={ImageMode::Symbols} />
        </view>
    }
}

/// One labeled fit example over a reserved cell box.
fn fit_example(
    theme: &Theme,
    caption: &str,
    image: &RasterImage,
    fit: ImageFit,
    mode: ImageMode,
) -> Node {
    let caption = caption.to_string();
    let image = image.clone();
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::all(1);
            style.border.kind /= BorderKind::Single;
            style.border.foreground /= theme.colors.border;
        }}>
            <muted>{Text::new(caption).wrap(TextWrap::Soft)}</muted>
            <raster_image
                src={ImageSource::loaded(image)}
                width={22_u16}
                height={9_u16}
                fit={fit}
                mode={mode} />
        </view>
    }
}

/// One labeled horizontal-alignment example in a wide, short box.
fn align_example(
    theme: &Theme,
    caption: &str,
    image: &RasterImage,
    horizontal: ImageAlign,
) -> Node {
    let caption = caption.to_string();
    let image = image.clone();
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::all(1);
            style.border.kind /= BorderKind::Single;
            style.border.foreground /= theme.colors.border;
        }}>
            <muted>{Text::new(caption).wrap(TextWrap::Soft)}</muted>
            <raster_image
                src={ImageSource::loaded(image)}
                width={14_u16}
                height={5_u16}
                fit={ImageFit::Contain}
                horizontal_align={horizontal}
                mode={ImageMode::Symbols} />
        </view>
    }
}

/// The three fits, three alignments, and the two explicit render modes.
fn fit_specimen(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let image = cx.use_memo((), || crate::assets::guide_image().ok());
    let Some(image) = image else {
        return ui! { <muted>"the bundled guide.png could not be decoded"</muted> };
    };
    let aligns = [
        ("align Start", ImageAlign::Start),
        ("align Center", ImageAlign::Center),
        ("align End", ImageAlign::End),
    ]
    .iter()
    .map(|(caption, align)| align_example(&theme, caption, &image, *align))
    .collect::<Node>();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <muted>"the three fits, forced into Symbols mode so every terminal shows the same geometry"</muted>
            {fit_example(&theme, "contain · the whole image inside the box, aspect preserved", &image, ImageFit::Contain, ImageMode::Symbols)}
            {fit_example(&theme, "cover · fills the box, cropping the excess", &image, ImageFit::Cover, ImageMode::Symbols)}
            {fit_example(&theme, "stretch · exactly the box, aspect ratio ignored", &image, ImageFit::Stretch, ImageMode::Symbols)}
            <muted>"horizontal alignment in a box wider than the fitted image"</muted>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>{aligns}</row>
            {fit_example(&theme, "mode Auto · negotiate Kitty, Sixel, iTerm2, then fall back to symbols", &image, ImageFit::Contain, ImageMode::Auto)}
            {fit_example(&theme, "mode Symbols · always text cells, whatever the terminal supports", &image, ImageFit::Contain, ImageMode::Symbols)}
        </view>
    }
}

/// Cache identity, budgets, and the error placeholder, computed live.
fn shipping_specimen(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let image = cx.use_memo((), || crate::assets::guide_image().ok());
    let limits = ResourceLimits::default();
    let loaded_key = image
        .as_ref()
        .map(|image| format!("{:?}", ImageSource::loaded(image.clone()).cache_key()))
        .unwrap_or_else(|| String::from("no decoded image"));
    let file_key = format!(
        "{:?}",
        ImageSource::file("/srv/artifacts/guide.png").cache_key()
    );
    let mib = 1024 * 1024;
    let budget = format!(
        "encoded {} MiB · source {} Mpx · decoded {} MiB · cache {} MiB",
        limits.max_encoded_image_bytes / mib,
        limits.max_source_pixels / mib as u64,
        limits.max_decoded_image_bytes / mib,
        limits.max_cache_bytes / mib,
    );
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                {Text::new("loaded key").foreground(primary).bold()}
                <muted>{Text::new(loaded_key).wrap(TextWrap::Soft)}</muted>
            </row>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 1;
            }}>
                {Text::new("file key").foreground(primary).bold()}
                <muted>{Text::new(file_key).wrap(TextWrap::Soft)}</muted>
            </row>
            {Text::from_spans([
                Span::new("default budgets ").foreground(primary).bold(),
                Span::new(budget).foreground(muted_foreground),
            ]).wrap(TextWrap::Soft)}
            <muted>"a source that cannot decode keeps its reserved box and paints its alt label:"</muted>
            <raster_image
                src={ImageSource::file("/srv/artifacts/missing.png")}
                width={12_u16}
                height={3_u16}
                alt={"missing.png"}
                mode={ImageMode::Symbols} />
        </view>
    }
}

/// Chapter 07.
pub(super) fn scroll_selection_media(
    cx: &mut ComponentContext,
    props: &Props<ChapterProps>,
) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(6);
    let sections: &[SectionMeta] = meta.sections;

    let scroll_section = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body("A scroll area is a clipped viewport. It takes the axes it scrolls and clips on, when its scrollbar appears, and how many cells one wheel notch moves. The enabled axes are clipped by the container itself, so content can never paint past the box.")}
            {docs::body("Three defaults are worth remembering. Scrollbar visibility starts at Auto, which draws a track only while the region is scrollable. wheel_step is one cell and is floored at one. And clipping is a property of the box, not of scrolling: a fixed height with Overflow::Clip trims content with no scroll state at all.")}
            {docs::specimen(&theme, "one box per axis, plus a plain clip", ui! { {scroll_specimen.apply(())} })}
            {docs::notice(&theme, "Scroll state lives on the host element, not on the content. Content that changes height does not need to know its offset, because the container recomputes the maximum from the committed frame.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "scroll_area", purpose: "clipping viewport with scrollbar and wheel handling", defaults: "vertical, Auto scrollbar, wheel_step 1", events: "on_scroll, on_element_change" },
                ApiRow { name: "ScrollAreaProps", purpose: "axes, visibility, offset, mouse/wheel/keyboard, wheel_step", defaults: "all enabled, offset runtime-owned", events: "on_scroll" },
                ApiRow { name: "ScrollAxes", purpose: "Vertical, Horizontal, Both", defaults: "Vertical", events: "none" },
                ApiRow { name: "ScrollbarVisibility", purpose: "Auto, Always, Hidden", defaults: "Auto", events: "none" },
                ApiRow { name: "ScrollbarStyle", purpose: "track and thumb glyphs and styles", defaults: "the theme's scrollbar", events: "none" },
            ])}
        },
    );

    let offsets_section = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body("An offset prop is an ownership decision. Supply offset and the application owns the scroll position: the runtime applies exactly what you pass and reports accepted positions through on_scroll. Omit offset and the runtime owns it, remembers the position across renders, and still reports changes.")}
            {docs::body("The trade is direct. Owning the offset lets you persist, clamp, animate, or restore it, and it makes a readout like the one below possible. Letting the runtime own it keeps the component smaller and avoids a state update for every wheel notch.")}
            {docs::live_example(
                &theme,
                "controlled and runtime-owned offsets",
                "Scroll the first box and watch x and y; then scroll the second, which reports nothing because this component never owns it.",
                ui! { {offsets_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "round trip", "a controlled offset must feed on_scroll back into state, or the next render snaps the region back")} }),
            )}
            {docs::notice(&theme, "ScrollEvent carries the offset that was accepted, the largest offset the region allows, and the delta that produced it. Reporting the accepted position rather than the requested one is what keeps a controlled offset honest.")}
            {docs::watch_for(&theme, "Because scroll events bubble, a nested region raises its own event at every ancestor with an `on_scroll`. A controlled offset must ignore events that are not its own - compare the event's `max_offset` with the range your region committed - or the outer region will adopt the inner region's position.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ScrollOffset", purpose: "x and y in cells", defaults: "0, 0", events: "none" },
                ApiRow { name: "ScrollEvent", purpose: "offset, max_offset, and delta", defaults: "—", events: "none" },
                ApiRow { name: "ScrollDelta", purpose: "signed movement in cells", defaults: "0, 0", events: "none" },
                ApiRow { name: "scroll_area", purpose: "omit offset to let the runtime own it", defaults: "runtime-owned", events: "on_scroll" },
            ])}
        },
    );

    let nesting_section = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body("Scroll areas nest. The innermost scrollable region under the pointer takes the wheel, and once it reaches its end the outer region continues; keyboard scrolling follows focus instead. Bounded heights are what make that predictable, because a region with an auto height grows to its content and then competes with its parent for space.")}
            {docs::body("Scroll events bubble. A region raises the event with its own offsets, and every ancestor with an `on_scroll` sees it - which is how a scroll host can wrap the region it observes, and also how a controlled ancestor can be broken by a descendant.")}
            {docs::body("Application chrome belongs outside the document scroller. A title bar, a toolbar, or a status row that lives inside scrolls away, and the reader loses the controls that would bring it back. The guide's own shell keeps its header and index outside the document column for exactly that reason.")}
            {docs::specimen(&theme, "fixed header above a scrolling body", ui! { {nesting_specimen.apply(())} })}
            {docs::watch_for(&theme, "A controlled offset must check that an event describes its own region before adopting it. This guide's document compares the event's `max_offset` with the range its viewport committed: without that check, scrolling the build log in section 7.2 also handed the document the log's offset, and the page jumped back toward the top on every notch.")}
            {docs::watch_for(&theme, "Two scrollers on the same axis with auto heights are the usual bug: neither can compute a maximum, so neither scrolls and both clip. Give each nested region a height in cells or a share of its parent.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "scroll_area", purpose: "nested regions scroll inner first, then outer", defaults: "wheel enabled, wheel_step 1", events: "on_scroll" },
                ApiRow { name: "ScrollEvent", purpose: "the offset, the range, and the delta that produced it", defaults: "bubbles to ancestors", events: "on_scroll" },
                ApiRow { name: "Dimension", purpose: "Cells, Percent, Max, or Auto for a bounded region", defaults: "Auto", events: "none" },
                ApiRow { name: "ElementRef", purpose: "read committed geometry for a custom scroll policy", defaults: "empty until commit", events: "on_element_change" },
            ])}
        },
    );

    let selection_section = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body("A selection area makes its painted text a document. Hit testing runs against the committed frame rather than against the widget's own coordinates, so a selection placed after a scroll still lands on the glyph the reader pointed at.")}
            {docs::body("The reported TextSelectionEvent carries a byte range into that document - the region's text leaves concatenated in paint order - plus the sliced text, so a range that spans blocks includes the newline between them.")}
            {docs::live_example(
                &theme,
                "selection inside a scroller",
                "Scroll the notes, then drag across a line; the range and an excerpt appear below.",
                ui! { {selection_scroll_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "paint order", "the document is rebuilt from what was painted, so a selection is always clamped into the current text")} }),
            )}
            {docs::notice(&theme, "Keyboard selection is opt-in per region and on by default; disabling it leaves pointer selection working, but the region no longer answers Ctrl+C. Focus follows the scroll container, which is why a region inside a scroller can still be selected after the pointer moves.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "selection_area", purpose: "make its painted subtree selectable", defaults: "keyboard selection on, focusable", events: "on_selection_change, on_clipboard" },
                ApiRow { name: "SelectionAreaProps", purpose: "styles, disabled, autofocus, enable_keyboard", defaults: "theme selection styles", events: "on_selection_change" },
                ApiRow { name: "TextSelectionEvent", purpose: "byte range plus the sliced text", defaults: "—", events: "none" },
                ApiRow { name: "scroll_area", purpose: "keeps the selectable region in a bounded viewport", defaults: "vertical, Auto scrollbar", events: "on_scroll" },
            ])}
        },
    );

    let sources_section = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body("ImageSource says where pixels come from. ImageSource::loaded wraps an image already in memory: decoded once, cloned cheaply, and never re-read. ImageSource::file stores a path that is loaded on demand through the same budgets as any other decode.")}
            {docs::body("A loaded source can derive its cell box from its pixel size. A file source cannot, because nothing has been decoded when the widget lays out, so the widget requires an explicit width and height and treats a zero on either axis as an invalid source.")}
            {docs::body("alt is what a box paints while its source cannot be shown, such as a missing file or a failed decode. A short label - a title, a filename, a one-line description - keeps a box that has already reserved its size readable, and a label too long for the box is clipped with an ellipsis. Without alt the box keeps the generic × placeholder.")}
            {docs::body("ImageLoading chooses when a file source is decoded. Lazy waits until the surface first needs pixels, which keeps an off-screen image cheap. Eager decodes as soon as the surface is created, which is what you want when the image is about to be shown. ResourceLimits bounds every path: encoded bytes, source dimensions, source pixels, decoded bytes, and bytes in flight.")}
            {docs::specimen(&theme, "loaded, file-backed, and refused sources", ui! { {sources_specimen.apply(())} })}
            {docs::callout(&theme, CalloutKind::Warning, "A file-backed source with only one explicit dimension is refused, not guessed. The widget has no pixels to measure, so it cannot infer the other axis, and it paints the invalid-source placeholder instead.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "raster_image", purpose: "place a raster source in a cell box", defaults: "Lazy, Contain, Center, Auto", events: "none" },
                ApiRow { name: "ImageProps", purpose: "src, width, height, loading, fit, alignment, mode, alt", defaults: "width and height 0, so a loaded source derives them", events: "none" },
                ApiRow { name: "alt", purpose: "label painted while the source cannot be shown", defaults: "× placeholder", events: "none" },
                ApiRow { name: "ImageSource", purpose: "Loaded(image) or File(path)", defaults: "unset renders an empty element", events: "none" },
                ApiRow { name: "ImageLoading", purpose: "when a file source is decoded", defaults: "Lazy", events: "none" },
                ApiRow { name: "RasterImage", purpose: "decoded RGBA8 pixels plus id, width, and height", defaults: "built by decode, open, or from_rgba8", events: "none" },
                ApiRow { name: "ResourceLimits", purpose: "hard ceilings every decode is checked against", defaults: "32 MiB encoded, 64 Mpx source, 256 MiB decoded", events: "none" },
            ])}
        },
    );

    let fit_section = section(
        &theme,
        data,
        5,
        &sections[5],
        ui! {
            {docs::body("Fit decides how the source maps into the cell box. Contain keeps the whole image inside and preserves the aspect ratio. Cover fills the box and crops what does not fit. Stretch maps to exactly the box and ignores the aspect ratio.")}
            {docs::body("Alignment decides where a fitted image sits when it is smaller than the box on an axis: Start, Center, or End. It applies per axis through horizontal_align and vertical_align.")}
            {docs::body("Mode decides what is drawn. Auto negotiates with the terminal: Kitty, Sixel, or iTerm2 is used natively when the terminal supports it, and symbols are the fallback. Native asks for a graphics protocol only. Symbols always emits text cells, which is the one mode every terminal can show.")}
            {docs::specimen(&theme, "fits, alignments, and modes over one image", ui! { {fit_specimen.apply(())} })}
            {docs::source_block(&theme, "the same image through every fit and mode, and a labelled placeholder", snippets::IMAGE_GALLERY)}
            {docs::notice(&theme, "ImageProtocol names a preferred protocol explicitly when Auto is not the choice you want; ImageProtocol::Symbols is equivalent to forcing symbol output. The renderer decides the protocol once per session, so a resize or a repaint never changes modes mid-frame.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ImageFit", purpose: "Contain, Cover, Stretch", defaults: "Contain", events: "none" },
                ApiRow { name: "ImageAlign", purpose: "Start, Center, End on each axis", defaults: "Center", events: "none" },
                ApiRow { name: "ImageMode", purpose: "Auto, Native, Symbols", defaults: "Auto", events: "none" },
                ApiRow { name: "ImageProtocol", purpose: "Auto, Kitty, Sixel, Iterm2, Symbols", defaults: "Auto", events: "none" },
                ApiRow { name: "ImageRenderOptions", purpose: "fit, both alignments, and mode in one value", defaults: "Contain, Center, Center, Auto", events: "none" },
            ])}
        },
    );

    let shipping_section = section(
        &theme,
        data,
        6,
        &sections[6],
        ui! {
            {docs::body("Cache identity comes from the source, not from the widget. A loaded source is keyed by its image id; a file source is keyed by its canonical path when the filesystem can resolve one, so two spellings of the same file share a single decoded entry. ImageSourceKey is that identity.")}
            {docs::body("Explicit sizing is what makes a file-backed source safe to place. State both dimensions and treat the box as reserved space: when a load fails, the renderer keeps the box and paints the × placeholder instead of collapsing the layout around it.")}
            {docs::body("Budgets are part of the same story. RuntimeConfig::image_cache_bytes bounds the decoded images the renderer retains, defaulting to 64 MiB, and the stricter of it and ResourceLimits::max_cache_bytes wins. Terminal capability varies, so anything that must look the same everywhere cannot depend on a graphics protocol.")}
            {docs::body("This guide bundles one original 144 x 96 asset with include_bytes! and decodes it through crate::assets::guide_image, so the media chapter never depends on a path or a network request. The keys and budgets below are read from the live runtime policy.")}
            {docs::specimen(&theme, "identity, budgets, and a failing source", ui! { {shipping_specimen.apply(())} })}
            {docs::callout(&theme, CalloutKind::Production, "Check the placeholder before you ship. A slow or failing terminal shows the reserved box with the × inside it, so reserve a box that reads as intentional rather than as a defect.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ImageSourceKey", purpose: "cache identity: Loaded(id) or File(canonical path)", defaults: "derived from the source", events: "none" },
                ApiRow { name: "ImageSource", purpose: "explicit dimensions are required for File", defaults: "Loaded reads pixels, File reads a path", events: "none" },
                ApiRow { name: "RuntimeConfig::image_cache_bytes", purpose: "decoded image bytes retained by the renderer", defaults: "64 MiB", events: "none" },
                ApiRow { name: "ResourceLimits", purpose: "per-decode ceilings for bytes, dimensions, and pixels", defaults: "see RuntimeConfig::limits", events: "none" },
                ApiRow { name: "RasterImageError", purpose: "reported load and decode failures", defaults: "—", events: "none" },
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
                {scroll_section}
                {offsets_section}
                {nesting_section}
                {selection_section}
                {sources_section}
                {fit_section}
                {shipping_section}
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 1;
                }}>
                    {route_card(&theme, data, 7, "◆", "08 · Themes", "The nineteen roles, eight presets, derivation, and contrast.")}
                </view>
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}

#[cfg(test)]
mod tests {
    use icmd::Size;

    /// A wheel gesture over a chapter that contains nested scroll areas must
    /// move the page monotonically.
    ///
    /// Scroll events bubble, so a demonstration's own build log raises an event
    /// at the document too. Adopting that offset as the document's made the page
    /// jump back toward the top on every notch, which is exactly what this test
    /// pins down: the sequence below used to read `2, 4, 6, 8, 10, 2, 4, ...`.
    #[test]
    fn wheel_scrolling_a_nested_region_never_resets_the_document() {
        for at in [(60_u16, 20_u16), (60, 30), (40, 15)] {
            let observed = crate::screen::drive_chapter_wheel(6, Size::new(120, 40), at, 10);
            let offsets: Vec<u32> = observed.iter().map(|(offset, _)| offset.y).collect();
            assert!(
                offsets.windows(2).all(|pair| pair[1] >= pair[0]),
                "wheel-down must never scroll the document back toward the top at {at:?}: {offsets:?}"
            );
            assert!(
                offsets.last().copied().unwrap_or(0) > 0,
                "the wheel must actually scroll the document at {at:?}: {offsets:?}"
            );
        }
    }
}
