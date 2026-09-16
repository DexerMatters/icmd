//! Theme system: palette presets and modes, the derived typography, spacing and
//! border tokens, and the context provider that shares one theme with the tree.
//! `ThemeColors` holds the nineteen base colors every other token derives from.

use std::sync::OnceLock;

use crossterm::style::Color;

use crate::{
    Attr, BorderKind, ComponentContext, ContextKey, Edges, Node, Props, ScrollbarStyle, Style,
    TextStyle, create_context, style,
};

/// Light or dark palette selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeMode {
    /// Light background with dark text.
    #[default]
    Light,
    /// Dark background with light text.
    Dark,
}

/// Named color palette; `Ansi` uses the terminal's own colors, the rest are
/// fixed RGB themes complete in both modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemePreset {
    /// The terminal's own sixteen ANSI colors.
    Ansi,
    /// Green phosphor terminal palette.
    Geek,
    /// Grayscale-only palette.
    Mono,
    /// Catppuccin Latte palette.
    Latte,
    /// Nord palette.
    Nord,
    /// Dracula palette.
    Dracula,
    /// Solarized palette.
    Solarized,
    /// Blue ocean palette.
    Ocean,
}

impl ThemePreset {
    /// Every preset in display order.
    pub const ALL: [Self; 8] = [
        Self::Ansi,
        Self::Geek,
        Self::Mono,
        Self::Latte,
        Self::Nord,
        Self::Dracula,
        Self::Solarized,
        Self::Ocean,
    ];

