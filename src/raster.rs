//! Raster image decoding, cache identity, and cell rendering.
//!
//! Owns [`RasterImage`] and its limit-aware loaders, [`RasterPlacement`] and the
//! render options, and the symbol engine that lowers pixels to cells.

use std::{
    error::Error,
    fmt, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crossterm::style::{Attributes, Color};

use crate::runtime::limits::{LimitError, ResourceLimits};
use crate::{Cell, Image, Size};

static NEXT_RASTER_ID: AtomicU64 = AtomicU64::new(1);

/// Why a raster image could not be created or loaded.
///
/// Errors carry their originating source, so they are compared by shape rather
/// than by `PartialEq`: two distinct I/O failures are not the same failure.
#[derive(Debug, Clone)]
pub enum RasterImageError {
    /// A requested dimension was zero.
    Empty,
    /// The RGBA8 buffer length did not match the dimensions.
    InvalidLength {
        /// Required buffer length in bytes.
        expected: usize,
        /// Supplied buffer length in bytes.
        actual: usize,
    },
    /// The dimensions overflowed the supported range.
    DimensionsTooLarge {
        /// Requested width in pixels.
        width: u32,
        /// Requested height in pixels.
        height: u32,
    },
    /// The encoded bytes could not be decoded.
    Decode {
        /// Underlying decoder failure, preserved for inspection.
        source: Arc<::image::ImageError>,
    },
    /// The file could not be read.
    Io {
        /// Path that was being read.
        path: PathBuf,
        /// Underlying I/O failure, preserved for inspection.
        source: Arc<std::io::Error>,
    },
    /// A configured resource ceiling rejected the work before allocating.
    Limit(LimitError),
    /// The native symbol engine could not create an image canvas.
    ChafaUnavailable,
}

impl RasterImageError {
    fn decode(error: ::image::ImageError) -> Self {
        Self::Decode {
            source: Arc::new(error),
        }
    }

    fn io(path: &Path, error: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source: Arc::new(error),
        }
    }

    fn limit(error: LimitError) -> Self {
        Self::Limit(error)
    }
}

impl fmt::Display for RasterImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "raster image dimensions must be non-zero"),
            Self::InvalidLength { expected, actual } => write!(
                f,
                "invalid RGBA8 buffer length: expected {expected} bytes, got {actual}"
            ),
            Self::DimensionsTooLarge { width, height } => {
                write!(f, "raster image {width}x{height} is too large")
            }
            Self::Decode { source } => write!(f, "could not decode image: {source}"),
            Self::Io { path, source } => {
                write!(f, "could not read image {}: {source}", path.display())
            }
            Self::Limit(error) => write!(f, "{error}"),
            Self::ChafaUnavailable => write!(f, "Chafa could not create an image canvas"),
        }
    }
}

impl Error for RasterImageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Decode { source } => Some(source.as_ref()),
            Self::Io { source, .. } => Some(source.as_ref()),
            Self::Limit(error) => Some(error),
            Self::Empty
            | Self::InvalidLength { .. }
            | Self::DimensionsTooLarge { .. }
            | Self::ChafaUnavailable => None,
        }
    }
}

/// Decodes a reader under framework policy, enforcing the dimension and
/// decoded-byte ceilings from the header before the pixel buffer exists.
fn decode_reader<R: std::io::BufRead + std::io::Seek>(
    mut reader: ::image::ImageReader<R>,
    limits: &ResourceLimits,
) -> Result<::image::RgbaImage, RasterImageError> {
    let mut image_limits = ::image::Limits::default();
    image_limits.max_image_width = Some(limits.max_source_width);
    image_limits.max_image_height = Some(limits.max_source_height);
    image_limits.max_alloc = Some(limits.max_decoded_image_bytes as u64);
    reader.limits(image_limits);
    let decoded = reader
        .decode()
        .map_err(RasterImageError::decode)?
        .to_rgba8();
    limits
        .check_source_size(decoded.width(), decoded.height())
        .map_err(RasterImageError::limit)?;
    limits
        .check_decoded_bytes(decoded.as_raw().len())
        .map_err(RasterImageError::limit)?;
    Ok(decoded)
}

