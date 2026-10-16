//! Transparent surfaces and how they composite with the layers below.

use icmd::{
    BackgroundBlend, ComponentContext, Dimension, Edges, Layout, Node, Props, Span, Text,
    transparent, ui, view,
};

/// A transparent panel over a painted surface.
///
/// `transparent()` is the terminal's own default color, and `background_blend`
/// decides how it composites: `Cover` paints that default over the layer below,
/// while `Blend` adds nothing and leaves the lower layer visible.
pub fn transparent_panel(cx: &mut ComponentContext, props: &Props<BackgroundBlend>) -> Node {
    let theme = cx.use_theme();
    let blend = *props.data();
    let surface = theme.colors.primary;
    let surface_foreground = theme.colors.primary_foreground;
    let panel_foreground = match blend {
        BackgroundBlend::Cover => theme.colors.foreground,
        BackgroundBlend::Blend => surface_foreground,
    };
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.padding /= Edges::all(1);
            style.background /= surface;
            style.text.foreground /= surface_foreground;
        }}>
            {Text::new("a painted surface on the layer below").foreground(surface_foreground).bold()}
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.padding /= Edges::all(1);
                style.background /= transparent();
                style.background_blend /= blend;
                style.text.foreground /= panel_foreground;
            }}>
                {Text::new("a transparent panel")}
            </view>
        </view>
    }
}

/// The same value in a text slot: the terminal's default foreground.
pub fn transparent_foreground(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let surface = theme.colors.card;
    let accent = theme.colors.accent;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.padding /= Edges::all(1);
            style.background /= surface;
        }}>
            {Text::from_spans([
                Span::new("themed: ").foreground(accent).bold(),
                Span::new("accent").foreground(accent),
            ])}
            {Text::from_spans([
                Span::new("terminal default: ").foreground(accent).bold(),
                Span::new("transparent").foreground(transparent()),
            ])}
        </view>
    }
}
