//! Chapter 08 — Themes.
//!
//! Explains the semantic design system while the guide itself proves it works:
//! the header controls restyle everything below, and a nested provider proves
//! that a local preview stays local.

use crossterm::style::Color;
use icmd::theme::{
    MIN_TEXT_CONTRAST, Theme, ThemeBorders, ThemeMode, ThemePreset, ThemeSpacing, ThemeTypography,
    contrast_ratio,
};
use icmd::{
    Align, BorderKind, Component, ComponentContext, Dimension, Edges, Layout, Node, Props, Span,
    Text, TextWrap, card, muted, row, theme_provider, ui, view,
};

use super::{ChapterProps, document, masthead, section};
use crate::docs::{self, ApiRow};
use crate::metadata::{self, SectionMeta};
use crate::snippets;

/// One resolved color role shown as a readable foreground/background pairing.
struct Swatch {
    /// Role name.
    name: &'static str,
    /// Text color used inside the swatch.
    foreground: Color,
    /// Surface color behind the text.
    background: Color,
}

/// Every role in `ThemeColors`, each paired with the color that reads on it.
fn role_swatches(theme: &Theme) -> Vec<Swatch> {
    let colors = &theme.colors;
    vec![
        Swatch {
            name: "background",
            foreground: colors.foreground,
            background: colors.background,
        },
        Swatch {
            name: "foreground",
            foreground: colors.background,
            background: colors.foreground,
        },
        Swatch {
            name: "card",
            foreground: colors.card_foreground,
            background: colors.card,
        },
        Swatch {
            name: "card_foreground",
            foreground: colors.card,
            background: colors.card_foreground,
        },
        Swatch {
            name: "popover",
            foreground: colors.popover_foreground,
            background: colors.popover,
        },
        Swatch {
            name: "popover_foreground",
            foreground: colors.popover,
            background: colors.popover_foreground,
        },
        Swatch {
            name: "primary",
            foreground: colors.primary_foreground,
            background: colors.primary,
        },
        Swatch {
            name: "primary_foreground",
            foreground: colors.primary,
            background: colors.primary_foreground,
        },
        Swatch {
            name: "secondary",
            foreground: colors.secondary_foreground,
            background: colors.secondary,
        },
        Swatch {
            name: "secondary_foreground",
            foreground: colors.secondary,
            background: colors.secondary_foreground,
        },
        Swatch {
            name: "muted",
            foreground: colors.muted_foreground,
            background: colors.muted,
        },
        Swatch {
            name: "muted_foreground",
            foreground: colors.muted,
            background: colors.muted_foreground,
        },
        Swatch {
            name: "accent",
            foreground: colors.accent_foreground,
            background: colors.accent,
        },
        Swatch {
            name: "accent_foreground",
            foreground: colors.accent,
            background: colors.accent_foreground,
        },
        Swatch {
            name: "destructive",
            foreground: colors.destructive_foreground,
            background: colors.destructive,
        },
        Swatch {
            name: "destructive_foreground",
            foreground: colors.destructive,
            background: colors.destructive_foreground,
        },
        Swatch {
            name: "border",
            foreground: colors.background,
            background: colors.border,
        },
        Swatch {
            name: "input",
            foreground: colors.foreground,
            background: colors.input,
        },
        Swatch {
            name: "ring",
            foreground: colors.card_foreground,
            background: colors.ring,
        },
    ]
}

/// One role swatch: the name and a sample line, drawn in the pairing itself.
fn swatch_card(swatch: &Swatch, width: u16) -> Node {
    let name = swatch.name;
    let foreground = swatch.foreground;
    let background = swatch.background;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Cells(width);
            style.gap /= 0;
            style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
            style.background /= background;
        }}>
            {Text::new(name).foreground(foreground).bold()}
            {Text::new("Aa 123 · sample").foreground(foreground)}
        </view>
    }
}

/// Lays swatches out in fixed rows so the pairing is what varies, not the grid.
fn swatch_grid(swatches: &[Swatch], per_row: usize, cell: u16) -> Node {
    let per_row = per_row.max(1);
    let mut rows: Vec<Node> = Vec::new();
    for chunk in swatches.chunks(per_row) {
        let cells = chunk
            .iter()
            .map(|swatch| swatch_card(swatch, cell))
            .collect::<Node>();
        rows.push(ui! {
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Start;
                style.gap /= 1;
            }}>
                {cells}
            </row>
        });
    }
    rows.into_iter().collect::<Node>()
}