/// An immutable RGBA8 raster image with a unique identity.
#[derive(Clone)]
pub struct RasterImage {
    id: u64,
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

impl fmt::Debug for RasterImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RasterImage")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl PartialEq for RasterImage {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for RasterImage {}

impl Hash for RasterImage {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl RasterImage {
    /// Creates an image from a row-major RGBA8 buffer, whose length must be
    /// exactly `width * height * 4` bytes.
    pub fn from_rgba8(
        width: u32,
        height: u32,
        pixels: impl Into<Arc<[u8]>>,
    ) -> Result<Self, RasterImageError> {
        if width == 0 || height == 0 {
            return Err(RasterImageError::Empty);
        }
        let expected = usize::try_from(width)
            .ok()
            .and_then(|value| value.checked_mul(usize::try_from(height).ok()?))
            .and_then(|value| value.checked_mul(4))
            .ok_or(RasterImageError::DimensionsTooLarge { width, height })?;
        let pixels = pixels.into();
        if pixels.len() != expected {
            return Err(RasterImageError::InvalidLength {
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            id: NEXT_RASTER_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            pixels,
        })
    }

    /// Decodes encoded image bytes under the default resource limits.
    pub fn decode(bytes: impl AsRef<[u8]>) -> Result<Self, RasterImageError> {
        Self::decode_with_limits(bytes.as_ref(), &ResourceLimits::default())
    }

    /// Decodes encoded image bytes under `limits`.
    ///
    /// A byte slice consults the same header, dimension, and byte budgets as
    /// file loading: the reader is configured from the policy rather than the
    /// dependency's defaults, so the framework states its own upper bound.
    pub fn decode_with_limits(
        bytes: &[u8],
        limits: &ResourceLimits,
    ) -> Result<Self, RasterImageError> {
        limits
            .check_encoded_bytes(bytes.len() as u64)
            .map_err(RasterImageError::limit)?;
        let reader = ::image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|error| RasterImageError::io(Path::new("<memory>"), error))?;
        let decoded = decode_reader(reader, limits)?;
        Self::from_rgba8(decoded.width(), decoded.height(), decoded.into_raw())
    }

    /// Decodes the image file at `path` under the default resource limits.
    ///
    /// Opens exactly the path the caller supplied: no lexical normalization is
    /// applied, because on a filesystem with symlinks `link/../target` resolves
    /// after the link is traversed, so folding `..` textually can select a
    /// different file than the kernel would. The original path is preserved in
    /// errors.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RasterImageError> {
        Self::open_with_limits(path, &ResourceLimits::default())
    }

    /// Decodes the image file at `path` under `limits`.
    ///
    /// An oversized encoded file is rejected from its metadata before reading,
    /// so a huge file never becomes a huge buffer.
    pub fn open_with_limits(
        path: impl AsRef<Path>,
        limits: &ResourceLimits,
    ) -> Result<Self, RasterImageError> {
        let path = path.as_ref();
        let metadata = fs::metadata(path).map_err(|error| RasterImageError::io(path, error))?;
        limits
            .check_encoded_bytes(metadata.len())
            .map_err(RasterImageError::limit)?;
        let file = fs::File::open(path).map_err(|error| RasterImageError::io(path, error))?;
        let reader = ::image::ImageReader::new(std::io::BufReader::new(file))
            .with_guessed_format()
            .map_err(|error| RasterImageError::io(path, error))?;
        let decoded = decode_reader(reader, limits)?;
        Self::from_rgba8(decoded.width(), decoded.height(), decoded.into_raw())
    }

    /// Returns the width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }
    /// Returns the height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }
    /// Returns the unique identity assigned when the image was created.
    pub const fn id(&self) -> u64 {
        self.id
    }
    /// Returns the row-major RGBA8 pixel buffer.
    pub fn rgba8(&self) -> &[u8] {
        &self.pixels
    }
}

/// Where a raster surface's pixels come from: an already-loaded image or a file
/// path loaded on demand.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageSource {
    /// An image already held in memory.
    Loaded(RasterImage),
    /// A path resolved when the image is loaded.
    File(Arc<PathBuf>),
}

