//! Shared editorial building blocks used by every chapter.
//!
//! These helpers carry the field-guide visual language: numbered eyebrows,
//! descriptive headings, framed live demonstrations, selectable source blocks,
//! API strips, callouts, and two-column layouts that stack on narrow
//! terminals. They are presentation only; the scroll-spy wiring lives in
//! [`crate::chapters`].

use crossterm::style::Color;
use icmd::theme::Theme;
use icmd::{
    Align, BorderKind, Dimension, Edges, Justify, Layout, Node, Percent, Span, Text, TextAlign,
    TextOverflow, TextWrap, code, muted, paragraph, row, selection_area, ui, view,
};

use crate::metadata::SectionMeta;

/// Accent role for a callout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CalloutKind {
    /// Neutral explanation.
    Info,
    /// Something that commonly goes wrong.
    Warning,
    /// A note aimed at shipped software.
    Production,
}

impl CalloutKind {
    /// Uppercase heading for the callout.
    const fn title(self) -> &'static str {
        match self {
            Self::Info => "NOTE",
            Self::Warning => "WATCH FOR",
            Self::Production => "IN PRODUCTION",
        }
    }

    /// Symbol that keeps the callout meaningful without color.
    const fn glyph(self) -> &'static str {
        match self {
            Self::Info => "◆",
            Self::Warning => "▲",
            Self::Production => "▸",
        }
    }
}

/// One row of an API strip.
pub(crate) struct ApiRow {
    /// Type or widget name.
    pub name: &'static str,
    /// What it is for.
    pub purpose: &'static str,
    /// Key props and their defaults.
    pub defaults: &'static str,
    /// Events it emits.
    pub events: &'static str,
}

/// Numbered eyebrow above a section heading.
pub(crate) fn eyebrow(theme: &Theme, number: &str, kicker: &str) -> Node {
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    ui! {
        <row style={|style| { style.align /= Align::Center; style.gap /= 1; }}>
            {Text::new(number).foreground(primary).bold()}
            {Text::new(kicker).foreground(muted_foreground).bold()}
        </row>
    }
}

/// The section's descriptive heading and one-sentence promise.
pub(crate) fn section_intro(theme: &Theme, meta: &SectionMeta) -> Node {
    let number = meta.number;
    let kicker = meta.kicker;
    let title = meta.title;
    let summary = meta.summary;
    let foreground = theme.colors.foreground;
    let secondary = theme.colors.secondary;
    ui! {
        {eyebrow(theme, number, kicker)}
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>
            {Text::new(title).foreground(foreground).bold().wrap(TextWrap::Soft)}
        </view>
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.border.kind /= BorderKind::Heavy;
            style.border.edges /= Edges { top: false, right: false, bottom: false, left: true };
            style.border.foreground /= secondary;
            style.padding /= Edges { top: 0, right: 0, bottom: 0, left: 1 };
        }}>
            {Text::new(summary).foreground(secondary).bold().wrap(TextWrap::Soft)}
        </view>
    }
}

/// A body paragraph in short, readable prose.
pub(crate) fn body(text: &str) -> Node {
    let text = text.to_string();
    ui! {
        <paragraph>{Text::new(text).wrap(TextWrap::Soft)}</paragraph>
    }
}

/// Two body paragraphs in reading order.
pub(crate) fn body_pair(first: &str, second: &str) -> Node {
    let first = first.to_string();
    let second = second.to_string();
    ui! {
        {body(&first)}
        {body(&second)}
    }
}

/// Framed live demonstration with an instruction line and optional status row.
/// Keeps a demonstration out of the chapter's selection document.
///
/// A demonstration is an example, not prose, so dragging inside it must not paint
/// a selection across the surrounding text and its own text must not join the
/// chapter's document. A demonstration that is itself about selection nests its
/// own enabled region inside this barrier, and that inner region takes over its
/// subtree and stays fully selectable - which is the intended exception.
fn unselectable(body: Node) -> Node {
    ui! {
        <selection_area disabled={true} style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
        }}>{body}</selection_area>
    }
}