/// One preset-and-mode chip drawn in that preset's own palette.
fn preset_chip(preset: ThemePreset, mode: ThemeMode, cell: u16) -> Node {
    let theme = preset.theme(mode);
    let background = theme.colors.background;
    let foreground = theme.colors.foreground;
    let primary = theme.colors.primary;
    let on_primary = theme.colors.primary_foreground;
    let name = preset.name();
    let mode_label = match mode {
        ThemeMode::Dark => "☾ dark",
        ThemeMode::Light => " light",
    };
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Cells(cell);
            style.gap /= 0;
            style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
            style.background /= background;
        }}>
            {Text::new(name).foreground(foreground).bold()}
            <view style={move |style| {
                style.width /= Dimension::Max;
                style.padding /= Edges::symmetric(0, 1);
                style.background /= primary;
            }}>
                {Text::new(mode_label).foreground(on_primary)}
            </view>
        </view>
    }
}

/// Every preset crossed with both modes, laid out as chips.
fn preset_grid(per_row: usize, cell: u16) -> Node {
    let chips: Vec<Node> = ThemePreset::ALL
        .iter()
        .flat_map(|preset| {
            [ThemeMode::Dark, ThemeMode::Light]
                .into_iter()
                .map(move |mode| preset_chip(*preset, mode, cell))
        })
        .collect();
    let per_row = per_row.max(1);
    let mut rows: Vec<Node> = Vec::new();
    for chunk in chips.chunks(per_row) {
        let cells = chunk.iter().cloned().collect::<Node>();
        rows.push(ui! {
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Start;
                style.gap /= 1;
            }}>
                {cells}
            </row>
        });
    }
    rows.into_iter().collect::<Node>()
}

/// Typography tokens as readable specimens rather than a debug dump.
fn typography_specimen(theme: &Theme) -> Node {
    let typography: &ThemeTypography = &theme.typography;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>
            {Text::new("body · default prose").text_style(typography.body.clone())}
            {Text::new("heading · derived bold").text_style(typography.heading.clone())}
            {Text::new("label · control labels").text_style(typography.label.clone())}
            {Text::new("muted · secondary notes").text_style(typography.muted.clone())}
            {Text::new("code · accent foreground").text_style(typography.code.clone())}
        </view>
    }
}

/// Spacing tokens drawn as measured bars.
fn spacing_specimen(theme: &Theme) -> Node {
    let spacing: ThemeSpacing = theme.spacing;
    let steps = [
        ("xs", spacing.xs),
        ("sm", spacing.sm),
        ("md", spacing.md),
        ("lg", spacing.lg),
        ("xl", spacing.xl),
    ];
    let primary = theme.colors.primary;
    let rows = steps
        .iter()
        .map(|(name, value)| {
            let bar = "▪".repeat(usize::from(*value));
            ui! {
                <row style={|style| {
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.gap /= 1;
                }}>
                    <muted>{Text::new(format!("{name:<2} {value:>2} cells"))}</muted>
                    {Text::new(bar).foreground(primary)}
                </row>
            }
        })
        .collect::<Node>();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>{rows}</view>
    }
}

/// Border and scrollbar tokens resolved from the same palette.
fn border_scrollbar_specimen(theme: &Theme) -> Node {
    let borders: ThemeBorders = theme.borders;
    let scrollbar = theme.scrollbar.clone();
    let border_line = format!(
        "borders · {:?} · top {} right {} bottom {} left {}",
        borders.kind,
        borders.edges.top,
        borders.edges.right,
        borders.edges.bottom,
        borders.edges.left
    );
    let scrollbar_line = format!(
        "scrollbar · track {} thumb {} · {} {}",
        scrollbar.vertical_track.symbol(),
        scrollbar.vertical_thumb.symbol(),
        scrollbar.horizontal_track.symbol(),
        scrollbar.horizontal_thumb.symbol()
    );
    ui! {
        <card style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::all(1);
            style.border.kind /= borders.kind;
            style.border.edges /= borders.edges;
            style.border.foreground /= borders.foreground;
            style.border.background /= borders.background;
        }}>
            {Text::new(border_line).wrap(TextWrap::Soft)}
            <muted>{Text::new(scrollbar_line).wrap(TextWrap::Soft)}</muted>
            <muted>{Text::new("Every one of these values came from the same nineteen colors.").wrap(TextWrap::Soft)}</muted>
        </card>
    }
}

