// A downstream application built from the advanced tier. It must compile
// against the documented advanced surface with native raster enabled, which the
// high-level fixture deliberately does not cover.
use icmd::advanced::{
    Commit, FrameBuilder, Lower, Renderer, RendererConfig, ResourceLimits, Runtime, ShutdownPolicy,
    Stage, SurfaceKind,
};
use icmd::{Cell, Component, Dimension, ImageProtocol, Node, Size, style};

#[allow(dead_code)]
fn typed_frame_construction() {
    let viewport = Size::new(20, 6);
    let _ = viewport;
    let limits = ResourceLimits::default();
    limits.validate().expect("the default policy is valid");

    let mut builder = FrameBuilder::new();
    // A typed handle names the surface kind, so a raster-only operation cannot
    // be aimed at a cell surface by mistake.
    let image = icmd::Image::new(2, 1, Cell::plain("x").expect("valid glyph"))
        .expect("the surface fits the viewport");
    let surface = builder.create_cells(image, icmd::ScreenPosition::new(0, 0), 0);
    builder
        .patch_cells(
            surface,
            vec![icmd::CellEdit {
                position: icmd::ImagePosition::new(0, 0),
                cell: Cell::plain("y").expect("valid glyph"),
            }],
        )
        .expect("patched cells fit the surface");
    let frame = builder.finish();
    let _ = (frame, surface.id());

    // The advanced tier reports its own protocol identifiers and metric types.
    let kind = SurfaceKind::Cells;
    let protocol = ImageProtocol::Symbols;
    let _ = (kind, protocol, Stage::Lower);
}

#[allow(dead_code)]
fn owns_its_runtime() {
    let viewport = Size::new(20, 6);
    let (commit, _viewport) = Commit::new(viewport);
    let renderer = Renderer::with_config(
        viewport,
        RendererConfig {
            image_protocol: ImageProtocol::Symbols,
            ..RendererConfig::default()
        },
    )
    .expect("the fixture viewport is valid");

    let runtime = Runtime::new(Lower::default())
        .then(commit)
        .then(renderer)
        .start_handle();
    let input = runtime.input();
    let node: Node = icmd::text("advanced");
    let _ = input.send(node);
    // Shutdown is explicit and acknowledged, not implied by dropping a handle.
    let _ = runtime.shutdown(ShutdownPolicy::default());
}

#[allow(dead_code)]
fn styles_through_the_high_level_tier_too() {
    let node: Node = icmd::view.apply(icmd::Props::new(()).with_dom(
        icmd::DomProps::default().with_style(style(|value| {
            value.width /= Dimension::Max;
        })),
    ));
    let _ = node;
}

fn main() {}