    /// Human-readable preset name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ansi => "ANSI",
            Self::Geek => "Geek",
            Self::Mono => "Mono",
            Self::Latte => "Latte",
            Self::Nord => "Nord",
            Self::Dracula => "Dracula",
            Self::Solarized => "Solarized",
            Self::Ocean => "Ocean",
        }
    }

    /// Builds this preset's theme for `mode`. Every preset replaces all
    /// nineteen colors, so each arm is a complete literal and adding a color
    /// field is a compile error in every preset; `Ansi` delegates to
    /// `Theme::ansi`.
    pub fn theme(self, mode: ThemeMode) -> Theme {
        if self == Self::Ansi {
            return Theme::ansi(mode);
        }

        let colors = match (self, mode) {
            (Self::Geek, ThemeMode::Light) => ThemeColors {
                background: rgb(0xeaf8ed),
                foreground: rgb(0x073b18),
                card: rgb(0xd5f0da),
                card_foreground: rgb(0x073b18),
                popover: rgb(0xf4fff5),
                popover_foreground: rgb(0x073b18),
                primary: rgb(0x087f23),
                primary_foreground: rgb(0xf2fff4),
                secondary: rgb(0x149447),
                secondary_foreground: rgb(0xf2fff4),
                muted: rgb(0xc5e6cb),
                muted_foreground: rgb(0x286c3b),
                accent: rgb(0x3f8a00),
                accent_foreground: rgb(0xf2fff4),
                destructive: rgb(0xb42318),
                destructive_foreground: rgb(0xfff5f4),
                border: rgb(0x3c9b57),
                input: rgb(0xf4fff5),
                ring: rgb(0x087f23),
            },
            (Self::Geek, ThemeMode::Dark) => ThemeColors {
                background: rgb(0x010b04),
                foreground: rgb(0x9cffaa),
                card: rgb(0x032b12),
                card_foreground: rgb(0x9cffaa),
                popover: rgb(0x021c0b),
                popover_foreground: rgb(0xb7ffc0),
                primary: rgb(0x39ff14),
                primary_foreground: rgb(0x001b07),
                secondary: rgb(0x00c853),
                secondary_foreground: rgb(0x001b07),
                muted: rgb(0x063b1a),
                muted_foreground: rgb(0x61c875),
                accent: rgb(0xb6ff00),
                accent_foreground: rgb(0x001b07),
                destructive: rgb(0xff5370),
                destructive_foreground: rgb(0x220006),
                border: rgb(0x0b7d3e),
                input: rgb(0x02200c),
                ring: rgb(0x39ff14),
            },
            (Self::Mono, ThemeMode::Light) => ThemeColors {
                background: rgb(0xffffff),
                foreground: rgb(0x111111),
                card: rgb(0xf2f2f2),
                card_foreground: rgb(0x111111),
                popover: rgb(0xffffff),
                popover_foreground: rgb(0x111111),
                primary: rgb(0x222222),
                primary_foreground: rgb(0xffffff),
                secondary: rgb(0x4d4d4d),
                secondary_foreground: rgb(0xffffff),
                muted: rgb(0xeaeaea),
                muted_foreground: rgb(0x5c5c5c),
                accent: rgb(0x3d3d3d),
                accent_foreground: rgb(0xffffff),
                destructive: rgb(0x8c1d18),
                destructive_foreground: rgb(0xffffff),
                border: rgb(0x999999),
                input: rgb(0xffffff),
                ring: rgb(0x222222),
            },
            (Self::Mono, ThemeMode::Dark) => ThemeColors {
                background: rgb(0x0f0f0f),
                foreground: rgb(0xf5f5f5),
                card: rgb(0x1b1b1b),
                card_foreground: rgb(0xf5f5f5),
                popover: rgb(0x161616),
                popover_foreground: rgb(0xf5f5f5),
                primary: rgb(0xf5f5f5),
                primary_foreground: rgb(0x0f0f0f),
                secondary: rgb(0xbbbbbb),
                secondary_foreground: rgb(0x0f0f0f),
                muted: rgb(0x262626),
                muted_foreground: rgb(0xa3a3a3),
                accent: rgb(0xdedede),
                accent_foreground: rgb(0x0f0f0f),
                destructive: rgb(0xff8a80),
                destructive_foreground: rgb(0x1a0503),
                border: rgb(0x606060),
                input: rgb(0x161616),
                ring: rgb(0xf5f5f5),
            },
            (Self::Latte, ThemeMode::Light) => ThemeColors {
                background: rgb(0xeff1f5),
                foreground: rgb(0x4c4f69),
                card: rgb(0xccd0da),
                card_foreground: rgb(0x4c4f69),
                popover: rgb(0xe6e9ef),
                popover_foreground: rgb(0x4c4f69),
                primary: rgb(0x1e66f5),
                primary_foreground: rgb(0xffffff),
                secondary: rgb(0x0f766e),
                secondary_foreground: rgb(0xffffff),
                muted: rgb(0xe6e9ef),
                muted_foreground: rgb(0x6c6f85),
                accent: rgb(0x9a5b00),
                accent_foreground: rgb(0xffffff),
                destructive: rgb(0xd20f39),
                destructive_foreground: rgb(0xffffff),
                border: rgb(0x7f849c),
                input: rgb(0xffffff),
                ring: rgb(0x1e66f5),
            },
            (Self::Latte, ThemeMode::Dark) => ThemeColors {
                background: rgb(0x1e1e2e),
                foreground: rgb(0xcdd6f4),
                card: rgb(0x313244),
                card_foreground: rgb(0xcdd6f4),
                popover: rgb(0x181825),
                popover_foreground: rgb(0xcdd6f4),
                primary: rgb(0x89b4fa),
                primary_foreground: rgb(0x11111b),
                secondary: rgb(0x94e2d5),
                secondary_foreground: rgb(0x11111b),
                muted: rgb(0x313244),
                muted_foreground: rgb(0xa6adc8),
                accent: rgb(0xf9e2af),
                accent_foreground: rgb(0x11111b),
                destructive: rgb(0xf38ba8),
                destructive_foreground: rgb(0x11111b),
                border: rgb(0x6c7086),
                input: rgb(0x181825),
                ring: rgb(0x89b4fa),
            },
            (Self::Nord, ThemeMode::Light) => ThemeColors {
                background: rgb(0xeceff4),
                foreground: rgb(0x2e3440),
                card: rgb(0xe5e9f0),
                card_foreground: rgb(0x2e3440),
                popover: rgb(0xf8f9fb),
                popover_foreground: rgb(0x2e3440),
                primary: rgb(0x5e81ac),
                primary_foreground: rgb(0xf8f9fb),
                secondary: rgb(0x4f8f8c),
                secondary_foreground: rgb(0x2e3440),
                muted: rgb(0xd8dee9),
                muted_foreground: rgb(0x4c566a),
                accent: rgb(0xa85f42),
                accent_foreground: rgb(0xf8f9fb),
                destructive: rgb(0xbf616a),
                destructive_foreground: rgb(0xf8f9fb),
                border: rgb(0x8490a3),
                input: rgb(0xf8f9fb),
                ring: rgb(0x5e81ac),
            },
            (Self::Nord, ThemeMode::Dark) => ThemeColors {
                background: rgb(0x2e3440),
                foreground: rgb(0xd8dee9),
                card: rgb(0x3b4252),
                card_foreground: rgb(0xe5e9f0),
                popover: rgb(0x3b4252),
                popover_foreground: rgb(0xe5e9f0),
                primary: rgb(0x88c0d0),
                primary_foreground: rgb(0x2e3440),
                secondary: rgb(0x81a1c1),
                secondary_foreground: rgb(0x2e3440),
                muted: rgb(0x434c5e),
                muted_foreground: rgb(0x9aa6ba),
                accent: rgb(0xebcb8b),
                accent_foreground: rgb(0x2e3440),
                destructive: rgb(0xcf7a82),
                destructive_foreground: rgb(0x2e3440),
                border: rgb(0x78839c),
                input: rgb(0x3b4252),
                ring: rgb(0x88c0d0),
            },
            (Self::Dracula, ThemeMode::Light) => ThemeColors {
                background: rgb(0xf8f8f2),
                foreground: rgb(0x282a36),
                card: rgb(0xe9e9e2),
                card_foreground: rgb(0x282a36),
                popover: rgb(0xffffff),
                popover_foreground: rgb(0x282a36),
                primary: rgb(0x6441a5),
                primary_foreground: rgb(0xffffff),
                secondary: rgb(0x0f766e),
                secondary_foreground: rgb(0xffffff),
                muted: rgb(0xe1e1d8),
                muted_foreground: rgb(0x6272a4),
                accent: rgb(0x9a5b00),
                accent_foreground: rgb(0xffffff),
                destructive: rgb(0xb3122a),
                destructive_foreground: rgb(0xffffff),
                border: rgb(0x9a9a90),
                input: rgb(0xffffff),
                ring: rgb(0x6441a5),
            },
            (Self::Dracula, ThemeMode::Dark) => ThemeColors {
                background: rgb(0x282a36),
                foreground: rgb(0xf8f8f2),
                card: rgb(0x343746),
                card_foreground: rgb(0xf8f8f2),
                popover: rgb(0x343746),
                popover_foreground: rgb(0xf8f8f2),
                primary: rgb(0xbd93f9),
                primary_foreground: rgb(0x282a36),
                secondary: rgb(0x50fa7b),
                secondary_foreground: rgb(0x282a36),
                muted: rgb(0x44475a),
                muted_foreground: rgb(0xb8b8b0),
                accent: rgb(0xffb86c),
                accent_foreground: rgb(0x282a36),
                destructive: rgb(0xff5555),
                destructive_foreground: rgb(0x282a36),
                border: rgb(0x6272a4),
                input: rgb(0x21222c),
                ring: rgb(0xbd93f9),
            },
            (Self::Solarized, ThemeMode::Light) => ThemeColors {
                background: rgb(0xfdf6e3),
                foreground: rgb(0x657b83),
                card: rgb(0xeee8d5),
                card_foreground: rgb(0x586e75),
                popover: rgb(0xfdf6e3),
                popover_foreground: rgb(0x586e75),
                primary: rgb(0x1c6fb0),
                primary_foreground: rgb(0xfdf6e3),
                secondary: rgb(0x1f7a72),
                secondary_foreground: rgb(0xfdf6e3),
                muted: rgb(0xeee8d5),
                muted_foreground: rgb(0x657b83),
                accent: rgb(0x8a6800),
                accent_foreground: rgb(0xfdf6e3),
                destructive: rgb(0xcf3f2c),
                destructive_foreground: rgb(0xfdf6e3),
                border: rgb(0x93a1a1),
                input: rgb(0xffffff),
                ring: rgb(0x1c6fb0),
            },
            (Self::Solarized, ThemeMode::Dark) => ThemeColors {
                background: rgb(0x002b36),
                foreground: rgb(0x93a1a1),
                card: rgb(0x073642),
                card_foreground: rgb(0x93a1a1),
                popover: rgb(0x073642),
                popover_foreground: rgb(0x93a1a1),
                primary: rgb(0x4aa3e0),
                primary_foreground: rgb(0x002b36),
                secondary: rgb(0x2aa198),
                secondary_foreground: rgb(0x002b36),
                muted: rgb(0x073642),
                muted_foreground: rgb(0x839496),
                accent: rgb(0xc99b00),
                accent_foreground: rgb(0x002b36),
                destructive: rgb(0xe06c5a),
                destructive_foreground: rgb(0x002b36),
                border: rgb(0x586e75),
                input: rgb(0x073642),
                ring: rgb(0x4aa3e0),
            },
            (Self::Ocean, ThemeMode::Light) => ThemeColors {
                background: rgb(0xeaf6ff),
                foreground: rgb(0x12324a),
                card: rgb(0xd6ecfa),
                card_foreground: rgb(0x12324a),
                popover: rgb(0xf4fbff),
                popover_foreground: rgb(0x12324a),
                primary: rgb(0x1261a0),
                primary_foreground: rgb(0xf4fbff),
                secondary: rgb(0x0f6f7a),
                secondary_foreground: rgb(0xf4fbff),
                muted: rgb(0xc2e1f2),
                muted_foreground: rgb(0x42677d),
                accent: rgb(0xa8531a),
                accent_foreground: rgb(0xf4fbff),
                destructive: rgb(0xb3261e),
                destructive_foreground: rgb(0xf4fbff),
                border: rgb(0x70a9c9),
                input: rgb(0xf4fbff),
                ring: rgb(0x1261a0),
            },
            (Self::Ocean, ThemeMode::Dark) => ThemeColors {
                background: rgb(0x071a2b),
                foreground: rgb(0xc8e7f5),
                card: rgb(0x0d2b43),
                card_foreground: rgb(0xd9f1fb),
                popover: rgb(0x0d2b43),
                popover_foreground: rgb(0xd9f1fb),
                primary: rgb(0x4db8ff),
                primary_foreground: rgb(0x041321),
                secondary: rgb(0x2dd4bf),
                secondary_foreground: rgb(0x041c1a),
                muted: rgb(0x123b57),
                muted_foreground: rgb(0x82b8d2),
                accent: rgb(0xffb86b),
                accent_foreground: rgb(0x291400),
                destructive: rgb(0xff6b6b),
                destructive_foreground: rgb(0x2b0505),
                border: rgb(0x276184),
                input: rgb(0x0a2237),
                ring: rgb(0x4db8ff),
            },
            (Self::Ansi, _) => unreachable!(),
        };

        Theme::new(mode, colors).customize(|theme| match self {
            Self::Geek => {
                theme.borders.kind = BorderKind::Single;
                theme.typography.heading.attr.underlined /= true;
                theme.typography.code.attr.bold /= true;
            }
            Self::Mono => theme.typography.heading.attr.underlined /= true,
            Self::Dracula => theme.borders.kind = BorderKind::Rounded,
            Self::Nord | Self::Latte | Self::Solarized | Self::Ocean | Self::Ansi => {}
        })
    }
}

