//! Raster image widget that places an image in a cell box.

use crate::{
    Align, Attr, Dimension, DomProps, ImageAlign, ImageFit, ImageLoading, ImageMode,
    ImageRenderOptions, Justify, Node, Props, RasterImage, RasterPlacement, Text, TextWrap,
    basic::ComponentContext,
};

/// Configuration for the raster widget, [`raster_image`](crate::widgets::raster_image).
#[derive(Debug, Clone, Default)]
pub struct ImageProps {
    /// Image source; when unset the widget renders an empty element.
    pub src: Attr<crate::ImageSource>,
    /// Requested width in terminal cells; `0` (default) derives it from the
    /// source when loaded, otherwise falls back to `1`.
    pub width: Attr<u16>,
    /// Requested height in terminal cells; `0` (default) derives it from the
    /// source when loaded, otherwise falls back to `1`.
    pub height: Attr<u16>,
    /// When the source is loaded; defaults to [`ImageLoading::Lazy`].
    pub loading: Attr<ImageLoading>,
    /// How the image fits its box; defaults to [`ImageFit::Contain`].
    pub fit: Attr<ImageFit>,
    /// Horizontal placement inside the box; defaults to [`ImageAlign::Center`].
    pub horizontal_align: Attr<ImageAlign>,
    /// Vertical placement inside the box; defaults to [`ImageAlign::Center`].
    pub vertical_align: Attr<ImageAlign>,
    /// Raster render mode; defaults to [`ImageMode::Auto`].
    pub mode: Attr<ImageMode>,
    /// Text painted in the box while the source cannot be shown, such as a
    /// missing file or a failed decode; defaults to the `×` placeholder.
    ///
    /// A short label - a title, a filename, or a one-line description - keeps a
    /// box that has reserved its size readable while the image is unavailable,
    /// and a label too long for the box is clipped with an ellipsis.
    pub alt: Attr<String>,
}

/// Raster image widget; see [`ImageProps`] for its configuration.
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
    let mut placement = RasterPlacement::new(src, width, height, options)
        .with_loading(props.loading | ImageLoading::Lazy);
    if let Some(alt) = Option::<String>::from(props.alt.clone()) {
        placement = placement.with_alt(alt);
    }
    Node::element(props.host_props(defaults), [Node::raster(placement)])
}

/// Placeholder for an unloaded source that lacks an explicit width or height.
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
    let label = props.alt.clone() | String::from("×");
    Node::element(
        props.host_props(defaults),
        [Text::new(label).wrap(TextWrap::Soft).into()],
    )
}

/// Derive the cell box from the requested size and the source's pixel size,
/// preserving the aspect ratio on the unconstrained axis.
///
/// 8x16 is only an intrinsic-size estimate. The renderer uses its actual
/// terminal cell geometry when it emits native pixels.
fn dimensions(source: &RasterImage, requested_width: u16, requested_height: u16) -> (u16, u16) {
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
