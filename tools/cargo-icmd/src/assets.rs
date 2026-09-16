//! Bundled binary assets owned by this package.
//!
//! The guide ships one small original raster image so the media chapter can
//! demonstrate decoding, fitting, and render modes without a network request or
//! a runtime path dependency. See `assets/README.md` for its source and license.

/// The bundled sample image, an original 144x96 RGB PNG.
pub(crate) const GUIDE_PNG: &[u8] = include_bytes!("../assets/guide.png");

/// Decodes the bundled image under the default resource limits.
pub(crate) fn guide_image() -> Result<icmd::RasterImage, icmd::RasterImageError> {
    icmd::RasterImage::decode(GUIDE_PNG)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use icmd::advanced::{Commit, Lower, ResourceLimits, Runtime, ShutdownPolicy};
    use icmd::widgets::raster_image;
    use icmd::{
        Component, ComponentContext, ImageMode, ImageSource, Node, Props, RasterImage, Size, card,
        muted, ui,
    };

    use super::GUIDE_PNG;

    #[test]
    fn bundled_asset_is_a_decodable_png() {
        assert_eq!(&GUIDE_PNG[..8], b"\x89PNG\r\n\x1a\n");
        let image = RasterImage::decode_with_limits(GUIDE_PNG, &ResourceLimits::default())
            .expect("the bundled image decodes under the default budgets");
        assert_eq!(image.width(), 144);
        assert_eq!(image.height(), 96);
        // RGBA8: four bytes per pixel.
        assert_eq!(image.rgba8().len(), 144 * 96 * 4);
        assert!(!image.rgba8().is_empty());
    }

    /// A deterministic Symbols-mode render, so automated tests never depend on
    /// terminal graphics capability.
    fn symbols_gallery(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
        let Ok(image) = super::guide_image() else {
            return ui! { <muted>"undecodable"</muted> };
        };
        ui! {
            <card style={|style| { style.gap /= 0; }}>
                <raster_image
                    src={ImageSource::loaded(image)}
                    width={36}
                    height={16}
                    mode={ImageMode::Symbols} />
            </card>
        }
    }

    #[test]
    fn symbols_mode_image_commits_a_non_empty_frame() {
        let viewport = Size::new(60, 24);
        let (commit, _, _) = Commit::new_with_events(viewport);
        let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
        let input = runtime.input();
        let output = runtime.output();
        input
            .send(symbols_gallery.apply(()))
            .expect("the gallery enters the pipeline");
        let frame = output
            .recv_timeout(Duration::from_secs(5))
            .expect("the gallery commits a frame");
        assert!(!frame.operations.is_empty());
        drop(input);
        runtime.shutdown(ShutdownPolicy::default()).unwrap();
    }
}