fn rgb(value: u32) -> Color {
    Color::Rgb {
        r: (value >> 16) as u8,
        g: (value >> 8) as u8,
        b: value as u8,
    }
}

/// The nineteen base colors a theme is built from; every other token derives
/// from these, so a new field must be supplied by every preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeColors {
    /// Default screen background.
    pub background: Color,
    /// Default text color on `background`.
    pub foreground: Color,
    /// Card surface background.
    pub card: Color,
    /// Text color on `card`.
    pub card_foreground: Color,
    /// Popover surface background.
    pub popover: Color,
    /// Text color on `popover`.
    pub popover_foreground: Color,
    /// Primary accent background, for filled controls.
    pub primary: Color,
    /// Text color on `primary`.
    pub primary_foreground: Color,
    /// Secondary accent background.
    pub secondary: Color,
    /// Text color on `secondary`.
    pub secondary_foreground: Color,
    /// Muted surface background, for subdued regions.
    pub muted: Color,
    /// Text color on `muted`.
    pub muted_foreground: Color,
    /// Accent background, for highlights and code.
    pub accent: Color,
    /// Text color on `accent`.
    pub accent_foreground: Color,
    /// Destructive (danger) background.
    pub destructive: Color,
    /// Text color on `destructive`.
    pub destructive_foreground: Color,
    /// Border color.
    pub border: Color,
    /// Input field background.
    pub input: Color,
    /// Focus ring color.
    pub ring: Color,
}

