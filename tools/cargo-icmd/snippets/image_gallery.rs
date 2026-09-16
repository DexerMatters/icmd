//! The bundled image decoded once and shown through every fit and render mode.

use icmd::widgets::raster_image;
use icmd::{
    ComponentContext, Dimension, ImageFit, ImageMode, ImageSource, Node, Props, RasterImage, card,
    muted, row, ui,
};

/// One repository-owned image, decoded once with `use_memo`.
pub fn image_gallery(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let image = cx.use_memo((), || {
        RasterImage::decode(include_bytes!("../assets/guide.png")).ok()
    });
    let Some(image) = image else {
        return ui! { <muted>"the bundled guide.png could not be decoded"</muted> };
    };

    let source = |fit: ImageFit, mode: ImageMode| {
        ui! {
            <raster_image
                src={ImageSource::loaded(image.clone())}
                width={24}
                height={12}
                fit={fit}
                mode={mode} />
        }
    };

    let unavailable = || {
        ui! {
            <raster_image
                src={ImageSource::file("/srv/artifacts/guide.png")}
                width={24}
                height={3}
                alt={"guide.png"}
                mode={ImageMode::Symbols} />
        }
    };

    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <muted>"Contain · Cover · Stretch"</muted>
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 1; }}>
                {source(ImageFit::Contain, ImageMode::Auto)}
                {source(ImageFit::Cover, ImageMode::Auto)}
                {source(ImageFit::Stretch, ImageMode::Auto)}
            </row>
            <muted>"Symbols mode stays readable where no graphics protocol exists."</muted>
            {source(ImageFit::Contain, ImageMode::Symbols)}
            <muted>"An unavailable source keeps its box and paints its alt label, not a bare ×."</muted>
            {unavailable()}
        </card>
    }
}