/// Framed interactive demonstration with a caption, an instruction, and an
/// optional status line.
pub(crate) fn live_example(
    theme: &Theme,
    caption: &str,
    instruction: &str,
    demo: Node,
    status: Option<Node>,
) -> Node {
    let caption = caption.to_string();
    let instruction = instruction.to_string();
    let primary = theme.colors.primary;
    let border = theme.colors.border;
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
            style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
            style.background /= card;
            style.text.foreground /= card_foreground;
            style.border.kind /= BorderKind::Rounded;
            style.border.foreground /= border;
            style.border.background /= card;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new("LIVE EXAMPLE").foreground(primary).bold()}
                <muted>{Text::new(caption).overflow(TextOverflow::Ellipsis)}</muted>
            </row>
            <muted>{Text::new(instruction).wrap(TextWrap::Soft)}</muted>
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
                style.padding /= Edges { top: 1, right: 0, bottom: 1, left: 0 };
                style.border.kind /= BorderKind::Single;
                style.border.edges /= Edges { top: true, right: false, bottom: false, left: false };
                style.border.foreground /= border;
            }}>
                {unselectable(demo)}
            </view>
            {status.unwrap_or_else(icmd::empty)}
        </view>
    }
}

/// Selectable Rust source rendered as a framed block.
pub(crate) fn source_block(theme: &Theme, caption: &str, source: &str) -> Node {
    let caption = caption.to_string();
    let source = source.to_string();
    let border = theme.colors.border;
    let muted_background = theme.colors.muted;
    let accent = theme.colors.accent;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
            style.background /= muted_background;
            style.border.kind /= BorderKind::Rounded;
            style.border.foreground /= border;
            style.border.background /= muted_background;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new("SOURCE").foreground(accent).bold()}
                <muted>{Text::new(caption).overflow(TextOverflow::Ellipsis)}</muted>
            </row>
            <code style={move |style| {
                style.width /= Dimension::Max;
                style.padding /= Edges::all(0);
                style.background /= muted_background;
            }}>{Text::new(source).wrap(TextWrap::Hard)}</code>
        </view>
    }
}

/// Fixed, labeled specimen that shows one behavior without controls.
pub(crate) fn specimen(theme: &Theme, caption: &str, body: Node) -> Node {
    let caption = caption.to_string();
    let border = theme.colors.border;
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let muted_foreground = theme.colors.muted_foreground;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
            style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
            style.background /= card;
            style.text.foreground /= card_foreground;
            style.border.kind /= BorderKind::Single;
            style.border.foreground /= border;
            style.border.background /= card;
        }}>
            <muted>{Text::new(caption.to_uppercase()).foreground(muted_foreground).bold().overflow(TextOverflow::Ellipsis)}</muted>
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
            }}>{unselectable(body)}</view>
        </view>
    }
}

/// "What to notice" surface: state ownership, defaults, and event flow.
pub(crate) fn notice(theme: &Theme, text: &str) -> Node {
    callout(theme, CalloutKind::Info, text)
}

/// A "Watch for" surface for likely misuse.
pub(crate) fn watch_for(theme: &Theme, text: &str) -> Node {
    callout(theme, CalloutKind::Warning, text)
}

/// A production note aimed at shipped software.
pub(crate) fn production_note(theme: &Theme, text: &str) -> Node {
    callout(theme, CalloutKind::Production, text)
}

/// A callout with a symbolic cue, a heading, and softly wrapped prose.
pub(crate) fn callout(theme: &Theme, kind: CalloutKind, text: &str) -> Node {
    let text = text.to_string();
    let accent = match kind {
        CalloutKind::Info => theme.colors.primary,
        CalloutKind::Warning => theme.colors.accent,
        CalloutKind::Production => theme.colors.secondary,
    };
    let heading = format!("{}  {}", kind.glyph(), kind.title());
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
            style.border.kind /= BorderKind::Heavy;
            style.border.edges /= Edges { top: false, right: false, bottom: false, left: true };
            style.border.foreground /= accent;
        }}>
            {Text::new(heading).foreground(accent).bold()}
            {Text::new(text).wrap(TextWrap::Soft)}
        </view>
    }
}