impl ThemeColors {
    /// The ANSI palette for `mode`.
    pub fn ansi(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
    }

    /// The ANSI palette on a light background.
    pub fn light() -> Self {
        Self {
            background: Color::White,
            foreground: Color::Black,
            card: Color::Grey,
            card_foreground: Color::Black,
            popover: Color::White,
            popover_foreground: Color::Black,
            primary: Color::Blue,
            primary_foreground: Color::White,
            secondary: Color::DarkCyan,
            secondary_foreground: Color::Black,
            muted: Color::Grey,
            muted_foreground: Color::DarkGrey,
            accent: Color::DarkYellow,
            accent_foreground: Color::Black,
            destructive: Color::Red,
            destructive_foreground: Color::White,
            border: Color::DarkGrey,
            input: Color::White,
            ring: Color::Blue,
        }
    }

    /// The ANSI palette on a dark background.
    pub fn dark() -> Self {
        Self {
            background: Color::Black,
            foreground: Color::White,
            card: Color::DarkGrey,
            card_foreground: Color::White,
            popover: Color::Black,
            popover_foreground: Color::White,
            primary: Color::Blue,
            primary_foreground: Color::White,
            secondary: Color::Cyan,
            secondary_foreground: Color::Black,
            muted: Color::DarkGrey,
            muted_foreground: Color::Grey,
            accent: Color::Yellow,
            accent_foreground: Color::Black,
            destructive: Color::Red,
            destructive_foreground: Color::White,
            border: Color::Grey,
            input: Color::DarkGrey,
            ring: Color::Cyan,
        }
    }
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self::light()
    }
}

