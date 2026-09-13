use crate::{
    Attr, DomProps, Node, Overflow, Props, ScrollAxes, ScrollOffset, ScrollbarStyle,
    ScrollbarVisibility, basic::ComponentContext, basic::props::ScrollConfig, ui, view,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScrollAreaProps {
    pub axes: Attr<ScrollAxes>,
    pub scrollbar_visibility: Attr<ScrollbarVisibility>,
    pub offset: Attr<ScrollOffset>,
    pub enable_mouse: Attr<bool>,
    pub enable_wheel: Attr<bool>,
    pub enable_keyboard: Attr<bool>,
    pub wheel_step: Attr<u16>,
    pub scrollbar_style: Attr<ScrollbarStyle>,
}

pub fn scroll_area(cx: &mut ComponentContext, props: &Props<ScrollAreaProps>) -> Node {
    let theme = cx.use_theme();
    let axes = props.axes | ScrollAxes::Vertical;
    let scrollbar_visibility = props.scrollbar_visibility | ScrollbarVisibility::Auto;
    let horizontal = matches!(axes, ScrollAxes::Horizontal | ScrollAxes::Both);
    let vertical = matches!(axes, ScrollAxes::Vertical | ScrollAxes::Both);

    // Caller style is applied exactly once, by the canonical host-prop merge.
    // The previous code cloned it into the defaults and then merged it again,
    // which both duplicated work and made precedence depend on merge order.
    let mut dom = props.host_props(DomProps::default());
    // Scrolling owns clipping on its enabled axes. This required invariant is
    // applied after the caller merge so it cannot be overridden away.
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
        scrollbar: props.user_defined.scrollbar_style.clone() | theme.scrollbar.clone(),
    }));

    ui! { <view dom={dom}>{props.children_node()}</view> }
}
