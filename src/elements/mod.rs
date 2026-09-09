pub(crate) mod basic;
pub(crate) mod extra;

use crate::{DomProps, Node, Props, Style, basic::ComponentContext, theme::Theme, ui};

pub use crate::basic::view;

fn themed(
    cx: &mut ComponentContext,
    props: &Props<()>,
    apply: impl FnOnce(&mut Style, &Theme),
) -> Node {
    let theme = cx.use_theme();
    let mut style = Style {
        text: theme.typography.body.clone(),
        ..Style::default()
    };
    apply(&mut style, &theme);
    let dom = props.host_props(DomProps {
        style,
        ..DomProps::default()
    });
    ui! {
        <view dom={dom}>{props.children_node()}</view>
    }
}

pub use basic::{
    blockquote, button, card, center, code, column, container, divider, footer, hbox, header,
    heading, input, kbd, label, muted, paragraph, row, section, spacer, title, vbox,
};
pub use extra::{
    AlertProps, AlertVariant, BadgeProps, BadgeVariant, CanvasContext, CanvasDraw, CanvasError,
    CanvasProps, CheckboxProps, ProgressBarProps, RadioProps, ScrollBarProps, ScrollbarOrientation,
    ScrollbarProps, SkeletonProps, SpinnerProps, SwitchProps, alert, badge, canvas, checkbox,
    progress_bar, progressbar, radio, scrollbar, skeleton, spinner, switch,
};