/// Text styles for each role, all derived from the palette by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeTypography {
    /// Default body text.
    pub body: TextStyle,
    /// Heading text, bold by default.
    pub heading: TextStyle,
    /// Control label text.
    pub label: TextStyle,
    /// Subdued secondary text.
    pub muted: TextStyle,
    /// Inline code text.
    pub code: TextStyle,
}

impl ThemeTypography {
    /// Derives each role's style from `colors`.
    pub fn from_colors(colors: &ThemeColors) -> Self {
        Self {
            body: TextStyle::default().foreground(colors.foreground),
            heading: TextStyle::default().foreground(colors.foreground).bold(),
            label: TextStyle::default().foreground(colors.foreground).bold(),
            muted: TextStyle::default().foreground(colors.muted_foreground),
            code: TextStyle::default().foreground(colors.accent),
        }
    }
}

impl Default for ThemeTypography {
    fn default() -> Self {
        Self::from_colors(&ThemeColors::default())
    }
}

/// Named spacing steps in terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThemeSpacing {
    /// Extra-small step, in cells.
    pub xs: u16,
    /// Small step, in cells.
    pub sm: u16,
    /// Medium step, in cells.
    pub md: u16,
    /// Large step, in cells.
    pub lg: u16,
    /// Extra-large step, in cells.
    pub xl: u16,
}

impl Default for ThemeSpacing {
    fn default() -> Self {
        Self {
            xs: 0,
            sm: 1,
            md: 1,
            lg: 2,
            xl: 3,
        }
    }
}

/// Border appearance: line kind, which edges are drawn, and the border colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThemeBorders {
    /// Line style drawn for borders.
    pub kind: BorderKind,
    /// Which edges of a box draw a border.
    pub edges: Edges<bool>,
    /// Border line color.
    pub foreground: Color,
    /// Color behind the border line.
    pub background: Color,
}

impl ThemeBorders {
    /// Derives border appearance from `colors`; all edges are drawn with
    /// rounded lines.
    pub fn from_colors(colors: &ThemeColors) -> Self {
        Self {
            kind: BorderKind::Rounded,
            edges: Edges::all(true),
            foreground: colors.border,
            background: colors.background,
        }
    }
}

impl Default for ThemeBorders {
    fn default() -> Self {
        Self::from_colors(&ThemeColors::default())
    }
}

/// A complete resolved theme: mode, palette, and every token derived from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Light or dark selection this theme was built for.
    pub mode: ThemeMode,
    /// The nineteen base colors.
    pub colors: ThemeColors,
    /// Text styles derived from `colors`.
    pub typography: ThemeTypography,
    /// Spacing steps.
    pub spacing: ThemeSpacing,
    /// Border appearance derived from `colors`.
    pub borders: ThemeBorders,
    /// Scrollbar colors derived from `colors`.
    pub scrollbar: ScrollbarStyle,
}