/// Cache identity derived from an [`ImageSource`].
///
/// It is computed separately from the caller-visible source so that two
/// spellings of the same opened file share one entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageSourceKey {
    /// Identity of a loaded image, by image id.
    Loaded(u64),
    /// Identity of a file source, by canonical path when available.
    File(PathBuf),
}

impl From<&ImageSource> for ImageSourceKey {
    fn from(source: &ImageSource) -> Self {
        source.cache_key()
    }
}

impl ImageSource {
    /// Wraps an already-loaded image.
    pub fn loaded(image: RasterImage) -> Self {
        Self::Loaded(image)
    }

    /// Creates a file source that stores the caller's path verbatim, so loading
    /// resolves exactly what the caller asked for; cache identity is decided
    /// later from the opened handle by [`ImageSource::cache_key`].
    pub fn file(path: impl AsRef<Path>) -> Self {
        Self::File(Arc::new(path.as_ref().to_path_buf()))
    }

    /// Returns the identity used for caching.
    ///
    /// Canonicalization is best-effort: a sandboxed or since-deleted path can
    /// still be a valid handle, so failure falls back to the caller-visible path
    /// instead of rejecting the source.
    pub fn cache_key(&self) -> ImageSourceKey {
        match self {
            Self::Loaded(image) => ImageSourceKey::Loaded(image.id()),
            Self::File(path) => ImageSourceKey::File(
                fs::canonicalize(path.as_path()).unwrap_or_else(|_| path.as_ref().to_path_buf()),
            ),
        }
    }

    /// Returns the file path, or `None` for an already-loaded image.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Loaded(_) => None,
            Self::File(path) => Some(path.as_path()),
        }
    }

    /// Returns the image, loading a file source under `limits`.
    pub fn load_with_limits(
        &self,
        limits: &ResourceLimits,
    ) -> Result<RasterImage, RasterImageError> {
        match self {
            Self::Loaded(image) => Ok(image.clone()),
            Self::File(path) => RasterImage::open_with_limits(path.as_path(), limits),
        }
    }

    /// Returns the already-loaded image, or `None` for a file source.
    pub(crate) fn loaded_image(&self) -> Option<&RasterImage> {
        match self {
            Self::Loaded(image) => Some(image),
            Self::File(_) => None,
        }
    }
}

impl From<RasterImage> for ImageSource {
    fn from(value: RasterImage) -> Self {
        Self::Loaded(value)
    }
}

impl From<PathBuf> for ImageSource {
    fn from(value: PathBuf) -> Self {
        Self::file(value)
    }
}

impl From<String> for ImageSource {
    fn from(value: String) -> Self {
        Self::file(value)
    }
}

impl From<&str> for ImageSource {
    fn from(value: &str) -> Self {
        Self::file(value)
    }
}

impl From<&Path> for ImageSource {
    fn from(value: &Path) -> Self {
        Self::file(value)
    }
}

/// When a file-backed raster source is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageLoading {
    /// Decode when the surface first needs pixels.
    #[default]
    Lazy,
    /// Decode when the surface is created.
    Eager,
}

/// The terminal image protocol a raster surface prefers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageProtocol {
    /// Select the protocol from the terminal's detected capabilities.
    #[default]
    Auto,
    /// The Kitty graphics protocol.
    Kitty,
    /// The Sixel graphics protocol.
    Sixel,
    /// The iTerm2 inline image protocol.
    Iterm2,
    /// Render with text symbols instead of a graphics protocol.
    Symbols,
}

/// When a raster surface may be repainted with the native protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageUpdatePolicy {
    /// Repaint with whichever protocol the terminal supports.
    #[default]
    Adaptive,
    /// Repaint only with the native protocol.
    NativeOnly,
}

/// Whether a raster surface renders natively or as text symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageMode {
    /// Choose the mode from terminal capabilities.
    #[default]
    Auto,
    /// Render with a native graphics protocol.
    Native,
    /// Render with text symbols.
    Symbols,
}

