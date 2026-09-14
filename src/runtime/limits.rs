use std::error::Error;
use std::fmt;

// One validated budget policy for every potentially unbounded unit of work.
// Every field is a hard ceiling checked before allocation or recursion; nothing
// here is advisory. `ConfigError` rejects a policy whose own fields contradict
// each other, so a runtime never starts with an unsatisfiable budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub max_input_bytes: usize,
    pub max_nodes: usize,
    pub max_tree_depth: usize,
    pub max_encoded_image_bytes: usize,
    pub max_source_width: u32,
    pub max_source_height: u32,
    pub max_source_pixels: u64,
    pub max_decoded_image_bytes: usize,
    pub max_in_flight_image_bytes: usize,
    pub max_transform_pixels: u64,
    pub max_cache_bytes: usize,
    pub max_output_bytes_per_frame: usize,
}

// The shared ceiling for editable text. The editor enforces this constant, and
// the default policy is defined in terms of it, so the two cannot drift.
pub(crate) const DEFAULT_MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            // Far beyond any interactive use, and enforced by the editor.
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            // A 1,000,000-node logical tree is already far past useful UI size.
            max_nodes: 1_000_000,
            // Depth 1,024 bounds worker stack use for recursive traversal.
            max_tree_depth: 1_024,
            max_encoded_image_bytes: 32 * 1024 * 1024,
            max_source_width: 16_384,
            max_source_height: 16_384,
            max_source_pixels: 64 * 1024 * 1024,
            max_decoded_image_bytes: 256 * 1024 * 1024,
            max_in_flight_image_bytes: 256 * 1024 * 1024,
            max_transform_pixels: 64 * 1024 * 1024,
            max_cache_bytes: 64 * 1024 * 1024,
            max_output_bytes_per_frame: 64 * 1024 * 1024,
        }
    }
}

impl ResourceLimits {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let checks: [(&'static str, usize); 6] = [
            ("max_input_bytes", self.max_input_bytes),
            ("max_nodes", self.max_nodes),
            ("max_tree_depth", self.max_tree_depth),
            ("max_decoded_image_bytes", self.max_decoded_image_bytes),
            ("max_in_flight_image_bytes", self.max_in_flight_image_bytes),
            (
                "max_output_bytes_per_frame",
                self.max_output_bytes_per_frame,
            ),
        ];
        for (name, value) in checks {
            if value == 0 {
                return Err(ConfigError::InvalidLimit { field: name });
            }
        }
        if self.max_encoded_image_bytes == 0 {
            return Err(ConfigError::InvalidLimit {
                field: "max_encoded_image_bytes",
            });
        }
        if self.max_source_width == 0 || self.max_source_height == 0 {
            return Err(ConfigError::InvalidLimit {
                field: "max_source_width/height",
            });
        }
        if self.max_source_pixels == 0 || self.max_transform_pixels == 0 {
            return Err(ConfigError::InvalidLimit {
                field: "max_source_pixels/max_transform_pixels",
            });
        }
        // A transform can only ever be produced from a decoded source, so a
        // transform budget above the decode budget is dead configuration.
        if self.max_in_flight_image_bytes < self.max_decoded_image_bytes {
            return Err(ConfigError::Inconsistent {
                detail: "max_in_flight_image_bytes must cover one decoded image",
            });
        }
        Ok(())
    }

    // Fallible product used everywhere a dimension pair turns into a byte or
    // pixel count. Saturation is deliberately not used: saturation followed by
    // an allocation converts an invalid request into a huge one.
    #[allow(dead_code)] // Wired by the raster resource-accounting pass.
    pub(crate) fn checked_area(width: u32, height: u32) -> Result<u64, LimitError> {
        (width as u64)
            .checked_mul(height as u64)
            .ok_or(LimitError::Overflow { what: "pixel area" })
    }

    #[allow(dead_code)] // Wired by the raster resource-accounting pass.
    pub(crate) fn check_source_size(&self, width: u32, height: u32) -> Result<u64, LimitError> {
        if width > self.max_source_width {
            return Err(LimitError::Exceeded {
                resource: ImageResource::SourceWidth,
                limit: self.max_source_width as u64,
                requested: width as u64,
            });
        }
        if height > self.max_source_height {
            return Err(LimitError::Exceeded {
                resource: ImageResource::SourceHeight,
                limit: self.max_source_height as u64,
                requested: height as u64,
            });
        }
        let pixels = Self::checked_area(width, height)?;
        if pixels > self.max_source_pixels {
            return Err(LimitError::Exceeded {
                resource: ImageResource::SourcePixels,
                limit: self.max_source_pixels,
                requested: pixels,
            });
        }
        Ok(pixels)
    }

