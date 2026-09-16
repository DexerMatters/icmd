//! Style resolution for the commit pass: turning declared style slots into the
//! concrete text and fill the painter uses.

use crossterm::style::{Attributes, Color};

use crate::basic::common::Attr;
use crate::{Edges, ScrollbarVisibility, Style, basic::props::ScrollConfig};

use super::types::{ComputedBorder, ComputedStyle, ComputedText, ScrollSpec};

pub(super) fn slot<T: Clone>(value: &Attr<T>) -> Option<T> {
    value.clone().into()
}

pub(super) fn content_insets(border: Edges<u16>, padding: Edges<u16>) -> Edges<u16> {
    Edges {
        top: border.top.saturating_add(padding.top),
        right: border.right.saturating_add(padding.right),
        bottom: border.bottom.saturating_add(padding.bottom),
        left: border.left.saturating_add(padding.left),
    }
}

impl ComputedStyle {
    pub(super) fn resolve(style: &Style, inherited: ComputedText) -> Self {
        let text_foreground = slot(&style.text.foreground).unwrap_or(inherited.foreground);
        let text_background = slot(&style.text.background).or(inherited.background);
        let text_attributes = style.text.attr.resolve(inherited.attributes);
        let border_kind = slot(&style.border.kind);
        Self {
            layout: slot(&style.layout).unwrap_or_default(),
            width: slot(&style.width).unwrap_or_default(),
            height: slot(&style.height).unwrap_or_default(),
            line: slot(&style.line).unwrap_or_default(),
            column: slot(&style.column).unwrap_or_default(),
            margin: slot(&style.margin).unwrap_or_default(),
            padding: slot(&style.padding).unwrap_or_default(),
            gap: slot(&style.gap).unwrap_or_default(),
            justify: slot(&style.justify).unwrap_or_default(),
            align: slot(&style.align).unwrap_or_default(),
            overflow: slot(&style.overflow).unwrap_or_default(),
            overflow_x: slot(&style.overflow_x)
                .or_else(|| slot(&style.overflow))
                .unwrap_or_default(),
            overflow_y: slot(&style.overflow_y)
                .or_else(|| slot(&style.overflow))
                .unwrap_or_default(),
            visibility: slot(&style.visibility).unwrap_or_default(),
            z_index: slot(&style.z_index).unwrap_or_default(),
            background: slot(&style.background),
            fill: slot(&style.fill),
            border: ComputedBorder {
                kind: border_kind,
                edges: slot(&style.border.edges)
                    .unwrap_or_else(|| Edges::all(border_kind.is_some())),
                foreground: slot(&style.border.foreground).unwrap_or(text_foreground),
                background: slot(&style.border.background),
                attributes: style.border.attr.resolve(text_attributes),
            },
            text: ComputedText {
                foreground: text_foreground,
                background: text_background,
                attributes: text_attributes,
            },
        }
    }
}

pub(super) fn scroll_spec<'a>(config: Option<&'a ScrollConfig>) -> Option<ScrollSpec<'a>> {
    let config = config?;
    Some(ScrollSpec {
        horizontal: config.horizontal,
        vertical: config.vertical,
        always_horizontal: matches!(config.scrollbar_visibility, ScrollbarVisibility::Always)
            && config.horizontal,
        always_vertical: matches!(config.scrollbar_visibility, ScrollbarVisibility::Always)
            && config.vertical,
        draw_scrollbar: config.scrollbar_visibility != ScrollbarVisibility::Hidden,
        wheel_step: config.wheel_step,
        wheel: config.enable_wheel,
        enable_mouse: config.enable_mouse,
        enable_keyboard: config.enable_keyboard,
        controlled: config.offset.is_some(),
        requested_offset: config.offset,
        scrollbar: &config.scrollbar,
    })
}

pub(super) fn terminal_text() -> ComputedText {
    ComputedText {
        foreground: Color::Reset,
        background: None,
        attributes: Attributes::default(),
    }
}