/// How a source image is fitted into the target raster box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageFit {
    /// Scale to fit entirely inside the box, preserving aspect ratio.
    #[default]
    Contain,
    /// Scale to cover the box, preserving aspect ratio and cropping.
    Cover,
    /// Scale to exactly the box, ignoring aspect ratio.
    Stretch,
}

/// Where content is placed on an axis when it is smaller than the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageAlign {
    /// Align to the start of the axis.
    Start,
    /// Center on the axis.
    #[default]
    Center,
    /// Align to the end of the axis.
    End,
}

/// How a raster source is fitted and aligned inside its target box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageRenderOptions {
    /// Fit policy for the source image.
    pub fit: ImageFit,
    /// Horizontal placement when the fitted image is narrower than the box.
    pub horizontal_align: ImageAlign,
    /// Vertical placement when the fitted image is shorter than the box.
    pub vertical_align: ImageAlign,
    /// Whether to render natively or with text symbols.
    pub mode: ImageMode,
}

impl Default for ImageRenderOptions {
    fn default() -> Self {
        Self {
            fit: ImageFit::Contain,
            horizontal_align: ImageAlign::Center,
            vertical_align: ImageAlign::Center,
            mode: ImageMode::Auto,
        }
    }
}

/// A raster surface's source, target box, and render options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterPlacement {
    /// Image source, either loaded or file-backed.
    pub source: ImageSource,
    /// Target width in terminal cells.
    pub width: u16,
    /// Target height in terminal cells.
    pub height: u16,
    /// Fit, alignment, and mode for rendering.
    pub options: ImageRenderOptions,
    pub(crate) loading: ImageLoading,
    pub(crate) invalid_source: bool,
    pub(crate) full_width: u16,
    pub(crate) full_height: u16,
}

impl RasterPlacement {
    /// Creates a placement for `source` in a `width` by `height` cell box. A
    /// file source with a zero dimension is marked invalid and clamped to 1.
    pub fn new(
        source: impl Into<ImageSource>,
        width: u16,
        height: u16,
        options: ImageRenderOptions,
    ) -> Self {
        let source: ImageSource = source.into();
        let invalid_source = matches!(&source, ImageSource::File(_)) && (width == 0 || height == 0);
        let (width, height) = if invalid_source {
            (width.max(1), height.max(1))
        } else {
            (width, height)
        };
        Self {
            source,
            width,
            height,
            options,
            loading: ImageLoading::Lazy,
            invalid_source,
            full_width: width,
            full_height: height,
        }
    }

    /// Returns the placement's image source.
    pub fn source(&self) -> &ImageSource {
        &self.source
    }

    /// Sets when a file-backed source is decoded.
    pub fn with_loading(mut self, loading: ImageLoading) -> Self {
        self.loading = loading;
        self
    }
}

/// Renders the whole source into an image of `width` by `height` cells.
pub(crate) fn symbols(
    source: &RasterImage,
    width: u16,
    height: u16,
    _options: ImageRenderOptions,
) -> Result<Image, RasterImageError> {
    symbols_tile(source, width, height, 0, 0, width, height, _options)
}

/// Renders a `width` by `height` cell tile at `column` and `line` of a
/// `full_width` by `full_height` render of the source.
#[allow(clippy::too_many_arguments)]
pub(crate) fn symbols_tile(
    source: &RasterImage,
    full_width: u16,
    full_height: u16,
    column: u16,
    line: u16,
    width: u16,
    height: u16,
    options: ImageRenderOptions,
) -> Result<Image, RasterImageError> {
    if width == 0 || height == 0 {
        return Err(RasterImageError::Empty);
    }
    let full = render_rgba(source, full_width, full_height, options)?;
    let pixels = crop_rgba(&full, column, line, width, height);
    symbols_from_rgba(
        &pixels,
        width,
        height,
        usize::from(width) * 8 * 4,
        Size::new(8, 16),
    )
}

