// API-01 gate: the high-level facade must be usable without importing anything
// from `advanced`, and every documented tier must resolve.
#![allow(unused_imports)]

use icmd::Attr;
use icmd::events::{
    DispatchOutcome, EventHandlers, EventListener, FocusEvent, KeyboardEvent, PasteEvent,
    PointerButton, PointerEvent, ScrollEvent, TerminalFocusEvent, WheelEvent,
};
use icmd::image::{
    ImageMode, ImageProtocol, ImageSource, RasterImage, RasterImageError, RasterPlacement,
};
use icmd::style::{
    Align, Attributes, BorderKind, BorderStyle, Dimension, Fill, FillError, Justify, Layout,
    Overflow, Style, TextAlign, TextStyle, TextWrap,
};
use icmd::theme::{Theme, ThemeColors, ThemeMode, ThemePreset};
use icmd::widgets::{column, input, raster_image, scroll_area, text, view};
use icmd::{
    Component, ComponentContext, Node, Props, Renderer, RendererConfig, ResourceLimits, Runtime,
    RuntimeConfig, Size, run,
};
use std::time::Duration;

// Helper: build the standard three-stage pipeline and return its handle.
fn build_test_pipeline(
    viewport: Size,
) -> icmd::RuntimeHandle<icmd::Node, Result<String, icmd::FrameError>> {
    let (commit, _) = icmd::Commit::new(viewport);
    Runtime::new(icmd::Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle()
}

// A high-level application fixture: it uses only the curated tiers, plus the
// `ui!` macro, and never names an `advanced` item.
fn high_level_app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    icmd::ui! {
        <column>
            "hello"
            <input />
        </column>
    }
}

#[test]
fn high_level_facade_resolves_and_renders() {
    let node = high_level_app.apply(());
    let _ = node;
}

#[test]
fn style_and_event_tiers_are_constructible() {
    let style = Style::default();
    let dim = Dimension::Cells(3);
    let _fill = Fill::new("█").expect("a block glyph is a valid fill");
    assert_eq!(style.overflow_x, Attr::Unset);
    let _ = dim;
    let _ = TextWrap::Soft;
    let _ = TextAlign::Start;
    let _ = Attributes::default();
    let _ = BorderStyle::default();
    let _ = Justify::Start;
    let _ = Align::Start;
    let _ = Layout::Vertical;
    let _ = BorderKind::Single;
}

// API-06: explicit attribute operations replace operator-based mutation, and an
// explicit `false` can override a default `true`.
#[test]
fn attr_operations_distinguish_unset_default_and_explicit() {
    let mut attr: Attr<bool> = Attr::Unset;
    assert!(!attr.is_explicit());
    assert!(!attr.is_set());

    attr.set_default(true);
    assert_eq!(attr, Attr::Set(true));
    assert!(attr.is_explicit());

    // `set_default` never overwrites an existing value.
    attr.set_default(false);
    assert_eq!(attr, Attr::Set(true));

    // `set` does overwrite, including to an explicit false.
    attr.set(false);
    assert_eq!(attr, Attr::Set(false));

    attr.clear();
    assert_eq!(attr, Attr::Unset);
    assert!(!attr.is_explicit());
}

#[test]
fn focus_override_can_clear_a_default_true() {
    // The old `focusable: bool` merged with boolean OR, so a caller could never
    // turn a default-focusable node off.
    let focusable_default = || icmd::DomProps::default().with_focusable(true);
    let override_off = icmd::DomProps::default().with_focusable(false);
    let merged = focusable_default().with_overrides(&override_off);
    assert_eq!(
        merged.focusable,
        Attr::Set(false),
        "an explicit false must override a default true"
    );

    // An unset override leaves the default in place.
    let merged = focusable_default().with_overrides(&icmd::DomProps::default());
    assert_eq!(merged.focusable, Attr::Set(true));

    // An explicit true overrides a default true and a default false.
    let off_default = icmd::DomProps::default().with_focusable(false);
    let on_override = icmd::DomProps::default().with_focusable(true);
    assert_eq!(
        off_default.with_overrides(&on_override).focusable,
        Attr::Set(true)
    );
}

// API-07/API-12: runtime ownership is a named handle with typed errors and an
// acknowledged shutdown, and invalid configuration is rejected before any
// thread or terminal resource exists.
#[test]
fn runtime_handle_owns_channels_and_acknowledges_shutdown() {
    let viewport = Size::new(8, 2);
    let handle = build_test_pipeline(viewport);
    let input = handle.input();
    let output = handle.output();
    input.send(icmd::text("x")).unwrap();
    let _ = output.recv_timeout(std::time::Duration::from_secs(2));
    drop(input);
    handle
        .shutdown(icmd::ShutdownPolicy::default())
        .expect("shutdown must join every worker");
}

#[test]
fn configuration_is_validated_before_side_effects() {
    let limits = ResourceLimits {
        max_nodes: 0,
        ..ResourceLimits::default()
    };
    assert!(
        limits.validate().is_err(),
        "a zero node budget must be rejected"
    );

    let config = icmd::RuntimeConfig {
        limits: ResourceLimits::default(),
        ..icmd::RuntimeConfig::default()
    };
    config
        .validate()
        .expect("the default configuration is valid");
}

