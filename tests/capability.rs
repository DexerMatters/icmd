#[cfg(not(feature = "native-raster"))]
use icmd::advanced::FrameError;
use icmd::advanced::{Renderer, RendererConfig, SurfaceKind};
use icmd::{ImageProtocol, Size};

// SAF-13: the renderer holds no foreign pointer after terminal detection, so it
// is `Send` by construction. This assertion would fail to compile if a raw
// Chafa object were retained and an `unsafe impl Send` were needed again.
fn assert_send<T: Send>() {}

#[test]
fn renderer_is_send_without_any_unsafe_assertion() {
    assert_send::<Renderer>();
    // A `SurfaceKind` is a plain value; the type is public so callers can
    // describe a validation failure without naming internals.
    assert_ne!(SurfaceKind::Cells, SurfaceKind::Raster);
}

#[cfg(feature = "native-raster")]
#[test]
fn native_protocols_construct_with_the_feature() {
    for protocol in [
        ImageProtocol::Kitty,
        ImageProtocol::Sixel,
        ImageProtocol::Iterm2,
        ImageProtocol::Symbols,
    ] {
        Renderer::with_config(
            Size::new(8, 4),
            RendererConfig {
                image_protocol: protocol,
                ..RendererConfig::default()
            },
        )
        .unwrap_or_else(|error| panic!("{protocol:?} should construct: {error}"));
    }
}

#[cfg(not(feature = "native-raster"))]
#[test]
fn native_protocols_report_unsupported_without_the_feature() {
    for protocol in [
        ImageProtocol::Kitty,
        ImageProtocol::Sixel,
        ImageProtocol::Iterm2,
    ] {
        let error = Renderer::with_config(
            Size::new(8, 4),
            RendererConfig {
                image_protocol: protocol,
                ..RendererConfig::default()
            },
        )
        .err()
        .unwrap_or_else(|| panic!("{protocol:?} must be rejected without native-raster"));
        assert!(
            matches!(error, FrameError::UnsupportedProtocol { .. }),
            "got {error:?}"
        );
    }
}

#[cfg(not(feature = "native-raster"))]
#[test]
fn symbols_rendering_stays_available_without_the_feature() {
    // The pure-Rust cell path must remain fully usable; only native payload
    // protocols are unavailable.
    Renderer::with_config(
        Size::new(8, 4),
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            ..RendererConfig::default()
        },
    )
    .expect("symbols rendering is pure Rust");
}