/// A panel that reads whatever theme is provided above it.
fn nested_card(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let card_background = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let primary = theme.colors.primary;
    let accent = theme.colors.accent;
    let kind = theme.borders.kind;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::all(1);
            style.background /= card_background;
            style.text.foreground /= card_foreground;
            style.border.kind /= kind;
            style.border.foreground /= accent;
            style.border.background /= card_background;
        }}>
            {Text::from_spans([
                Span::new("nested_card").foreground(primary).bold(),
                Span::new("  reads cx.use_theme()").foreground(card_foreground),
            ])}
            <muted>{Text::new(format!("border {kind:?} · accent is this theme's accent"))}</muted>
        </view>
    }
}

/// Proves a local provider does not leak into the surrounding shell.
fn provider_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let shell = cx.use_theme();
    let alien = ThemePreset::Geek.theme(ThemeMode::Dark).customize(|theme| {
        theme.borders.kind = BorderKind::Double;
    });
    let caption_color = shell.colors.muted_foreground;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {Text::new("① shell theme").foreground(caption_color).bold()}
            {nested_card.apply(())}
            {Text::new("② inside theme_provider").foreground(caption_color).bold()}
            <theme_provider value={alien}>
                {nested_card.apply(())}
            </theme_provider>
            {Text::new("③ shell theme again").foreground(caption_color).bold()}
            {nested_card.apply(())}
        </view>
    }
}

/// One contrast measurement against a surface.
struct ContrastCheck {
    /// What is being measured.
    label: &'static str,
    /// Ratio between the color and its surface.
    ratio: f64,
}

/// Measures a few representative roles against the surfaces they are used on.
fn contrast_checks(theme: &Theme) -> Vec<ContrastCheck> {
    let colors = &theme.colors;
    [
        (
            "foreground on background",
            colors.foreground,
            colors.background,
        ),
        (
            "card_foreground on card",
            colors.card_foreground,
            colors.card,
        ),
        (
            "muted_foreground on background",
            colors.muted_foreground,
            colors.background,
        ),
        ("primary on card", colors.primary, colors.card),
        ("accent on card", colors.accent, colors.card),
        ("destructive on card", colors.destructive, colors.card),
        ("border on card", colors.border, colors.card),
    ]
    .into_iter()
    .map(|(label, color, surface)| ContrastCheck {
        label,
        ratio: contrast_ratio(color, surface),
    })
    .collect()
}

/// Renders the measurements with a textual pass/fail cue.
fn contrast_table(theme: &Theme) -> Node {
    let checks = contrast_checks(theme);
    let pass = theme.colors.secondary;
    let fail = theme.colors.destructive;
    let muted_foreground = theme.colors.muted_foreground;
    let rows = checks
        .iter()
        .map(|check| {
            let (marker, color) = if check.ratio >= MIN_TEXT_CONTRAST {
                ("✓", pass)
            } else {
                ("✗", fail)
            };
            let ratio = if check.ratio.is_finite() {
                format!("{:.2}:1", check.ratio)
            } else {
                String::from("∞")
            };
            let label = check.label;
            ui! {
                <row style={|style| {
                    style.width /= Dimension::Max;
                    style.align /= Align::Center;
                    style.gap /= 1;
                }}>
                    {Text::new(marker).foreground(color).bold()}
                    <muted>{Text::new(format!("{ratio:>8}"))}</muted>
                    {Text::new(label).foreground(muted_foreground).wrap(TextWrap::Soft)}
                </row>
            }
        })
        .collect::<Node>();
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
        }}>{rows}</view>
    }
}

/// Shows the readable substitute for a role that fails as text on a card.
fn on_card_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let theme = cx.use_theme();
    let raw = theme.colors.border;
    let substituted = theme.on_card(raw);
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::all(1);
            style.background /= card;
        }}>
            {Text::from_spans([
                Span::new("raw border role as text: ").foreground(card_foreground),
                Span::new("may be unreadable").foreground(raw),
            ])}
            {Text::from_spans([
                Span::new("after Theme::on_card: ").foreground(card_foreground),
                Span::new("always readable on the card").foreground(substituted).bold(),
            ])}
        </view>
    }
}

