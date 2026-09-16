//! The shared validator for one printable terminal glyph.
//!
//! Owns the single implementation of the terminal-glyph invariant that [`Cell`],
//! fill, and scrollbar symbols all delegate to.
//!
//! [`Cell`]: crate::Cell

use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::data::MAX_GLYPH_BYTES;

/// The display width a caller permits for one terminal glyph.
///
/// `Cell` and `Fill` accept the full range a terminal can paint, while a
/// scrollbar glyph requires exactly one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowedGlyphWidth {
    /// Exactly one column.
    One,
    /// One or two columns.
    OneOrTwo,
}

/// A glyph that has passed the shared terminal-glyph invariant.
///
/// The invariant requires exactly one printable grapheme within the byte
/// budget, with an allowed display width and no control behavior. `Cell`,
/// `Fill`, and scrollbar glyphs keep their own public error type and extra
/// domain rules; only the shared rule lives here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedTerminalGlyph {
    text: Arc<str>,
    width: u8,
}

impl ValidatedTerminalGlyph {
    /// Returns the glyph's display width in columns, 1 or 2.
    pub fn width(&self) -> usize {
        usize::from(self.width)
    }

    pub(crate) fn into_text(self) -> Arc<str> {
        self.text
    }
}

/// Why a glyph was rejected.
///
/// Domain wrappers translate this into their own public error, so the shared
/// validator never leaks into the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphError {
    /// The glyph was empty.
    Empty,
    /// The glyph exceeded [`MAX_GLYPH_BYTES`]; carries its byte length.
    SymbolTooLong(usize),
    /// The glyph contained more than one grapheme.
    MultipleGraphemes,
    /// The glyph contained a control character.
    ControlCharacter,
    /// The display width was not permitted; carries the measured width.
    UnsupportedWidth(usize),
}

/// Validates one glyph as a single printable grapheme within the byte budget,
/// with a display width allowed by `allowed` and no control character.
pub fn validate_terminal_glyph(
    value: &str,
    allowed: AllowedGlyphWidth,
) -> Result<ValidatedTerminalGlyph, GlyphError> {
    if value.is_empty() {
        return Err(GlyphError::Empty);
    }
    if value.len() > MAX_GLYPH_BYTES {
        return Err(GlyphError::SymbolTooLong(value.len()));
    }
    if value.graphemes(true).count() != 1 {
        return Err(GlyphError::MultipleGraphemes);
    }
    if value.chars().any(char::is_control) {
        return Err(GlyphError::ControlCharacter);
    }
    let width = UnicodeWidthStr::width(value);
    let permitted = match allowed {
        AllowedGlyphWidth::One => width == 1,
        AllowedGlyphWidth::OneOrTwo => (1..=2).contains(&width),
    };
    if !permitted {
        return Err(GlyphError::UnsupportedWidth(width));
    }
    Ok(ValidatedTerminalGlyph {
        text: Arc::from(value),
        width: width as u8,
    })
}
