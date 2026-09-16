//! Aligned rows proving that Unicode occupies cells, not bytes.

use icmd::{
    ComponentContext, Dimension, Edges, Node, Props, Text, TextOverflow, card, muted, row, ui, view,
};

/// Four specimens in fixed columns: the cell grid, not the byte length, aligns them.
pub fn unicode_rows(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    const SAMPLES: [(&str, &str, &str); 4] = [
        ("ascii", "terminal", "8"),
        ("cjk", "端末アプリ", "10"),
        ("combining", "e\u{0301}le\u{0300}ve", "5"),
        ("emoji", "🚀 ship it", "11"),
    ];

    let rows = SAMPLES
        .iter()
        .map(|(label, sample, cells)| {
            ui! {
                <view style={|style| {
                    style.layout /= icmd::Layout::Horizontal;
                    style.width /= Dimension::Max;
                    style.gap /= 1;
                    style.padding /= Edges::symmetric(0, 1);
                }}>
                    <view style={|style| { style.width /= Dimension::Cells(12); }}>
                        {Text::new(*label).overflow(TextOverflow::Ellipsis)}
                    </view>
                    <view style={|style| { style.width /= Dimension::Cells(20); }}>
                        {Text::new(*sample)}
                    </view>
                    <view style={|style| { style.width /= Dimension::Cells(8); }}>
                        <muted>{Text::new(format!("{cells} cells"))}</muted>
                    </view>
                </view>
            }
        })
        .collect::<Node>();

    ui! {
        <card style={|style| { style.gap /= 0; }}>
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 1; }}>
                <muted>"sample"</muted>
                <muted>"rendered"</muted>
                <muted>"width"</muted>
            </row>
            {rows}
            <muted>"A wide cell still occupies two columns, and a combining mark none."</muted>
        </card>
    }
}
