//! Validated resource budgets for every potentially unbounded unit of runtime
//! work. Each ceiling is checked before allocation or recursion, and
//! `ConfigError` rejects a self-contradictory policy before a runtime starts.

use std::error::Error;
use std::fmt;

/// Hard ceilings for every potentially unbounded unit of runtime work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Maximum accepted editable input text, in bytes.
    pub max_input_bytes: usize,
    /// Maximum accepted logical tree nodes.
    pub max_nodes: usize,
    /// Maximum accepted logical tree depth, bounding recursive traversal.
    pub max_tree_depth: usize,
    /// Maximum accepted encoded image payload, in bytes.
    pub max_encoded_image_bytes: usize,
    /// Maximum accepted source image width, in pixels.
    pub max_source_width: u32,
    /// Maximum accepted source image height, in pixels.
    pub max_source_height: u32,
    /// Maximum accepted source image area, in pixels.
    pub max_source_pixels: u64,
    /// Maximum decoded image size held in memory, in bytes.
    pub max_decoded_image_bytes: usize,
    /// Maximum decoded image bytes kept in flight at once, in bytes.
    pub max_in_flight_image_bytes: usize,
    /// Maximum pixels produced by one image transform.
    pub max_transform_pixels: u64,
    /// Maximum bytes retained in the renderer image cache.
    pub max_cache_bytes: usize,
    /// Maximum encoded output bytes emitted in one frame.
    pub max_output_bytes_per_frame: usize,
}

/// Shared editable-text ceiling, in bytes; the default policy derives from it
/// so the editor and the policy cannot drift.
pub(crate) const DEFAULT_MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_nodes: 1_000_000,
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
    /// Rejects a policy whose limits are zero or mutually inconsistent.
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
        if self.max_in_flight_image_bytes < self.max_decoded_image_bytes {
            return Err(ConfigError::Inconsistent {
                detail: "max_in_flight_image_bytes must cover one decoded image",
            });
        }
        Ok(())
    }

    /// Fallible pixel area for a width and height pair; overflow is reported
    /// rather than saturated into a huge allocation.
    pub fn checked_area(width: u32, height: u32) -> Result<u64, LimitError> {
        (width as u64)
            .checked_mul(height as u64)
            .ok_or(LimitError::Overflow { what: "pixel area" })
    }

    /// Checks a source image's width, height, and pixel area; returns the area
    /// in pixels.
    pub fn check_source_size(&self, width: u32, height: u32) -> Result<u64, LimitError> {
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

    /// Checks that one transformed image stays within the transform pixel budget.
    pub fn check_transform_pixels(&self, pixels: u64) -> Result<(), LimitError> {
        if pixels > self.max_transform_pixels {
            return Err(LimitError::Exceeded {
                resource: ImageResource::TransformPixels,
                limit: self.max_transform_pixels,
                requested: pixels,
            });
        }
        Ok(())
    }

    /// Checks that a decoded image size stays within the decoded byte budget.
    pub fn check_decoded_bytes(&self, bytes: usize) -> Result<(), LimitError> {
        if bytes as u64 > self.max_decoded_image_bytes as u64 {
            return Err(LimitError::Exceeded {
                resource: ImageResource::DecodedBytes,
                limit: self.max_decoded_image_bytes as u64,
                requested: bytes as u64,
            });
        }
        Ok(())
    }

    /// Checks that an encoded image payload stays within the encoded byte budget.
    pub fn check_encoded_bytes(&self, bytes: u64) -> Result<(), LimitError> {
        if bytes > self.max_encoded_image_bytes as u64 {
            return Err(LimitError::Exceeded {
                resource: ImageResource::EncodedBytes,
                limit: self.max_encoded_image_bytes as u64,
                requested: bytes,
            });
        }
        Ok(())
    }

    /// Checks that editable input text stays within the input byte budget.
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

/// A budget whose ceiling a limit error can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageResource {
    /// Editable input text, in bytes.
    InputBytes,
    /// Source image width, in pixels.
    SourceWidth,
    /// Source image height, in pixels.
    SourceHeight,
    /// Source image area, in pixels.
    SourcePixels,
    /// Encoded image payload, in bytes.
    EncodedBytes,
    /// Decoded image data, in bytes.
    DecodedBytes,
    /// Decoded image data held in flight, in bytes.
    InFlightBytes,
    /// Pixels produced by one image transform.
    TransformPixels,
    /// Bytes retained in the renderer image cache.
    CacheBytes,
}

impl ImageResource {
    /// Human-readable name of this resource, used in error messages.
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

/// A request that exceeded a configured resource ceiling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitError {
    /// A requested amount exceeded the configured ceiling.
    Exceeded {
        /// The budget that was exceeded.
        resource: ImageResource,
        /// The configured ceiling, in the unit named by `resource`.
        limit: u64,
        /// The amount requested, in the same unit as `limit`.
        requested: u64,
    },
    /// Arithmetic on a checked quantity overflowed its type.
    Overflow {
        /// Name of the quantity that overflowed.
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

/// A configuration rejected before a runtime starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A limit was configured as zero.
    InvalidLimit {
        /// Name of the offending limit field.
        field: &'static str,
    },
    /// Two limits contradict each other.
    Inconsistent {
        /// Description of the contradiction.
        detail: &'static str,
    },
    /// The poll interval is zero or exceeds 60 seconds.
    InvalidPollInterval {
        /// The rejected poll interval, in milliseconds.
        millis: u128,
    },
    /// The events-per-tick budget is zero.
    InvalidEventsPerTick,
    /// The renderer configuration is invalid.
    Renderer(RendererConfigError),
}

/// A renderer configuration rejected before a runtime starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendererConfigError {
    /// The renderer cell pixel width is zero.
    ZeroCellPixelWidth,
    /// The renderer cell pixel height is zero.
    ZeroCellPixelHeight,
    /// A single cell's pixel size can never fit the transform pixel budget.
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
