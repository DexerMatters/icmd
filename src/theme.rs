use std::sync::OnceLock;

use crossterm::style::Color;

use crate::{
    Attr, BorderKind, ComponentContext, ContextKey, Edges, Node, Props, ScrollbarStyle, Style,
    TextStyle, create_context, style,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemePreset {
    Ansi,
    Geek,
    Mono,
    Latte,
    Nord,
    Dracula,
    Solarized,
    Ocean,
}

impl ThemePreset {
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

    pub fn theme(self, mode: ThemeMode) -> Theme {
        if self == Self::Ansi {
            return Theme::ansi(mode);
        }

        // Every preset replaces *all* nineteen colours. Starting from the ANSI
        // palette and overriding a subset would leave the rest as terminal
        // names such as `Color::White`, which look correct only in a light
        // scheme and turn into unreadable leftovers in a dark one (a white
        // badge foreground on a dark accent, for example).
        // Every arm is a complete literal. A new colour field is therefore a
        // compile error in every preset instead of silently inheriting an ANSI
        // default, which the previous piecemeal mutation script allowed.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeColors {
    pub background: Color,
    pub foreground: Color,
    pub card: Color,
    pub card_foreground: Color,
    pub popover: Color,
    pub popover_foreground: Color,
    pub primary: Color,
    pub primary_foreground: Color,
    pub secondary: Color,
    pub secondary_foreground: Color,
    pub muted: Color,
    pub muted_foreground: Color,
    pub accent: Color,
    pub accent_foreground: Color,
    pub destructive: Color,
    pub destructive_foreground: Color,
    pub border: Color,
    pub input: Color,
    pub ring: Color,
}

impl ThemeColors {
    pub fn ansi(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeTypography {
    pub body: TextStyle,
    pub heading: TextStyle,
    pub label: TextStyle,
    pub muted: TextStyle,
    pub code: TextStyle,
}

impl ThemeTypography {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThemeSpacing {
    pub xs: u16,
    pub sm: u16,
    pub md: u16,
    pub lg: u16,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThemeBorders {
    pub kind: BorderKind,
    pub edges: Edges<bool>,
    pub foreground: Color,
    pub background: Color,
}

impl ThemeBorders {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub mode: ThemeMode,
    pub colors: ThemeColors,
    pub typography: ThemeTypography,
    pub spacing: ThemeSpacing,
    pub borders: ThemeBorders,
    pub scrollbar: ScrollbarStyle,
}

#[derive(Clone, Default)]
pub struct ThemeProviderProps {
    pub value: Attr<Theme>,
}

pub fn theme_provider(_cx: &mut ComponentContext, props: &Props<ThemeProviderProps>) -> Node {
    // Omitting the value has coherent semantics: provide the default theme.
    // The old `expect` turned a syntactically valid macro omission into a
    // worker panic.
    let value = props.value.as_ref().cloned().unwrap_or_else(Theme::default);
    theme_context().provider(value, props.children.clone())
}

impl Default for Theme {
    fn default() -> Self {
        Self::ansi(ThemeMode::Light)
    }
}

impl Theme {
    pub fn ansi(mode: ThemeMode) -> Self {
        let mut theme = Self::new(mode, ThemeColors::ansi(mode));
        theme.borders.kind = BorderKind::Single;
        theme
    }

    pub fn light() -> Self {
        Self::ansi(ThemeMode::Light)
    }

    pub fn dark() -> Self {
        Self::ansi(ThemeMode::Dark)
    }

    pub fn new(mode: ThemeMode, colors: ThemeColors) -> Self {
        let typography = ThemeTypography::from_colors(&colors);
        let borders = ThemeBorders::from_colors(&colors);
        let scrollbar = scrollbar_style(&colors);
        Self::from_parts(
            mode,
            colors,
            typography,
            ThemeSpacing::default(),
            borders,
            scrollbar,
        )
    }

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

    pub fn customize(mut self, apply: impl FnOnce(&mut Self)) -> Self {
        apply(&mut self);
        self
    }

    pub fn base_style(&self) -> Style {
        style(|style| {
            style.background /= self.colors.background;
            style.text.foreground /= self.colors.foreground;
        })
    }

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

    pub fn primary_style(&self) -> Style {
        foreground_style(self.colors.primary, self.colors.primary_foreground)
    }

    pub fn muted_style(&self) -> Style {
        foreground_style(self.colors.muted, self.colors.muted_foreground)
    }

    pub fn destructive_style(&self) -> Style {
        foreground_style(self.colors.destructive, self.colors.destructive_foreground)
    }

    pub fn on_card(&self, color: Color) -> Color {
        if contrast_ratio(color, self.colors.card) >= MIN_TEXT_CONTRAST {
            color
        } else {
            self.colors.card_foreground
        }
    }
}

pub const MIN_TEXT_CONTRAST: f64 = 3.0;

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

pub fn theme_context() -> &'static ContextKey<Theme> {
    THEME_CONTEXT.get_or_init(|| create_context(Theme::default()))
}

impl ComponentContext {
    pub fn use_theme(&self) -> Theme {
        self.use_context(theme_context)
    }
}
