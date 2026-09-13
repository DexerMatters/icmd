use std::{
    error::Error,
    fmt, fs,
    hash::{Hash, Hasher},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crossterm::style::{Attributes, Color};

use crate::{Cell, Image, Size};

static NEXT_RASTER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RasterImageError {
    Empty,
    InvalidLength { expected: usize, actual: usize },
    DimensionsTooLarge { width: u32, height: u32 },
    Decode(String),
    Io(String),
    ChafaUnavailable,
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
            Self::Decode(error) => write!(f, "could not decode image: {error}"),
            Self::Io(error) => write!(f, "could not read image: {error}"),
            Self::ChafaUnavailable => write!(f, "Chafa could not create an image canvas"),
        }
    }
}

impl Error for RasterImageError {}

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

    pub fn decode(bytes: impl AsRef<[u8]>) -> Result<Self, RasterImageError> {
        let decoded = ::image::load_from_memory(bytes.as_ref())
            .map_err(|error| RasterImageError::Decode(error.to_string()))?
            .to_rgba8();
        Self::from_rgba8(decoded.width(), decoded.height(), decoded.into_raw())
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, RasterImageError> {
        let bytes = fs::read(path).map_err(|error| RasterImageError::Io(error.to_string()))?;
        Self::decode(bytes)
    }

    pub const fn width(&self) -> u32 {
        self.width
    }
    pub const fn height(&self) -> u32 {
        self.height
    }
    pub const fn id(&self) -> u64 {
        self.id
    }
    pub fn rgba8(&self) -> &[u8] {
        &self.pixels
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageSource {
    Loaded(RasterImage),
    File(Arc<PathBuf>),
}

impl ImageSource {
    pub fn loaded(image: RasterImage) -> Self {
        Self::Loaded(image)
    }

    pub fn file(path: impl AsRef<Path>) -> Self {
        Self::File(Arc::new(normalize_path(path.as_ref().to_path_buf())))
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Loaded(_) => None,
            Self::File(path) => Some(path.as_path()),
        }
    }

    pub(crate) fn load(&self) -> Result<RasterImage, RasterImageError> {
        match self {
            Self::Loaded(image) => Ok(image.clone()),
            Self::File(path) => RasterImage::open(path.as_path()),
        }
    }

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

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    let mut rooted = false;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if rooted => {
                normalized.pop();
            }
            Component::ParentDir => {
                let can_pop = normalized
                    .file_name()
                    .is_some_and(|name| name != std::ffi::OsStr::new(".."));
                if can_pop {
                    normalized.pop();
                } else {
                    normalized.push("..");
                }
            }
            Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::RootDir => {
                rooted = true;
                normalized.push(component.as_os_str());
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageLoading {
    #[default]
    Lazy,
    Eager,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageProtocol {
    #[default]
    Auto,
    Kitty,
    Sixel,
    Iterm2,
    Symbols,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageUpdatePolicy {
    #[default]
    Adaptive,
    NativeOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageMode {
    #[default]
    Auto,
    Native,
    Symbols,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageFit {
    #[default]
    Contain,
    Cover,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageAlign {
    Start,
    #[default]
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageRenderOptions {
    pub fit: ImageFit,
    pub horizontal_align: ImageAlign,
    pub vertical_align: ImageAlign,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterPlacement {
    pub source: ImageSource,
    pub width: u16,
    pub height: u16,
    pub options: ImageRenderOptions,
    pub(crate) loading: ImageLoading,
    pub(crate) invalid_source: bool,
    pub(crate) full_width: u16,
    pub(crate) full_height: u16,
}

impl RasterPlacement {
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

    pub fn source(&self) -> &ImageSource {
        &self.source
    }

    pub fn with_loading(mut self, loading: ImageLoading) -> Self {
        self.loading = loading;
        self
    }
}

pub(crate) fn symbols(
    source: &RasterImage,
    width: u16,
    height: u16,
    _options: ImageRenderOptions,
) -> Result<Image, RasterImageError> {
    symbols_tile(source, width, height, 0, 0, width, height, _options)
}

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
    let full = render_rgba(source, full_width, full_height, options);
    let pixels = crop_rgba(&full, column, line, width, height);
    symbols_from_rgba(
        &pixels,
        width,
        height,
        usize::from(width) * 8 * 4,
        Size::new(8, 16),
    )
}

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

// A pure-Rust engine renders each cell as a colored block derived from the
// average of the pixels it covers. It is deliberately simple: it keeps the
// no-default-features build useful for raster previews without asserting the
// quality of the native symbol engine.
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

#[derive(Debug, Clone)]
pub(crate) struct RasterPixels {
    pub(crate) width: u32,
    pub(crate) pixels: Vec<u8>,
}

pub(crate) fn render_rgba(
    source: &RasterImage,
    full_width: u16,
    full_height: u16,
    options: ImageRenderOptions,
) -> RasterPixels {
    render_rgba_with_cell_size(source, full_width, full_height, options, Size::new(8, 16))
}

pub(crate) fn render_rgba_with_cell_size(
    source: &RasterImage,
    full_width: u16,
    full_height: u16,
    options: ImageRenderOptions,
    cell_pixels: Size,
) -> RasterPixels {
    let width = u32::from(full_width)
        .saturating_mul(u32::from(cell_pixels.width.max(1)))
        .max(1);
    let height = u32::from(full_height)
        .saturating_mul(u32::from(cell_pixels.height.max(1)))
        .max(1);
    let mut full = vec![0; width.saturating_mul(height).saturating_mul(4) as usize];
    let source_image = ::image::ImageBuffer::<::image::Rgba<u8>, _>::from_raw(
        source.width(),
        source.height(),
        source.rgba8(),
    )
    .expect("RasterImage validates RGBA8 length");
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
    RasterPixels {
        width,
        pixels: full,
    }
}

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
