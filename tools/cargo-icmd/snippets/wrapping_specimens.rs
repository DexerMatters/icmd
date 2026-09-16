//! Fixed side-by-side specimens for wrap, align, and truncation.

use icmd::{
    ComponentContext, Dimension, Edges, Node, Props, Text, TextAlign, TextOverflow, TextWrap, card,
    muted, ui, view,
};

/// Three wrap modes and two truncation modes, shown without any controls.
pub fn wrapping_specimens(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    const SENTENCE: &str = "the quick brown fox jumps over the lazy dog";
    let modes = [
        ("NoWrap", TextWrap::NoWrap),
        ("Soft", TextWrap::Soft),
        ("Hard", TextWrap::Hard),
    ];
    let specimens = modes
        .iter()
        .map(|(label, wrap)| {
            ui! {
                <view style={|style| {
                    style.layout /= icmd::Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                    style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
                }}>
                    <muted>{Text::new(*label)}</muted>
                    {Text::new(SENTENCE).wrap(*wrap).overflow(TextOverflow::Clip)}
                </view>
            }
        })
        .collect::<Node>();

    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <view style={|style| { style.width /= Dimension::Cells(28); }}>
                {specimens}
            </view>
            <view style={|style| { style.width /= Dimension::Max; style.gap /= 0; }}>
                <muted>"Clip keeps the text inside its box"</muted>
                {Text::new(SENTENCE).wrap(TextWrap::NoWrap).overflow(TextOverflow::Clip)}
                <muted>"Ellipsis marks the truncation"</muted>
                {Text::new(SENTENCE).wrap(TextWrap::NoWrap).overflow(TextOverflow::Ellipsis)}
                <muted>"Center alignment inside a fixed box"</muted>
                {Text::new("centered").align(TextAlign::Center).wrap(TextWrap::NoWrap)}
                <muted>"End alignment inside a fixed box"</muted>
                {Text::new("right").align(TextAlign::End).wrap(TextWrap::NoWrap)}
            </view>
        </card>
    }
}