/// A panel whose border and heading tokens were overridden deliberately.
fn customized_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let shell = cx.use_theme();
    let custom = ThemePreset::Nord.theme(ThemeMode::Dark).customize(|theme| {
        theme.borders.kind = BorderKind::Double;
        theme.borders.foreground = theme.colors.accent;
        theme.typography.heading = theme.typography.heading.clone().italic();
    });
    let card_background = custom.colors.card;
    let card_foreground = custom.colors.card_foreground;
    let accent = custom.colors.accent;
    let kind = custom.borders.kind;
    let heading_style = custom.typography.heading.clone();
    let shell_note = shell.colors.muted_foreground;
    ui! {
        <view style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {Text::new("Customized panel").text_style(heading_style)}
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.padding /= Edges::all(1);
                style.background /= card_background;
                style.text.foreground /= card_foreground;
                style.border.kind /= kind;
                style.border.foreground /= accent;
                style.border.background /= card_background;
            }}>
                {Text::from_spans([
                    Span::new("borders: ").foreground(card_foreground),
                    Span::new(format!("{kind:?}")).foreground(accent).bold(),
                ])}
                <muted>{Text::new("the heading token is italic now; the palette is unchanged").wrap(TextWrap::Soft)}</muted>
            </view>
            {Text::new("the guide around this panel is untouched").foreground(shell_note)}
        </view>
    }
}