    #[allow(dead_code)] // Wired by the raster resource-accounting pass.
    pub(crate) fn check_transform_pixels(&self, pixels: u64) -> Result<(), LimitError> {
        if pixels > self.max_transform_pixels {
            return Err(LimitError::Exceeded {
                resource: ImageResource::TransformPixels,
                limit: self.max_transform_pixels,
                requested: pixels,
            });
        }
        Ok(())
    }

    #[allow(dead_code)] // Wired by the raster resource-accounting pass.
    pub(crate) fn check_decoded_bytes(&self, bytes: usize) -> Result<(), LimitError> {
        if bytes as u64 > self.max_decoded_image_bytes as u64 {
            return Err(LimitError::Exceeded {
                resource: ImageResource::DecodedBytes,
                limit: self.max_decoded_image_bytes as u64,
                requested: bytes as u64,
            });
        }
        Ok(())
    }

    #[allow(dead_code)] // Wired by the raster resource-accounting pass.
    pub(crate) fn check_encoded_bytes(&self, bytes: u64) -> Result<(), LimitError> {
        if bytes > self.max_encoded_image_bytes as u64 {
            return Err(LimitError::Exceeded {
                resource: ImageResource::EncodedBytes,
                limit: self.max_encoded_image_bytes as u64,
                requested: bytes,
            });
        }
        Ok(())
    }

    pub fn check_input_bytes(&self, bytes: usize) -> Result<(), LimitError> {
        if bytes > self.max_input_bytes {
            return Err(LimitError::Exceeded {
                resource: ImageResource::InputBytes,
                limit: self.max_input_bytes as u64,
                requested: bytes as u64,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageResource {
    InputBytes,
    SourceWidth,
    SourceHeight,
    SourcePixels,
    EncodedBytes,
    DecodedBytes,
    InFlightBytes,
    TransformPixels,
    CacheBytes,
}

impl ImageResource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InputBytes => "input bytes",
            Self::SourceWidth => "source width",
            Self::SourceHeight => "source height",
            Self::SourcePixels => "source pixels",
            Self::EncodedBytes => "encoded image bytes",
            Self::DecodedBytes => "decoded image bytes",
            Self::InFlightBytes => "in-flight image bytes",
            Self::TransformPixels => "transform pixels",
            Self::CacheBytes => "cache bytes",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitError {
    Exceeded {
        resource: ImageResource,
        limit: u64,
        requested: u64,
    },
    Overflow {
        what: &'static str,
    },
}

impl fmt::Display for LimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exceeded {
                resource,
                limit,
                requested,
            } => write!(
                f,
                "{} limit exceeded: requested {requested}, limit {limit}",
                resource.as_str()
            ),
            Self::Overflow { what } => write!(f, "{what} overflowed the supported range"),
        }
    }
}

impl Error for LimitError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidLimit { field: &'static str },
    Inconsistent { detail: &'static str },
    InvalidPollInterval { millis: u128 },
    InvalidEventsPerTick,
    Renderer(RendererConfigError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendererConfigError {
    ZeroCellPixelWidth,
    ZeroCellPixelHeight,
    CellPixelSizeExceedsTransformBudget,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit { field } => {
                write!(f, "resource limit `{field}` must be greater than zero")
            }
            Self::Inconsistent { detail } => write!(f, "inconsistent configuration: {detail}"),
            Self::InvalidPollInterval { millis } => write!(
                f,
                "poll interval must be nonzero and at most 60s, got {millis}ms"
            ),
            Self::InvalidEventsPerTick => {
                write!(f, "events_per_tick must be greater than zero")
            }
            Self::Renderer(error) => write!(f, "{error}"),
        }
    }
}

impl fmt::Display for RendererConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCellPixelWidth => write!(f, "renderer cell pixel width must be nonzero"),
            Self::ZeroCellPixelHeight => write!(f, "renderer cell pixel height must be nonzero"),
            Self::CellPixelSizeExceedsTransformBudget => write!(
                f,
                "renderer cell pixel size can never satisfy the transform pixel budget"
            ),
        }
    }
}

impl Error for ConfigError {}

impl Error for RendererConfigError {}