/// Derives every token from a palette exactly once. After `from_palette` each
/// token is an independent resolved value, so a later field assignment cannot
/// leave the theme in a half-derived state; `Theme::from_palette` covers the
/// common case.
#[derive(Debug, Clone)]
pub struct ThemeBuilder {
    mode: ThemeMode,
    colors: ThemeColors,
    typography: ThemeTypography,
    spacing: ThemeSpacing,
    borders: ThemeBorders,
    scrollbar: ScrollbarStyle,
}

impl ThemeBuilder {
    /// Starts from `colors` and derives typography, borders and scrollbar from
    /// it; spacing starts at its default.
    pub fn from_palette(mode: ThemeMode, colors: ThemeColors) -> Self {
        Self {
            mode,
            typography: ThemeTypography::from_colors(&colors),
            borders: ThemeBorders::from_colors(&colors),
            scrollbar: scrollbar_style(&colors),
            spacing: ThemeSpacing::default(),
            colors,
        }
    }

    /// Replaces the resolved typography token with a deliberate override, not a
    /// recomputation trigger.
    pub fn typography(mut self, typography: ThemeTypography) -> Self {
        self.typography = typography;
        self
    }

    /// Replaces the resolved border token with a deliberate override.
    pub fn borders(mut self, borders: ThemeBorders) -> Self {
        self.borders = borders;
        self
    }

    /// Replaces the resolved scrollbar token with a deliberate override.
    pub fn scrollbar(mut self, scrollbar: ScrollbarStyle) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Replaces the resolved spacing token with a deliberate override.
    pub fn spacing(mut self, spacing: ThemeSpacing) -> Self {
        self.spacing = spacing;
        self
    }

    /// Assembles the theme from the builder's current parts without re-deriving.
    pub fn build(self) -> Theme {
        Theme::from_parts(
            self.mode,
            self.colors,
            self.typography,
            self.spacing,
            self.borders,
            self.scrollbar,
        )
    }
}

/// Props for `theme_provider`, carrying the theme to install for the subtree.
#[derive(Clone, Default)]
pub struct ThemeProviderProps {
    /// Theme to provide; when omitted the default theme is installed.
    pub value: Attr<Theme>,
}

/// Provides the theme in `props.value`, or the default theme when no value is
/// given, to every component in `props.children`.
pub fn theme_provider(_cx: &mut ComponentContext, props: &Props<ThemeProviderProps>) -> Node {
    let value = props.value.as_ref().cloned().unwrap_or_else(Theme::default);
    theme_context().provider(value, props.children.clone())
}

impl Default for Theme {
    fn default() -> Self {
        Self::ansi(ThemeMode::Light)
    }
}

impl Theme {
    /// Builds the ANSI theme for `mode` with single-line borders.
    pub fn ansi(mode: ThemeMode) -> Self {
        let mut theme = Self::new(mode, ThemeColors::ansi(mode));
        theme.borders.kind = BorderKind::Single;
        theme
    }

    /// The ANSI theme on a light background.
    pub fn light() -> Self {
        Self::ansi(ThemeMode::Light)
    }

    /// The ANSI theme on a dark background.
    pub fn dark() -> Self {
        Self::ansi(ThemeMode::Dark)
    }

    /// Derives all tokens from a complete palette in one shot; prefer this, or
    /// `ThemeBuilder` when an override is needed, over mutating `colors` after
    /// construction, which leaves derived tokens stale.
    pub fn new(mode: ThemeMode, colors: ThemeColors) -> Self {
        ThemeBuilder::from_palette(mode, colors).build()
    }

    /// Alias for `new`: derives all tokens from a complete palette.
    pub fn from_palette(mode: ThemeMode, colors: ThemeColors) -> Self {
        Self::new(mode, colors)
    }

    /// Builds a theme from already-resolved parts without re-deriving them.
    pub fn from_parts(
        mode: ThemeMode,
        colors: ThemeColors,
        typography: ThemeTypography,
        spacing: ThemeSpacing,
        borders: ThemeBorders,
        scrollbar: ScrollbarStyle,
    ) -> Self {
        Self {
            mode,
            colors,
            typography,
            spacing,
            borders,
            scrollbar,
        }
    }

