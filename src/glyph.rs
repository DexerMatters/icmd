use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::data::MAX_GLYPH_BYTES;

// The display width a caller permits for one terminal glyph. `Cell` accepts the
// full range a terminal can paint, `Fill` accepts the same range because a fill
// may be a wide block, and a scrollbar glyph requires exactly one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AllowedGlyphWidth {
    One,
    OneOrTwo,
}

// The one implementation of the terminal-glyph invariant: normalized content
// must be exactly one printable grapheme, within the byte budget, with an
// allowed display width and no control behavior. `Cell`, `Fill`, and scrollbar
// glyphs each keep their own public error type and extra domain rules; only the
// shared rule lives here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedTerminalGlyph {
    text: Arc<str>,
    width: u8,
}

impl ValidatedTerminalGlyph {
    pub(crate) fn width(&self) -> usize {
        usize::from(self.width)
    }

    pub(crate) fn into_text(self) -> Arc<str> {
        self.text
    }
}

// Why a glyph was rejected. Domain wrappers translate this into their own
// public error, so the shared validator never leaks into the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlyphError {
    Empty,
    SymbolTooLong(usize),
    MultipleGraphemes,
    ControlCharacter,
    UnsupportedWidth(usize),
}

pub(crate) fn validate_terminal_glyph(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_printable_single_graphemes_within_the_width_policy() {
        assert_eq!(
            validate_terminal_glyph("x", AllowedGlyphWidth::One)
                .unwrap()
                .width(),
            1
        );
        assert_eq!(
            validate_terminal_glyph("界", AllowedGlyphWidth::OneOrTwo)
                .unwrap()
                .width(),
            2
        );
        assert!(validate_terminal_glyph("界", AllowedGlyphWidth::One).is_err());
    }

    #[test]
    fn rejects_empty_control_multi_grapheme_and_zero_width() {
        assert_eq!(
            validate_terminal_glyph("", AllowedGlyphWidth::OneOrTwo),
            Err(GlyphError::Empty)
        );
        assert_eq!(
            validate_terminal_glyph("\u{7}", AllowedGlyphWidth::OneOrTwo),
            Err(GlyphError::ControlCharacter)
        );
        assert_eq!(
            validate_terminal_glyph("ab", AllowedGlyphWidth::OneOrTwo),
            Err(GlyphError::MultipleGraphemes)
        );
        assert_eq!(
            validate_terminal_glyph("\u{200b}", AllowedGlyphWidth::OneOrTwo),
            Err(GlyphError::UnsupportedWidth(0))
        );
    }

    #[test]
    fn rejects_symbols_over_the_byte_budget() {
        let long = "a".repeat(MAX_GLYPH_BYTES + 1);
        assert!(matches!(
            validate_terminal_glyph(&long, AllowedGlyphWidth::OneOrTwo),
            Err(GlyphError::SymbolTooLong(_))
        ));
    }
}

#[cfg(test)]
mod characterization {
    use super::*;
    use crate::{Cell, CellError, Fill, FillError, ScrollbarGlyph};

    // The plan's cross-type policy table: one input table drives the canonical
    // validator and every public domain wrapper, proving they agree on the
    // shared rule while keeping their own error types and extra policy.
    #[test]
    fn all_glyph_paths_agree_on_the_shared_rule() {
        // (input, canonical, cell, fill, scrollbar)
        // `None` means the path accepts the value.
        struct Case {
            input: &'static str,
            shared: Option<GlyphError>,
            scrollbar_accepts: bool,
        }
        let cases = [
            Case {
                input: "x",
                shared: None,
                scrollbar_accepts: true,
            },
            Case {
                input: "界",
                shared: None,
                // A wide glyph satisfies cell and fill, but not the
                // one-column scrollbar policy.
                scrollbar_accepts: false,
            },
            Case {
                input: "",
                shared: Some(GlyphError::Empty),
                scrollbar_accepts: false,
            },
            Case {
                input: "ab",
                shared: Some(GlyphError::MultipleGraphemes),
                scrollbar_accepts: false,
            },
            Case {
                input: "\u{7}",
                shared: Some(GlyphError::ControlCharacter),
                scrollbar_accepts: false,
            },
            Case {
                input: "\u{200b}",
                shared: Some(GlyphError::UnsupportedWidth(0)),
                scrollbar_accepts: false,
            },
        ];

        for case in cases {
            let shared = validate_terminal_glyph(case.input, AllowedGlyphWidth::OneOrTwo).err();
            assert_eq!(
                shared, case.shared,
                "canonical verdict for {:?}",
                case.input
            );

            let cell = Cell::plain(case.input).err();
            let fill = Fill::new(case.input).err();
            let scrollbar = ScrollbarGlyph::new(case.input).err();

            match case.shared {
                None => {
                    assert!(cell.is_none(), "cell must accept {:?}", case.input);
                    assert!(fill.is_none(), "fill must accept {:?}", case.input);
                }
                Some(_) => {
                    // Domain wrappers reject exactly the same shared rule and
                    // report it through their own public error type.
                    assert!(
                        matches!(
                            cell,
                            Some(
                                CellError::Empty
                                    | CellError::MultipleGraphemes
                                    | CellError::ControlCharacter
                                    | CellError::SymbolTooLong(_)
                                    | CellError::UnsupportedWidth(_)
                            )
                        ),
                        "cell verdict for {:?}",
                        case.input
                    );
                    assert!(
                        matches!(
                            fill,
                            Some(
                                FillError::Empty
                                    | FillError::MultipleGraphemes
                                    | FillError::ControlCharacter
                                    | FillError::SymbolTooLong(_)
                                    | FillError::UnsupportedWidth(_)
                            )
                        ),
                        "fill verdict for {:?}",
                        case.input
                    );
                }
            }
            assert_eq!(
                scrollbar.is_none(),
                case.scrollbar_accepts,
                "scrollbar verdict for {:?}",
                case.input
            );
        }
    }

    #[test]
    fn fill_caches_the_validated_width() {
        let fill = Fill::new("界").unwrap();
        assert_eq!(fill.width(), 2);
        // The cached width matches a fresh measurement of the stored symbol.
        assert_eq!(
            fill.width(),
            unicode_width::UnicodeWidthStr::width(fill.symbol())
        );
    }
}
