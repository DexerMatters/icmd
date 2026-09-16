//! A live geometry readout driven by committed element snapshots.

use icmd::{ComponentContext, Dimension, ElementSnapshot, Node, Props, Text, card, muted, row, ui};

/// Reads the committed rectangles without making them render state of the box.
pub fn geometry_readout(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let measured = cx.use_element_ref();
    let (report, set_report) = cx.use_state(|| String::from("waiting for the first commit"));

    let measured_for_element = measured.clone();
    let report_line = report.clone();
    ui! {
        <card element_ref={measured_for_element}
            on_element_change={move |snapshot: Option<ElementSnapshot>| {
                let next = match snapshot {
                    Some(snapshot) => {
                        let bounds = snapshot.bounding_rect();
                        let content = snapshot.content_rect();
                        format!(
                            "bounds {}x{} at ({}, {}) · content {}x{}",
                            bounds.width, bounds.height, bounds.line, bounds.column,
                            content.width, content.height
                        )
                    }
                    None => String::from("not committed"),
                };
                set_report.set(next);
            }}
            style={|style| {
                style.layout /= icmd::Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.padding /= icmd::Edges::all(1);
            }}>
            {Text::new("MEASURED").bold()}
            <muted>{Text::new(report_line)}</muted>
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 1; }}>
                <muted>"resize the terminal to watch the numbers change"</muted>
            </row>
        </card>
    }
}