/// Renders a pixel buffer into an image of `width` by `height` cells, using
/// `cell_pixels` as the per-cell pixel geometry.
pub(crate) fn symbols_from_pixels(
    pixels: &RasterPixels,
    width: u16,
    height: u16,
    cell_pixels: Size,
) -> Result<Image, RasterImageError> {
    symbols_from_rgba(
        &pixels.pixels,
        width,
        height,
        pixels.width as usize * 4,
        cell_pixels,
    )
}

/// Renders each cell as a colored block derived from the average of the pixels
/// it covers.
///
/// The pure-Rust fallback keeps the no-default-features build useful for raster
/// previews without asserting the quality of the native symbol engine.
#[cfg(not(feature = "native-raster"))]
fn symbols_from_rgba(
    pixels: &[u8],
    width: u16,
    height: u16,
    row_stride: usize,
    cell_pixels: Size,
) -> Result<Image, RasterImageError> {
    let cell_width = usize::from(cell_pixels.width.max(1));
    let cell_height = usize::from(cell_pixels.height.max(1));
    let mut rows = Vec::with_capacity(usize::from(height));
    for y in 0..usize::from(height) {
        let mut row = Vec::with_capacity(usize::from(width));
        for x in 0..usize::from(width) {
            let mut sums = [0u64; 3];
            let mut count = 0u64;
            for dy in 0..cell_height {
                let line = y * cell_height + dy;
                let base = match line.checked_mul(row_stride) {
                    Some(base) => base,
                    None => break,
                };
                for dx in 0..cell_width {
                    let offset = match base.checked_add((x * cell_width + dx).saturating_mul(4)) {
                        Some(offset) => offset,
                        None => continue,
                    };
                    let Some(pixel) = pixels.get(offset..offset + 4) else {
                        continue;
                    };
                    sums[0] += u64::from(pixel[0]);
                    sums[1] += u64::from(pixel[1]);
                    sums[2] += u64::from(pixel[2]);
                    count += 1;
                }
            }
            let average = |channel: usize| -> u8 {
                match sums[channel].checked_div(count) {
                    Some(value) => value.min(255) as u8,
                    None => 0,
                }
            };
            let fg = Color::Rgb {
                r: average(0),
                g: average(1),
                b: average(2),
            };
            row.push(
                Cell::styled(fg, Color::Reset, Attributes::default(), "█".to_string())
                    .unwrap_or_else(|_| Cell::blank()),
            );
        }
        rows.push(row);
    }
    Image::from_rows(rows).map_err(|_| RasterImageError::ChafaUnavailable)
}

/// Renders a pixel buffer into an image of `width` by `height` cells through the
/// Chafa symbol engine.
#[cfg(feature = "native-raster")]
fn symbols_from_rgba(
    pixels: &[u8],
    width: u16,
    height: u16,
    row_stride: usize,
    cell_pixels: Size,
) -> Result<Image, RasterImageError> {
    unsafe {
        let config = chafa_sys::chafa_canvas_config_new();
        if config.is_null() {
            return Err(RasterImageError::ChafaUnavailable);
        }
        chafa_sys::chafa_canvas_config_set_geometry(config, i32::from(width), i32::from(height));
        chafa_sys::chafa_canvas_config_set_cell_geometry(
            config,
            i32::from(cell_pixels.width.max(1)),
            i32::from(cell_pixels.height.max(1)),
        );
        chafa_sys::chafa_canvas_config_set_pixel_mode(
            config,
            chafa_sys::ChafaPixelMode_CHAFA_PIXEL_MODE_SYMBOLS,
        );
        chafa_sys::chafa_canvas_config_set_canvas_mode(
            config,
            chafa_sys::ChafaCanvasMode_CHAFA_CANVAS_MODE_TRUECOLOR,
        );
        let canvas = chafa_sys::chafa_canvas_new(config);
        chafa_sys::chafa_canvas_config_unref(config);
        if canvas.is_null() {
            return Err(RasterImageError::ChafaUnavailable);
        }
        chafa_sys::chafa_canvas_draw_all_pixels(
            canvas,
            chafa_sys::ChafaPixelType_CHAFA_PIXEL_RGBA8_UNASSOCIATED,
            pixels.as_ptr(),
            i32::from(width) * i32::from(cell_pixels.width.max(1)),
            i32::from(height) * i32::from(cell_pixels.height.max(1)),
            row_stride.min(i32::MAX as usize) as i32,
        );
        let mut rows = Vec::with_capacity(usize::from(height));
        for y in 0..i32::from(height) {
            let mut row = Vec::with_capacity(usize::from(width));
            for x in 0..i32::from(width) {
                let character = chafa_sys::chafa_canvas_get_char_at(canvas, x, y);
                let symbol = char::from_u32(character).unwrap_or(' ');
                let mut fg = -1;
                let mut bg = -1;
                chafa_sys::chafa_canvas_get_colors_at(canvas, x, y, &mut fg, &mut bg);
                row.push(
                    Cell::styled(
                        packed_color(fg),
                        packed_color(bg),
                        Attributes::default(),
                        symbol.to_string(),
                    )
                    .unwrap_or_else(|_| Cell::blank()),
                );
            }
            rows.push(row);
        }
        chafa_sys::chafa_canvas_unref(canvas);
        Image::from_rows(rows).map_err(|_| RasterImageError::ChafaUnavailable)
    }
}

