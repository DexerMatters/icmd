// Glyph policy and cross-type characterization. The canonical validator lives
// behind the hidden module; every public domain wrapper is exercised beside it.
use icmd::__private::{AllowedGlyphWidth, GlyphError, validate_terminal_glyph};
use icmd::{Cell, CellError, Fill, FillError, MAX_GLYPH_BYTES, ScrollbarGlyph};

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
