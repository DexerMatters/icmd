//! Scroll container that clips its content and owns a scroll offset.

use crate::{
    Attr, DomProps, Node, Overflow, Props, ScrollAxes, ScrollOffset, ScrollbarStyle,
    ScrollbarVisibility, basic::ComponentContext, basic::props::ScrollConfig, ui, view,
};

/// Configuration for [`scroll_area`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScrollAreaProps {
    /// Axes that scroll and clip; defaults to [`ScrollAxes::Vertical`].
    pub axes: Attr<ScrollAxes>,
    /// When the scrollbar is shown; defaults to
    /// [`ScrollbarVisibility::Auto`].
    pub scrollbar_visibility: Attr<ScrollbarVisibility>,
    /// Controlled scroll offset in cells; when unset the runtime owns it.
    pub offset: Attr<ScrollOffset>,
    /// Whether mouse dragging scrolls; defaults to `true`.
    pub enable_mouse: Attr<bool>,
    /// Whether the wheel scrolls; defaults to `true`.
    pub enable_wheel: Attr<bool>,
    /// Whether keyboard scrolling is handled; defaults to `true`.
    pub enable_keyboard: Attr<bool>,
    /// Cells moved per wheel notch; defaults to `1` and is floored at `1`.
    pub wheel_step: Attr<u16>,
    /// Scrollbar glyph style; defaults to the theme's scrollbar.
    pub scrollbar_style: Attr<ScrollbarStyle>,
}

/// Clipping scroll container; see [`ScrollAreaProps`] for its configuration.
///
/// Caller style is applied exactly once, by the canonical host-prop merge.
/// Scrolling owns clipping on its enabled axes; that required invariant is
/// applied after the caller merge so it cannot be overridden away.
pub fn scroll_area(cx: &mut ComponentContext, props: &Props<ScrollAreaProps>) -> Node {
    let theme = cx.use_theme();
    let axes = props.axes | ScrollAxes::Vertical;
    let scrollbar_visibility = props.scrollbar_visibility | ScrollbarVisibility::Auto;
    let horizontal = matches!(axes, ScrollAxes::Horizontal | ScrollAxes::Both);
    let vertical = matches!(axes, ScrollAxes::Vertical | ScrollAxes::Both);

    let mut dom = props.host_props(DomProps::default());
    if horizontal {
        dom.style.overflow_x = Attr::Set(Overflow::Clip);
    }
    if vertical {
        dom.style.overflow_y = Attr::Set(Overflow::Clip);
    }
    dom.scroll = Some(Box::new(ScrollConfig {
        horizontal,
        vertical,
        scrollbar_visibility,
        offset: props.offset.as_ref().copied(),
        enable_mouse: props.enable_mouse | true,
        enable_wheel: props.enable_wheel | true,
        enable_keyboard: props.enable_keyboard | true,
        wheel_step: (props.wheel_step | 1).max(1),
        scrollbar: props.data().scrollbar_style.clone() | theme.scrollbar.clone(),
    }));

    ui! { <view dom={dom}>{props.children_node()}</view> }
}