/// A rendered RGBA8 pixel buffer with its pixel width.
#[derive(Debug, Clone)]
pub struct RasterPixels {
    /// Width in pixels; the buffer is row-major with four bytes per pixel.
    pub width: u32,
    /// Pixel bytes, four per pixel, top row first.
    pub pixels: Vec<u8>,
}

/// Renders `source` into a full-cell-size RGBA8 buffer under the default limits.
pub(crate) fn render_rgba(
    source: &RasterImage,
    full_width: u16,
    full_height: u16,
    options: ImageRenderOptions,
) -> Result<RasterPixels, RasterImageError> {
    render_rgba_with_cell_size(
        source,
        full_width,
        full_height,
        options,
        Size::new(8, 16),
        &ResourceLimits::default(),
    )
}

/// Renders `source` into an RGBA8 buffer of `full_width` by `full_height` cells,
/// each cell `cell_pixels` pixels across.
///
/// Every dimension and byte count is checked before allocation; saturating
/// multiplication is deliberately avoided because it would turn an invalid
/// request into a huge allocation instead of a typed error, and the output
/// buffer is reserved fallibly so a capacity failure is a resource error rather
/// than a process abort.
pub fn render_rgba_with_cell_size(
    source: &RasterImage,
    full_width: u16,
    full_height: u16,
    options: ImageRenderOptions,
    cell_pixels: Size,
    limits: &ResourceLimits,
) -> Result<RasterPixels, RasterImageError> {
    let cell_width = u64::from(cell_pixels.width.max(1));
    let cell_height = u64::from(cell_pixels.height.max(1));
    let width_u64 = (u64::from(full_width) * cell_width).max(1);
    let height_u64 = (u64::from(full_height) * cell_height).max(1);
    let width = u32::try_from(width_u64).unwrap_or(u32::MAX);
    let height = u32::try_from(height_u64).unwrap_or(u32::MAX);
    let pixels = ResourceLimits::checked_area(width, height).map_err(RasterImageError::limit)?;
    limits
        .check_transform_pixels(pixels)
        .map_err(RasterImageError::limit)?;
    let bytes = pixels
        .checked_mul(4)
        .ok_or(LimitError::Overflow {
            what: "RGBA byte length",
        })
        .map_err(RasterImageError::limit)?;
    let bytes = usize::try_from(bytes).map_err(|_| {
        RasterImageError::limit(LimitError::Overflow {
            what: "RGBA byte length",
        })
    })?;
    let mut full: Vec<u8> = Vec::new();
    full.try_reserve_exact(bytes).map_err(|_| {
        RasterImageError::limit(LimitError::Exceeded {
            resource: crate::runtime::limits::ImageResource::TransformPixels,
            limit: limits.max_transform_pixels,
            requested: bytes as u64,
        })
    })?;
    full.resize(bytes, 0);
    let source_image = ::image::ImageBuffer::<::image::Rgba<u8>, _>::from_raw(
        source.width(),
        source.height(),
        source.rgba8(),
    )
    .ok_or(RasterImageError::InvalidLength {
        expected: (source.width() as usize)
            .saturating_mul(source.height() as usize)
            .saturating_mul(4),
        actual: source.rgba8().len(),
    })?;
    let (scaled_width, scaled_height) =
        scaled_size(source.width(), source.height(), width, height, options.fit);
    let scaled = ::image::imageops::resize(
        &source_image,
        scaled_width,
        scaled_height,
        ::image::imageops::FilterType::Triangle,
    );
    let offset_x = align_offset(width, scaled_width, options.horizontal_align);
    let offset_y = align_offset(height, scaled_height, options.vertical_align);
    for y in 0..scaled_height {
        let target_y = offset_y + i64::from(y);
        if !(0..i64::from(height)).contains(&target_y) {
            continue;
        }
        for x in 0..scaled_width {
            let target_x = offset_x + i64::from(x);
            if !(0..i64::from(width)).contains(&target_x) {
                continue;
            }
            let dst = ((target_y as u32 * width + target_x as u32) * 4) as usize;
            full[dst..dst + 4].copy_from_slice(&scaled.get_pixel(x, y).0);
        }
    }
    Ok(RasterPixels {
        width,
        pixels: full,
    })
}