    /// Returns the theme after applying `apply` to it in place.
    pub fn customize(mut self, apply: impl FnOnce(&mut Self)) -> Self {
        apply(&mut self);
        self
    }

    /// Style with the theme's background and foreground text colors.
    pub fn base_style(&self) -> Style {
        style(|style| {
            style.background /= self.colors.background;
            style.text.foreground /= self.colors.foreground;
        })
    }

    /// Style for a card surface: card colors plus the theme's border kind,
    /// edges and colors.
    pub fn card_style(&self) -> Style {
        style(|style| {
            style.background /= self.colors.card;
            style.text.foreground /= self.colors.card_foreground;
            style.border.kind /= self.borders.kind;
            style.border.edges /= self.borders.edges;
            style.border.foreground /= self.borders.foreground;
            style.border.background /= self.borders.background;
        })
    }

    /// Style with the primary background and foreground colors.
    pub fn primary_style(&self) -> Style {
        foreground_style(self.colors.primary, self.colors.primary_foreground)
    }

    /// Style with the muted background and foreground colors.
    pub fn muted_style(&self) -> Style {
        foreground_style(self.colors.muted, self.colors.muted_foreground)
    }

    /// Style with the destructive background and foreground colors.
    pub fn destructive_style(&self) -> Style {
        foreground_style(self.colors.destructive, self.colors.destructive_foreground)
    }

    /// Returns `color` when it reaches `MIN_TEXT_CONTRAST` against the card
    /// background, otherwise the card foreground.
    pub fn on_card(&self, color: Color) -> Color {
        if contrast_ratio(color, self.colors.card) >= MIN_TEXT_CONTRAST {
            color
        } else {
            self.colors.card_foreground
        }
    }
}

/// Minimum contrast ratio, per `Theme::on_card`, below which a color is
/// replaced by the card foreground.
pub const MIN_TEXT_CONTRAST: f64 = 3.0;

/// WCAG contrast ratio between two colors; returns infinity when either color's
/// luminance is unknown (an indexed or reset color).
pub fn contrast_ratio(a: Color, b: Color) -> f64 {
    let (Some(a), Some(b)) = (relative_luminance(a), relative_luminance(b)) else {
        return f64::INFINITY;
    };
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

fn relative_luminance(color: Color) -> Option<f64> {
    let (r, g, b) = match color {
        Color::Rgb { r, g, b } => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::DarkGrey => (128, 128, 128),
        Color::Red => (255, 0, 0),
        Color::DarkRed => (128, 0, 0),
        Color::Green => (0, 255, 0),
        Color::DarkGreen => (0, 128, 0),
        Color::Yellow => (255, 255, 0),
        Color::DarkYellow => (128, 128, 0),
        Color::Blue => (0, 0, 255),
        Color::DarkBlue => (0, 0, 128),
        Color::Magenta => (255, 0, 255),
        Color::DarkMagenta => (128, 0, 128),
        Color::Cyan => (0, 255, 255),
        Color::DarkCyan => (0, 128, 128),
        Color::White => (255, 255, 255),
        Color::Grey => (192, 192, 192),
        _ => return None,
    };
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    Some(0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b))
}

/// Builds the default theme (ANSI light) with `apply` applied.
pub fn theme(apply: impl FnOnce(&mut Theme)) -> Theme {
    Theme::default().customize(apply)
}

fn foreground_style(background: Color, foreground: Color) -> Style {
    style(|style| {
        style.background /= background;
        style.text.foreground /= foreground;
    })
}

fn scrollbar_style(colors: &ThemeColors) -> ScrollbarStyle {
    let mut scrollbar = ScrollbarStyle::default();
    scrollbar.track.foreground /= colors.muted_foreground;
    scrollbar.thumb.foreground /= colors.primary;
    scrollbar
}

static THEME_CONTEXT: OnceLock<ContextKey<Theme>> = OnceLock::new();

/// The process-wide context key holding the active theme, created on first use
/// with the default theme.
pub fn theme_context() -> &'static ContextKey<Theme> {
    THEME_CONTEXT.get_or_init(|| create_context(Theme::default()))
}

impl ComponentContext {
    /// Borrows the single theme in context instead of cloning every token on
    /// each render.
    pub fn use_theme(&self) -> std::sync::Arc<Theme> {
        self.use_context_arc(theme_context)
    }
}