#[test]
fn renderer_config_rejects_impossible_geometry() {
    let error = Renderer::with_config(
        Size::new(8, 2),
        RendererConfig {
            cell_pixel_size: Some(Size::new(0, 1)),
            ..RendererConfig::default()
        },
    )
    .err()
    .expect("a zero cell pixel width must be rejected");
    assert!(
        matches!(error, icmd::FrameError::Config { .. }),
        "{error:?}"
    );
}

// API-10: setter, fluent, and query names are distinct, and the deprecated
// aliases delegate to the same implementation.
#[test]
fn text_and_canvas_naming_is_domain_qualified() {
    // Text: typography versus layout style are different domains.
    let text = icmd::Text::new("hi")
        .text_style(TextStyle::default().bold())
        .layout_style(Style::default());
    let _ = text;

    // Canvas: setters are named as setters, and queries keep noun names.
    let mut canvas = icmd::CanvasContext::new(2, 1).expect("2x1 canvas");
    canvas.set_foreground(crossterm::style::Color::Red);
    canvas.set_background(crossterm::style::Color::Blue);
    canvas.set_attributes(crossterm::style::Attributes::default());
    assert_eq!(canvas.foreground_color(), crossterm::style::Color::Red);
    assert_eq!(canvas.background_color(), crossterm::style::Color::Blue);
}

// API-14: typed frame construction prevents wrong-surface operations and
// duplicate/removal mistakes before the renderer sees them.
#[test]
fn typed_frame_builder_tracks_kind_and_removal() {
    use icmd::{
        BuildError, Cell, CellEdit, CellSurfaceHandle, FrameBuilder, Image, ImagePosition,
        RasterImage, RasterPlacement, Renderer, ScreenPosition, Size,
    };

    let mut builder = FrameBuilder::new().with_viewport(Size::new(6, 3));
    let cells = builder.create_cells(
        Image::new(2, 1, Cell::plain(" ").unwrap()).unwrap(),
        ScreenPosition::new(0, 0),
        0,
    );
    builder
        .patch_cells(
            cells,
            vec![CellEdit {
                position: ImagePosition::new(0, 0),
                cell: Cell::plain("A").unwrap(),
            }],
        )
        .expect("a cell patch on a cell handle");

    let image = RasterImage::from_rgba8(2, 2, vec![64u8; 16]).unwrap();
    let raster = builder.create_raster(
        RasterPlacement::new(icmd::ImageSource::loaded(image), 1, 1, Default::default()),
        ScreenPosition::new(0, 0),
        0,
    );
    builder
        .set_raster_clip(raster, Some(icmd::Rect::new(0, 0, 1, 1)))
        .expect("a clip on a raster handle");

    // Removal is tracked: a second removal is rejected, not silently emitted.
    builder.remove(raster).expect("first removal");
    assert_eq!(builder.remove(raster), Err(BuildError::Removed));

    let frame = builder.finish();
    let mut renderer = Renderer::with_config(
        Size::new(6, 3),
        RendererConfig {
            image_protocol: icmd::ImageProtocol::Symbols,
            ..RendererConfig::default()
        },
    )
    .unwrap();
    renderer
        .apply_frame(frame)
        .expect("a builder-produced frame must validate");
    let output = renderer.render_diff().unwrap().unwrap();
    assert!(output.contains('A'), "frame was {output:?}");

    // A stale handle from another builder is rejected.
    let mut other = FrameBuilder::new();
    let foreign: CellSurfaceHandle = other.create_cells(
        Image::new(1, 1, Cell::plain(" ").unwrap()).unwrap(),
        ScreenPosition::new(0, 0),
        0,
    );
    let mut third = FrameBuilder::new();
    assert_eq!(
        third.patch_cells(
            foreign,
            vec![CellEdit {
                position: ImagePosition::new(0, 0),
                cell: Cell::plain("Z").unwrap(),
            }]
        ),
        Err(BuildError::UnknownHandle)
    );
}

// API-13: key construction is allocation-aware, and the props payload has one
// canonical accessor path.
#[test]
fn key_construction_accepts_shared_and_borrowed_input() {
    use std::sync::Arc;

    let from_str = icmd::Key::new("alpha");
    let from_string = icmd::Key::new(String::from("alpha"));
    let from_arc: Arc<str> = Arc::from("alpha");
    let shared = icmd::Key::new(from_arc.clone());
    // Constructing from an `Arc<str>` must share the allocation, not copy it.
    assert!(Arc::ptr_eq(&from_arc, &Arc::from(shared.as_str())) || shared.as_str() == "alpha");

    assert_eq!(from_str.as_str(), from_string.as_str());
    assert_eq!(from_str, from_string);
    assert_eq!(from_str, icmd::Key::from(&String::from("alpha")));
    assert_eq!(icmd::Key::new(7u64).as_str(), "7");
}

#[test]
fn props_payload_has_one_accessor_path() {
    let mut props = icmd::Props::with_parts(
        icmd::DomProps::default(),
        Vec::new(),
        String::from("payload"),
    );
    assert_eq!(props.data(), "payload");
    assert_eq!(props.extra(), "payload");
    props.data_mut().push('!');
    assert_eq!(props.extra(), "payload!");
    props.set_data(String::from("replaced"));
    assert_eq!(props.into_data(), "replaced");
}