/// Chapter 08.
pub(super) fn themes(cx: &mut ComponentContext, props: &Props<ChapterProps>) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(7);
    let sections: &[SectionMeta] = meta.sections;

    let roles = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body("A theme is not a list of nice colors. It is nineteen named roles, each with a partner guaranteed to read on it. Application code names the role it means, and the palette decides what that looks like in light, dark, ANSI, or any preset.")}
            {docs::body("The swatches below are drawn with the resolved colors themselves: the text color is the role and the surface is its partner. If a pairing were unreadable, the specimen would be unreadable too, which is the point.")}
            {docs::specimen(&theme, "nineteen roles, foreground on its partner", ui! {
                {swatch_grid(&role_swatches(&theme), if data.wide() { 3 } else { 2 }, if data.wide() { 21 } else { 24 })}
            })}
            {docs::notice(&theme, "Surface roles and text roles are distinct. `primary` is a background for filled controls; `primary_foreground` is what reads on it. Pairing them is the theme's job, not the caller's.")}
            {docs::watch_for(&theme, "Reaching for a raw terminal color breaks every guarantee the theme makes: it ignores contrast, the light/dark switch, and the header controls.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ThemeColors", purpose: "the nineteen base roles", defaults: "every preset supplies all nineteen", events: "none" },
                ApiRow { name: "Theme", purpose: "colors plus derived tokens", defaults: "Theme::ansi(ThemeMode::Light)", events: "none" },
                ApiRow { name: "use_theme", purpose: "read the resolved theme from context", defaults: "the provider's value or Theme::default()", events: "none" },
            ])}
        },
    );

    let presets = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body("Eight presets, each a complete literal in both modes. Every chip below is painted with the palette it names, so switching the guide's own theme in the header is the same operation applied to the whole document.")}
            {docs::specimen(&theme, "eight presets × light and dark", ui! {
                {preset_grid(if data.wide() { 4 } else { 2 }, if data.wide() { 22 } else { 24 })}
            })}
            {docs::body("Theme choice lives in the shell, so it survives chapter changes and never touches disk. Use the ` preset` and mode buttons in the header: they cycle every `ThemePreset::ALL` value and toggle `ThemeMode`, and the whole guide — chrome, code, and every demonstration — follows.")}
            {docs::notice(&theme, "Because a preset replaces all nineteen roles, adding a role to `ThemeColors` is a compile error in every preset rather than a silently missing color.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ThemePreset", purpose: "named palettes", defaults: "ThemePreset::ALL lists all eight", events: "none" },
                ApiRow { name: "ThemePreset::theme", purpose: "build a preset for a mode", defaults: "requires an explicit ThemeMode", events: "none" },
                ApiRow { name: "ThemeMode", purpose: "light or dark", defaults: "ThemeMode::Light", events: "none" },
            ])}
        },
    );

    let tokens = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body("Palette roles are only half a theme. Typography, spacing, borders, and scrollbars are derived from those roles once, at construction, and are then independent resolved values.")}
            {docs::two_column(
                &theme,
                data.wide(),
                ui! {
                    {docs::specimen(&theme, "typography roles", ui! { {typography_specimen(&theme)} })}
                    {docs::specimen(&theme, "spacing steps", ui! { {spacing_specimen(&theme)} })}
                },
                ui! {
                    {docs::specimen(&theme, "borders and scrollbars", ui! { {border_scrollbar_specimen(&theme)} })}
                },
            )}
            {docs::notice(&theme, "Widgets consume these tokens, which is why a card, an input, and an alert share one border treatment and one scrollbar treatment without any of them restating it.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ThemeTypography", purpose: "body, heading, label, muted, code", defaults: "derived from the palette", events: "none" },
                ApiRow { name: "ThemeSpacing", purpose: "xs through xl, in cells", defaults: "0, 1, 1, 2, 3", events: "none" },
                ApiRow { name: "ThemeBorders", purpose: "kind, edges, foreground, background", defaults: "rounded, all edges", events: "none" },
                ApiRow { name: "ScrollbarStyle", purpose: "track and thumb glyphs plus styles", defaults: "vertical and horizontal pairs", events: "none" },
            ])}
        },
    );

    let customize = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body("Customization starts from a complete palette and overrides resolved tokens deliberately. `ThemePreset::theme` gives you the preset, `ThemeBuilder` assembles a theme from parts, and `Theme::customize` patches one you already have.")}
            {docs::live_example(
                &theme,
                "a customized derivation",
                "The panel below keeps the palette but doubles the border and italicizes the heading token.",
                ui! { {customized_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "builder, override, then build", snippets::THEME_CUSTOMIZE)}
            {docs::watch_for(&theme, "Do not mutate `colors` after deriving: typography, borders, and scrollbar were already computed from the old palette, so the theme ends up half-updated. Change the palette first, or override the derived token directly.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ThemeBuilder::from_palette", purpose: "derive every token from a palette", defaults: "typography, borders, scrollbar derived", events: "none" },
                ApiRow { name: "Theme::customize", purpose: "patch an existing theme in place", defaults: "runs once at construction", events: "none" },
                ApiRow { name: "Theme::from_parts", purpose: "assemble without re-deriving", defaults: "explicit parts", events: "none" },
            ])}
        },
    );

    let providers = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body("A provider installs a theme for one subtree. The same component is mounted three times below; only the second mount sits inside a provider, and it is the only one that changes.")}
            {docs::live_example(
                &theme,
                "one component, two themes",
                "The middle card is provided a Geek Dark theme with a double border; the outer two keep the shell's theme.",
                ui! { {provider_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "proof", "the third card matches the first, so the local preview did not leak")} }),
            )}
            {docs::notice(&theme, "Providers compose. The nearest provider wins for the subtree it wraps, and a consumer that never finds one falls back to the key's default rather than failing.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "theme_provider", purpose: "install a theme for a subtree", defaults: "Theme::default() when no value is set", events: "none" },
                ApiRow { name: "theme_context", purpose: "the context key behind the provider", defaults: "process-unique, with a default theme", events: "none" },
                ApiRow { name: "use_theme", purpose: "read the nearest provided theme", defaults: "Arc<Theme>, cheap to clone", events: "none" },
            ])}
        },
    );

    let contrast = section(
        &theme,
        data,
        5,
        &sections[5],
        ui! {
            {docs::body("Readability is measurable. `contrast_ratio` returns the WCAG ratio between two colors, and `MIN_TEXT_CONTRAST` is the floor this framework uses when it substitutes a readable color for an unreadable one.")}
            {docs::specimen(&theme, "measured against the surface each is used on", ui! {
                {contrast_table(&theme)}
            })}
            {docs::body("Color is never the only cue. A badge carries its label, an alert carries a title, a spinner pairs its glyph with text, a progress bar prints a percentage, and this table prints a symbol beside every ratio. `Theme::on_card` is the automatic version of the same rule.")}
            {docs::live_example(
                &theme,
                "the readable substitute",
                "A low-contrast role used as text is replaced by the card foreground.",
                ui! { {on_card_demo.apply(())} },
                None,
            )}
            {docs::api_strip(&theme, &[
                ApiRow { name: "contrast_ratio", purpose: "WCAG ratio between two colors", defaults: "infinite for indexed or reset colors", events: "none" },
                ApiRow { name: "MIN_TEXT_CONTRAST", purpose: "the substitution floor", defaults: "3.0", events: "none" },
                ApiRow { name: "Theme::on_card", purpose: "return the color or the card foreground", defaults: "compares against the card surface", events: "none" },
            ])}
            {docs::production_note(&theme, "Test a theme in both modes. A palette that reads well in dark mode can fail badly in light, and the header toggle makes that a one-keystroke check.")}
        },
    );

    let background = theme.colors.background;
    document(
        data,
        ui! {
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.background /= background;
            }}>
                {masthead(&theme, meta, data.section_count())}
                {roles}
                {presets}
                {tokens}
                {customize}
                {providers}
                {contrast}
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}
