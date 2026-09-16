//! Flat vocabulary of the element widgets: module declarations plus the
//! re-exports that make each widget and its props available by name.

pub(crate) mod basic;
pub(crate) mod canvas;
pub(crate) mod controls;
pub(crate) mod feedback;
pub(crate) mod image;
pub(crate) mod input;
pub(crate) mod interactive;
pub(crate) mod link;
pub(crate) mod scroll;
pub(crate) mod selection_area;

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
    ButtonProps, ButtonVariant, blockquote, button, card, center, code, column, container, divider,
    footer, heading, kbd, label, muted, paragraph, row, section, spacer,
};
pub use canvas::{CanvasContext, CanvasDraw, CanvasError, CanvasProps, canvas};
pub use controls::{CheckboxProps, RadioProps, SwitchProps, checkbox, radio, switch};
pub use feedback::{
    AlertProps, AlertVariant, BadgeProps, BadgeVariant, ProgressBarProps, SkeletonProps,
    SpinnerProps, alert, badge, progress_bar, skeleton, spinner,
};
/// The raster widget constructor is published once, as `widgets::raster_image`;
/// only its props type is part of the flat vocabulary.
pub use image::ImageProps;
pub use input::{
    InputProps, RawInputAppearance, RawInputMode, RawInputProps, TextClipboardAction,
    TextClipboardEvent, TextValueEvent, TextareaProps, input, normalize_for_test, raw_input,
    textarea,
};
pub use link::{LinkProps, link};
pub use scroll::{ScrollAreaProps, scroll_area};
pub use selection_area::{SelectionAreaProps, TextSelectionEvent, selection_area};
