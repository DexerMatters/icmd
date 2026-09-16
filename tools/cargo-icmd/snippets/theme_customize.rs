//! Customizing a preset without leaving the theme half-derived.

use icmd::theme::{ThemeBuilder, ThemeColors, ThemeMode, ThemePreset};
use icmd::{
    BorderKind, ComponentContext, Dimension, Edges, Node, Props, Text, card, theme_provider, ui,
};

/// Start from a preset, then override resolved tokens deliberately.
pub fn themed_panel(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let custom = ThemeBuilder::from_palette(ThemeMode::Dark, ThemeColors::dark())
        .borders(icmd::theme::ThemeBorders {
            kind: BorderKind::Double,
            edges: Edges::all(true),
            foreground: theme.colors.accent,
            background: theme.colors.card,
        })
        .build();

    ui! {
        <theme_provider value={custom}>
            <card style={|style| {
                style.layout /= icmd::Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
                style.padding /= Edges::all(1);
            }}>
                {Text::new("A nested palette").bold()}
                {Text::new("Derived tokens come from the builder, not from late edits.")}
            </card>
        </theme_provider>
    }
}

/// Presets are complete literals in both modes, so a replacement is total.
pub fn preset_pair() -> (icmd::theme::Theme, icmd::theme::Theme) {
    (
        ThemePreset::Nord.theme(ThemeMode::Dark),
        ThemePreset::Latte.theme(ThemeMode::Light),
    )
}
