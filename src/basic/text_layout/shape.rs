//! Text shaping: grapheme segmentation, separate/merge policy, and the shared
//! normalized buffer that glyph records index into.
#![allow(unused_imports)]

use super::*;

/// Shaping output: one shared normalized buffer plus glyph records that index
/// into it.
pub(super) struct ShapedText<S> {
    /// Concatenated normalized text that every glyph `text` range indexes into.
    pub(super) normalized: String,
    /// One record per addressable unit, in source order.
    pub(super) glyphs: Vec<ShapedGlyph<S>>,
}

/// Shapes a text value into one normalized buffer plus per-unit glyph records,
/// splitting graphemes into separate units unless `merging` joins them. Source
/// offsets are global to the whole `Text`, so each span contributes its
/// normalized length to the running origin, and a separated unit owns its own
/// codepoints' source range so a caret boundary exists between the parts.
pub(super) fn shape_text<S>(
    text: &Text,
    inherited: ComputedText,
    merging: EmojiMerging,
    merge: impl Fn(ComputedText, &TextStyle) -> S,
) -> ShapedText<S>
where
    S: Clone,
{
    crate::runtime::metrics::note_text_shaping();
    let mut normalized = String::new();
    let mut shaped = Vec::new();
    let mut source_origin = 0usize;
    for span in &text.spans {
        let style = merge(inherited, &span.style);
        let content = span.content.replace("\r\n", "\n").replace('\r', "\n");
        for (source_index, grapheme) in content.grapheme_indices(true) {
            if !merging.merges()
                && let Some(parts) = crate::data::separate_units(grapheme)
            {
                for (range, width) in parts {
                    let start = normalized.len();
                    normalized.push_str(&grapheme[range.clone()]);
                    shaped.push(ShapedGlyph {
                        source: source_origin + source_index + range.start
                            ..source_origin + source_index + range.end,
                        text: start..normalized.len(),
                        width,
                        kind: ItemKind::Glyph,
                        style: style.clone(),
                    });
                }
                continue;
            }
            let (symbol, width, kind) = display_glyph(grapheme);
            let start = normalized.len();
            normalized.push_str(&symbol);
            shaped.push(ShapedGlyph {
                source: source_origin + source_index..source_origin + source_index + grapheme.len(),
                text: start..normalized.len(),
                width,
                kind,
                style: style.clone(),
            });
        }
        source_origin += content.len();
    }
    ShapedText {
        normalized,
        glyphs: shaped,
    }
}

/// Maps one grapheme to its display symbol, cell width, and glyph kind. Newlines
/// and tabs get width 0 (tabs are column-relative, and `TextLayout::layout`
/// fixes their width from the logical line column); control, over-long, or
/// out-of-range-width graphemes become U+FFFD at width 1.
fn display_glyph(grapheme: &str) -> (String, usize, ItemKind) {
    if grapheme == "\n" {
        return ("\n".to_string(), 0, ItemKind::Newline);
    }
    if grapheme == "\t" {
        return ("\t".to_string(), 0, ItemKind::Glyph);
    }
    if grapheme.chars().any(char::is_control) {
        return ("\u{FFFD}".to_string(), 1, ItemKind::Glyph);
    }
    let symbol = grapheme.to_string();
    let width = UnicodeWidthStr::width(symbol.as_str());
    if symbol.len() > MAX_GLYPH_BYTES || !(1..=2).contains(&width) {
        return ("\u{FFFD}".to_string(), 1, ItemKind::Glyph);
    }
    (symbol, width, ItemKind::Glyph)
}