/// Copies the tile at `tile_column` and `tile_line` out of a full render into a
/// new RGBA8 buffer of `tile_width` by `tile_height` cells.
pub(crate) fn crop_rgba(
    full: &RasterPixels,
    tile_column: u16,
    tile_line: u16,
    tile_width: u16,
    tile_height: u16,
) -> Vec<u8> {
    const CELL_WIDTH: u32 = 8;
    const CELL_HEIGHT: u32 = 16;
    let x0 = u32::from(tile_column).saturating_mul(CELL_WIDTH);
    let y0 = u32::from(tile_line).saturating_mul(CELL_HEIGHT);
    let tile_pixels_w = u32::from(tile_width).saturating_mul(CELL_WIDTH);
    let tile_pixels_h = u32::from(tile_height).saturating_mul(CELL_HEIGHT);
    let mut result = vec![
        0;
        tile_pixels_w
            .saturating_mul(tile_pixels_h)
            .saturating_mul(4) as usize
    ];
    for row in 0..tile_pixels_h {
        let src = (((y0 + row) * full.width + x0) * 4) as usize;
        let dst = (row * tile_pixels_w * 4) as usize;
        result[dst..dst + tile_pixels_w as usize * 4]
            .copy_from_slice(&full.pixels[src..src + tile_pixels_w as usize * 4]);
    }
    result
}

fn scaled_size(source_w: u32, source_h: u32, width: u32, height: u32, fit: ImageFit) -> (u32, u32) {
    if fit == ImageFit::Stretch {
        return (width, height);
    }
    let width_ratio = width as f64 / source_w.max(1) as f64;
    let height_ratio = height as f64 / source_h.max(1) as f64;
    let ratio = match fit {
        ImageFit::Contain => width_ratio.min(height_ratio),
        ImageFit::Cover => width_ratio.max(height_ratio),
        ImageFit::Stretch => unreachable!(),
    };
    (
        ((source_w as f64 * ratio).round() as u32).max(1),
        ((source_h as f64 * ratio).round() as u32).max(1),
    )
}

fn align_offset(space: u32, content: u32, align: ImageAlign) -> i64 {
    match align {
        ImageAlign::Start => 0,
        ImageAlign::Center => (i64::from(space) - i64::from(content)) / 2,
        ImageAlign::End => i64::from(space) - i64::from(content),
    }
}

#[cfg(feature = "native-raster")]
fn packed_color(value: i32) -> Color {
    if value < 0 {
        Color::Reset
    } else {
        Color::Rgb {
            r: ((value >> 16) & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: (value & 0xff) as u8,
        }
    }
}
