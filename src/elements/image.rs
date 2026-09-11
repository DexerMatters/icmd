use crate::{
    Align, Attr, Dimension, DomProps, ImageAlign, ImageFit, ImageLoading, ImageMode,
    ImageRenderOptions, Justify, Node, Props, RasterImage, RasterPlacement, Text,
    basic::ComponentContext,
};

/// Props for the declarative `<image>` component.
#[derive(Debug, Clone, Default)]
pub struct ImageProps {
    pub src: Attr<crate::ImageSource>,
    pub width: Attr<u16>,
    pub height: Attr<u16>,
    /// File sources are prefetched near the viewport by default.
    pub loading: Attr<ImageLoading>,
    pub fit: Attr<ImageFit>,
    pub horizontal_align: Attr<ImageAlign>,
    pub vertical_align: Attr<ImageAlign>,
    pub mode: Attr<ImageMode>,
}

pub fn image(_cx: &mut ComponentContext, props: &Props<ImageProps>) -> Node {
    let Some(src) = Option::<crate::ImageSource>::from(props.src.clone()) else {
        return Node::element(props.dom.clone(), []);
    };
    let requested_width = props.width | 0;
    let requested_height = props.height | 0;
    if src.loaded_image().is_none() && (requested_width == 0 || requested_height == 0) {
        return invalid_source(props, requested_width.max(1), requested_height.max(1));
    }
    let (width, height) = src
        .loaded_image()
        .map(|image| dimensions(image, requested_width, requested_height))
        .unwrap_or((requested_width, requested_height));
    let defaults = DomProps {
        style: crate::Style {
            width: Attr::Set(Dimension::Cells(width)),
            height: Attr::Set(Dimension::Cells(height)),
            ..crate::Style::default()
        },
        ..DomProps::default()
    };
    let options = ImageRenderOptions {
        fit: props.fit | ImageFit::Contain,
        horizontal_align: props.horizontal_align | ImageAlign::Center,
        vertical_align: props.vertical_align | ImageAlign::Center,
        mode: props.mode | ImageMode::Auto,
    };
    Node::element(
        props.host_props(defaults),
        [Node::raster(
            RasterPlacement::new(src, width, height, options)
                .with_loading(props.loading | ImageLoading::Lazy),
        )],
    )
}

fn invalid_source(props: &Props<ImageProps>, width: u16, height: u16) -> Node {
    let defaults = DomProps {
        style: crate::Style {
            width: Attr::Set(Dimension::Cells(width)),
            height: Attr::Set(Dimension::Cells(height)),
            layout: Attr::Set(crate::Layout::Vertical),
            justify: Attr::Set(Justify::Center),
            align: Attr::Set(Align::Center),
            ..crate::Style::default()
        },
        ..DomProps::default()
    };
    Node::element(props.host_props(defaults), [Text::new("×").into()])
}

fn dimensions(source: &RasterImage, requested_width: u16, requested_height: u16) -> (u16, u16) {
    // 8x16 is only an intrinsic-size estimate. The renderer uses its actual
    // terminal cell geometry when it emits native pixels.
    let natural_width = (source.width().saturating_add(7) / 8).clamp(1, u16::MAX as u32) as u16;
    let natural_height = (source.height().saturating_add(15) / 16).clamp(1, u16::MAX as u32) as u16;
    match (requested_width, requested_height) {
        (0, 0) => (natural_width, natural_height),
        (width, 0) => {
            let height = ((u32::from(width) * source.height() * 8) / (source.width().max(1) * 16))
                .clamp(1, u16::MAX as u32) as u16;
            (width, height)
        }
        (0, height) => {
            let width = ((u32::from(height) * source.width() * 16) / (source.height().max(1) * 8))
                .clamp(1, u16::MAX as u32) as u16;
            (width, height)
        }
        (width, height) => (width, height),
    }
}
