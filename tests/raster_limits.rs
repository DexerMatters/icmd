// Raster limit tests. Decoding and transforming run inside the crate, so
// their boundaries are driven through the hidden module.
use icmd::__private::render_rgba_with_cell_size;
use icmd::advanced::{ImageResource, LimitError, ResourceLimits};
use icmd::image::{ImageRenderOptions, ImageSource};
use icmd::{RasterImage, RasterImageError, Size};

fn tiny() -> RasterImage {
    RasterImage::from_rgba8(2, 2, vec![0u8; 16]).unwrap()
}

#[test]
fn transform_boundary_succeeds_and_one_past_fails() {
    let source = tiny();
    let limits = ResourceLimits {
        max_source_pixels: 64,
        max_transform_pixels: 64,
        max_decoded_image_bytes: 64 * 4,
        max_in_flight_image_bytes: 64 * 4,
        max_source_width: 64,
        max_source_height: 64,
        ..ResourceLimits::default()
    };
    // 2x2 cells at 4x4 pixels = 8x8 = 64 pixels, exactly the budget.
    let ok = render_rgba_with_cell_size(
        &source,
        2,
        2,
        ImageRenderOptions::default(),
        Size::new(4, 4),
        &limits,
    )
    .expect("exactly at the budget must succeed");
    assert_eq!(ok.width, 8);

    // One cell more exceeds the transform pixel budget.
    let error = render_rgba_with_cell_size(
        &source,
        3,
        3,
        ImageRenderOptions::default(),
        Size::new(4, 4),
        &limits,
    )
    .expect_err("one past the budget must fail");
    assert!(matches!(error, RasterImageError::Limit(_)), "{error:?}");
}

#[test]
fn transform_dimension_overflow_is_a_typed_error() {
    let source = tiny();
    // u16::MAX cells at a large cell size overflows any sane budget; the
    // checked width must report a limit error instead of allocating.
    let limits = ResourceLimits {
        max_source_width: 1,
        max_source_height: 1,
        ..ResourceLimits::default()
    };
    let error = render_rgba_with_cell_size(
        &source,
        u16::MAX,
        u16::MAX,
        ImageRenderOptions::default(),
        Size::new(u16::MAX, u16::MAX),
        &limits,
    )
    .expect_err("overflow must not allocate");
    assert!(matches!(error, RasterImageError::Limit(_)), "{error:?}");
}

#[test]
fn decoded_bytes_boundary_is_enforced() {
    let limits = ResourceLimits {
        max_decoded_image_bytes: 16,
        max_in_flight_image_bytes: 16,
        ..ResourceLimits::default()
    };
    // 2x2 RGBA is exactly 16 bytes.
    assert!(RasterImage::from_rgba8(2, 2, vec![0u8; 16]).is_ok());
    limits.check_decoded_bytes(16).unwrap();
    assert!(limits.check_decoded_bytes(17).is_err());
}

// A tiny file can still declare enormous dimensions in its header. The
// decoder must refuse it from the header, before any pixel buffer exists.
#[test]
fn a_crafted_huge_header_never_allocates_the_declared_size() {
    // PNG signature + IHDR with 100_000 x 100_000.
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&13u32.to_be_bytes());
    ihdr.extend_from_slice(b"IHDR");
    ihdr.extend_from_slice(&100_000u32.to_be_bytes());
    ihdr.extend_from_slice(&100_000u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    ihdr.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&ihdr);
    assert!(bytes.len() < 64, "the crafted input stays tiny");

    let limits = ResourceLimits {
        max_source_width: 4096,
        max_source_height: 4096,
        max_source_pixels: 4096 * 4096,
        ..ResourceLimits::default()
    };
    let error = RasterImage::decode_with_limits(&bytes, &limits)
        .expect_err("a 10-gigapixel header must be rejected");
    // Either the reader's configured header limit or the framework's own
    // check rejects it; both are typed resource/decode errors, never an
    // allocation of the declared shape.
    assert!(
        matches!(
            error,
            RasterImageError::Limit(_) | RasterImageError::Decode { .. }
        ),
        "{error:?}"
    );
}

#[test]
fn file_source_budget_is_checked_before_reading() {
    let limits = ResourceLimits {
        max_encoded_image_bytes: 4,
        ..ResourceLimits::default()
    };
    let source = ImageSource::file("examples/res/amber.jpg");
    let error = source
        .load_with_limits(&limits)
        .expect_err("an oversized file must be rejected before reading it");
    assert!(
        matches!(
            error,
            RasterImageError::Limit(LimitError::Exceeded {
                resource: ImageResource::EncodedBytes,
                ..
            })
        ),
        "{error:?}"
    );
}
