use crate::{
    Attr, DomProps, Node, Overflow, Props, ScrollAxes, ScrollOffset, ScrollbarStyle,
    ScrollbarVisibility, basic::ComponentContext, basic::props::ScrollConfig, theme::Theme, ui,
    view,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScrollAreaProps {
    /// Axes that may be scrolled. Defaults to vertical scrolling.
    pub axes: Attr<ScrollAxes>,
    /// Whether bars are automatic, reserved even without overflow, or hidden.
    pub scrollbar_visibility: Attr<ScrollbarVisibility>,
    /// When set, the caller owns the scroll position and should feed updates from `on_scroll` back.
    pub offset: Attr<ScrollOffset>,
    /// Enable pointer interactions with bars and the scroll area.
    pub enable_mouse: Attr<bool>,
    /// Enable wheel input independently of pointer interactions.
    pub enable_wheel: Attr<bool>,
    /// Enable arrows, Home/End, and page-key scrolling independently.
    pub enable_keyboard: Attr<bool>,
    /// Number of logical cells moved by one wheel tick.
    pub wheel_step: Attr<u16>,
    /// Optional per-instance override for the theme's scrollbar appearance.
    pub scrollbar_style: Attr<ScrollbarStyle>,
}

pub fn scroll_area(cx: &mut ComponentContext, props: &Props<ScrollAreaProps>) -> Node {
    let theme: Theme = cx.use_theme();
    let axes = props.axes | ScrollAxes::Vertical;
    let scrollbar_visibility = props.scrollbar_visibility | ScrollbarVisibility::Auto;
    let horizontal = matches!(axes, ScrollAxes::Horizontal | ScrollAxes::Both);
    let vertical = matches!(axes, ScrollAxes::Vertical | ScrollAxes::Both);

    let mut style = props.dom.style.clone();
    if horizontal {
        style.overflow_x /= Overflow::Clip;
    }
    if vertical {
        style.overflow_y /= Overflow::Clip;
    }

    let mut dom = props.host_props(DomProps {
        style,
        ..DomProps::default()
    });
    // Scrolling owns clipping on its enabled axes.  Preserve every other
    // caller style while preventing a later override from leaking content.
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
        scrollbar: props.user_defined.scrollbar_style.clone() | theme.scrollbar,
    }));

    ui! { <view dom={dom}>{props.children_node()}</view> }
}