/// API strip listing types, key props, defaults, and emitted events.
pub(crate) fn api_strip(theme: &Theme, rows: &[ApiRow]) -> Node {
    let border = theme.colors.border;
    let muted_background = theme.colors.muted;
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let mut body = Vec::new();
    for entry in rows {
        body.push(api_row(
            primary,
            muted_foreground,
            entry.name,
            entry.purpose,
            entry.defaults,
            entry.events,
        ));
    }
    let body = body.into_iter().collect::<Node>();
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
            style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
            style.background /= muted_background;
            style.border.kind /= BorderKind::Rounded;
            style.border.foreground /= border;
            style.border.background /= muted_background;
        }}>
            {Text::new("API").foreground(primary).bold()}
            {body}
        </view>
    }
}

/// One API strip entry: name, purpose, defaults, and events.
fn api_row(
    primary: Color,
    muted_foreground: Color,
    name: &str,
    purpose: &str,
    defaults: &str,
    events: &str,
) -> Node {
    let name = name.to_string();
    let purpose = purpose.to_string();
    let detail = format!("defaults: {defaults}   ·   events: {events}");
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>
            {Text::from_spans([
                Span::new(name).foreground(primary).bold(),
                Span::new("  "),
                Span::new(purpose),
            ]).wrap(TextWrap::Soft)}
            {Text::new(detail).foreground(muted_foreground).wrap(TextWrap::Soft)}
        </view>
    }
}

/// Two labelled columns that stack below the wide breakpoint.
pub(crate) fn two_column(theme: &Theme, wide: bool, left: Node, right: Node) -> Node {
    let gap = theme.spacing.md;
    if wide {
        ui! {
            <view style={move |style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.align /= Align::Start;
                style.gap /= gap;
            }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Percent(Percent::available(50));
                    style.gap /= 1;
                }}>{left}</view>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 1;
                }}>{right}</view>
            </view>
        }
    } else {
        ui! {
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= gap;
            }}>
                {left}
                {right}
            </view>
        }
    }
}

/// Compact reference row: name and destination, then purpose.
pub(crate) fn ref_row(theme: &Theme, name: &str, purpose: &str, pointer: &str) -> Node {
    let name = name.to_string();
    let purpose = purpose.to_string();
    let pointer = pointer.to_string();
    let priority = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let border = theme.colors.border;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
            style.border.kind /= BorderKind::Single;
            style.border.edges /= Edges { top: false, right: false, bottom: true, left: false };
            style.border.foreground /= border;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new(name).foreground(priority).bold()}
                {Text::new(pointer).foreground(muted_foreground).align(TextAlign::End).wrap(TextWrap::NoWrap)}
            </row>
            <muted>{Text::new(purpose).wrap(TextWrap::Soft)}</muted>
        </view>
    }
}

/// One feature-matrix row for the installation section.
pub(crate) fn feature_row(theme: &Theme, feature: &str, purpose: &str) -> Node {
    let feature = feature.to_string();
    let purpose = purpose.to_string();
    let accent = theme.colors.accent;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>
            {Text::new(feature).foreground(accent).bold()}
            <muted>{Text::new(purpose).wrap(TextWrap::Soft)}</muted>
        </view>
    }
}

/// Closing mark for a chapter, with a pointer to the next passage.
pub(crate) fn chapter_end(theme: &Theme, number: &str, title: &str) -> Node {
    let number = number.to_string();
    let title = title.to_string();
    let primary = theme.colors.primary;
    let border = theme.colors.border;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Horizontal;
            style.width /= Dimension::Max;
            style.align /= Align::Center;
            style.justify /= Justify::SpaceBetween;
            style.gap /= 1;
            style.padding /= Edges { top: 2, right: 4, bottom: 3, left: 4 };
            style.border.kind /= BorderKind::Single;
            style.border.edges /= Edges { top: true, right: false, bottom: false, left: false };
            style.border.foreground /= border;
        }}>
            <muted>{Text::new(format!("End of chapter {number} · {title}")).wrap(TextWrap::Soft)}</muted>
            {Text::new("◆  ICMD FIELD GUIDE").foreground(primary).bold()}
        </view>
    }
}

/// A labeled inline hint row: a token followed by a muted explanation.
pub(crate) fn hint(theme: &Theme, token: &str, text: &str) -> Node {
    let primary = theme.colors.primary;
    let token = token.to_string();
    let text = text.to_string();
    ui! {
        <row style={|style| { style.align /= Align::Center; style.gap /= 1; }}>
            {Text::new(token).foreground(primary).bold()}
            <muted>{Text::new(text).wrap(TextWrap::Soft)}</muted>
        </row>
    }
}
