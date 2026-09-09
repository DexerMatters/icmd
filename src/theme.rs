use std::sync::OnceLock;

use crossterm::style::Color;

use crate::{
    Attr, BorderKind, ComponentContext, ContextKey, Edges, Node, Props, Style, TextStyle,
    create_context, style,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

/// Built-in palettes used by the demo and available to applications as
/// convenient starting points for customization.
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

        let mut colors = ThemeColors::ansi(mode);
        match (self, mode) {
            // A deliberately neon terminal palette: deep green surfaces,
            // phosphor text, and electric lime accents.
            (Self::Geek, ThemeMode::Light) => {
                colors.background = rgb(0xeaf8ed);
                colors.foreground = rgb(0x073b18);
                colors.card = rgb(0xd5f0da);
                colors.card_foreground = rgb(0x073b18);
                colors.popover = rgb(0xf4fff5);
                colors.popover_foreground = rgb(0x073b18);
                colors.primary = rgb(0x087f23);
                colors.primary_foreground = rgb(0xf2fff4);
                colors.secondary = rgb(0x149447);
                colors.secondary_foreground = rgb(0xf2fff4);
                colors.muted = rgb(0xc5e6cb);
                colors.muted_foreground = rgb(0x286c3b);
                colors.accent = rgb(0x4f9f00);
                colors.accent_foreground = rgb(0xf2fff4);
                colors.destructive = rgb(0xb42318);
                colors.destructive_foreground = rgb(0xfff5f4);
                colors.border = rgb(0x3c9b57);
                colors.input = rgb(0xf4fff5);
                colors.ring = rgb(0x087f23);
            }
            (Self::Geek, ThemeMode::Dark) => {
                colors.background = rgb(0x010b04);
                colors.foreground = rgb(0x9cffaa);
                colors.card = rgb(0x032b12);
                colors.card_foreground = rgb(0x9cffaa);
                colors.popover = rgb(0x021c0b);
                colors.popover_foreground = rgb(0xb7ffc0);
                colors.primary = rgb(0x39ff14);
                colors.primary_foreground = rgb(0x001b07);
                colors.secondary = rgb(0x00c853);
                colors.secondary_foreground = rgb(0x001b07);
                colors.muted = rgb(0x063b1a);
                colors.muted_foreground = rgb(0x61c875);
                colors.accent = rgb(0xb6ff00);
                colors.accent_foreground = rgb(0x001b07);
                colors.destructive = rgb(0xff5370);
                colors.destructive_foreground = rgb(0x220006);
                colors.border = rgb(0x0b7d3e);
                colors.input = rgb(0x02200c);
                colors.ring = rgb(0x39ff14);
            }
            (Self::Mono, ThemeMode::Light) => {
                colors.foreground = rgb(0x111111);
                colors.card = rgb(0xf2f2f2);
                colors.primary = rgb(0x222222);
                colors.secondary = rgb(0xd9d9d9);
                colors.muted = rgb(0xeaeaea);
                colors.muted_foreground = rgb(0x666666);
                colors.border = rgb(0x999999);
            }
            (Self::Mono, ThemeMode::Dark) => {
                colors.background = rgb(0x0f0f0f);
                colors.foreground = rgb(0xf5f5f5);
                colors.card = rgb(0x1b1b1b);
                colors.primary = rgb(0xf5f5f5);
                colors.secondary = rgb(0x333333);
                colors.muted = rgb(0x262626);
                colors.muted_foreground = rgb(0xa3a3a3);
                colors.border = rgb(0x606060);
            }
            (Self::Latte, ThemeMode::Light) => {
                colors.background = rgb(0xeff1f5);
                colors.foreground = rgb(0x4c4f69);
                colors.card = rgb(0xccd0da);
                colors.primary = rgb(0x1e66f5);
                colors.secondary = rgb(0x179299);
                colors.muted = rgb(0xe6e9ef);
                colors.muted_foreground = rgb(0x6c6f85);
                colors.border = rgb(0x9ca0b0);
            }
            (Self::Latte, ThemeMode::Dark) => {
                colors.background = rgb(0x1e1e2e);
                colors.foreground = rgb(0xcdd6f4);
                colors.card = rgb(0x313244);
                colors.primary = rgb(0x89b4fa);
                colors.secondary = rgb(0x94e2d5);
                colors.muted = rgb(0x313244);
                colors.muted_foreground = rgb(0xa6adc8);
                colors.border = rgb(0x585b70);
            }
            (Self::Nord, ThemeMode::Light) => {
                colors.background = rgb(0xeceff4);
                colors.foreground = rgb(0x2e3440);
                colors.card = rgb(0xe5e9f0);
                colors.card_foreground = rgb(0x2e3440);
                colors.primary = rgb(0x5e81ac);
                colors.primary_foreground = rgb(0xf8f9fb);
                colors.secondary = rgb(0x8fbcbb);
                colors.secondary_foreground = rgb(0x2e3440);
                colors.muted = rgb(0xd8dee9);
                colors.muted_foreground = rgb(0x4c566a);
                colors.accent = rgb(0xd08770);
                colors.accent_foreground = rgb(0x2e3440);
                colors.border = rgb(0xb8c0ce);
                colors.input = rgb(0xf8f9fb);
                colors.ring = rgb(0x5e81ac);
            }
            (Self::Nord, ThemeMode::Dark) => {
                colors.background = rgb(0x2e3440);
                colors.foreground = rgb(0xd8dee9);
                colors.card = rgb(0x3b4252);
                colors.card_foreground = rgb(0xe5e9f0);
                colors.primary = rgb(0x88c0d0);
                colors.primary_foreground = rgb(0x2e3440);
                colors.secondary = rgb(0x81a1c1);
                colors.secondary_foreground = rgb(0x2e3440);
                colors.muted = rgb(0x434c5e);
                colors.muted_foreground = rgb(0x9aa6ba);
                colors.accent = rgb(0xebcb8b);
                colors.accent_foreground = rgb(0x2e3440);
                colors.border = rgb(0x616b7f);
                colors.input = rgb(0x3b4252);
                colors.ring = rgb(0x88c0d0);
            }
            (Self::Dracula, ThemeMode::Light) => {
                colors.background = rgb(0xf8f8f2);
                colors.foreground = rgb(0x282a36);
                colors.card = rgb(0xe9e9e2);
                colors.card_foreground = rgb(0x282a36);
                colors.primary = rgb(0x6441a5);
                colors.primary_foreground = rgb(0xffffff);
                colors.secondary = rgb(0x168b72);
                colors.secondary_foreground = rgb(0xffffff);
                colors.muted = rgb(0xe1e1d8);
                colors.muted_foreground = rgb(0x6272a4);
                colors.accent = rgb(0xb05a00);
                colors.accent_foreground = rgb(0xffffff);
                colors.border = rgb(0xb7b7ae);
                colors.input = rgb(0xffffff);
                colors.ring = rgb(0x6441a5);
            }
            (Self::Dracula, ThemeMode::Dark) => {
                colors.background = rgb(0x282a36);
                colors.foreground = rgb(0xf8f8f2);
                colors.card = rgb(0x343746);
                colors.card_foreground = rgb(0xf8f8f2);
                colors.primary = rgb(0xbd93f9);
                colors.primary_foreground = rgb(0x282a36);
                colors.secondary = rgb(0x50fa7b);
                colors.secondary_foreground = rgb(0x282a36);
                colors.muted = rgb(0x44475a);
                colors.muted_foreground = rgb(0xb8b8b0);
                colors.accent = rgb(0xffb86c);
                colors.accent_foreground = rgb(0x282a36);
                colors.border = rgb(0x6272a4);
                colors.input = rgb(0x21222c);
                colors.ring = rgb(0xbd93f9);
            }
            (Self::Solarized, ThemeMode::Light) => {
                colors.background = rgb(0xfdf6e3);
                colors.foreground = rgb(0x657b83);
                colors.card = rgb(0xeee8d5);
                colors.card_foreground = rgb(0x586e75);
                colors.primary = rgb(0x268bd2);
                colors.primary_foreground = rgb(0xfdf6e3);
                colors.secondary = rgb(0x2aa198);
                colors.secondary_foreground = rgb(0xfdf6e3);
                colors.muted = rgb(0xeee8d5);
                colors.muted_foreground = rgb(0x839496);
                colors.accent = rgb(0xb58900);
                colors.accent_foreground = rgb(0x073642);
                colors.border = rgb(0x93a1a1);
                colors.input = rgb(0xffffff);
                colors.ring = rgb(0x268bd2);
            }
            (Self::Solarized, ThemeMode::Dark) => {
                colors.background = rgb(0x002b36);
                colors.foreground = rgb(0x839496);
                colors.card = rgb(0x073642);
                colors.card_foreground = rgb(0x93a1a1);
                colors.primary = rgb(0x268bd2);
                colors.primary_foreground = rgb(0xfdf6e3);
                colors.secondary = rgb(0x2aa198);
                colors.secondary_foreground = rgb(0x002b36);
                colors.muted = rgb(0x073642);
                colors.muted_foreground = rgb(0x657b83);
                colors.accent = rgb(0xb58900);
                colors.accent_foreground = rgb(0x002b36);
                colors.border = rgb(0x586e75);
                colors.input = rgb(0x073642);
                colors.ring = rgb(0x268bd2);
            }
            (Self::Ocean, ThemeMode::Light) => {
                colors.background = rgb(0xeaf6ff);
                colors.foreground = rgb(0x12324a);
                colors.card = rgb(0xd6ecfa);
                colors.card_foreground = rgb(0x12324a);
                colors.primary = rgb(0x1261a0);
                colors.primary_foreground = rgb(0xf4fbff);
                colors.secondary = rgb(0x168aad);
                colors.secondary_foreground = rgb(0xf4fbff);
                colors.muted = rgb(0xc2e1f2);
                colors.muted_foreground = rgb(0x42677d);
                colors.accent = rgb(0xf28f3b);
                colors.accent_foreground = rgb(0x301600);
                colors.border = rgb(0x70a9c9);
                colors.input = rgb(0xf4fbff);
                colors.ring = rgb(0x1261a0);
            }
            (Self::Ocean, ThemeMode::Dark) => {
                colors.background = rgb(0x071a2b);
                colors.foreground = rgb(0xc8e7f5);
                colors.card = rgb(0x0d2b43);
                colors.card_foreground = rgb(0xd9f1fb);
                colors.primary = rgb(0x4db8ff);
                colors.primary_foreground = rgb(0x041321);
                colors.secondary = rgb(0x2dd4bf);
                colors.secondary_foreground = rgb(0x041c1a);
                colors.muted = rgb(0x123b57);
                colors.muted_foreground = rgb(0x82b8d2);
                colors.accent = rgb(0xffb86b);
                colors.accent_foreground = rgb(0x291400);
                colors.border = rgb(0x276184);
                colors.input = rgb(0x0a2237);
                colors.ring = rgb(0x4db8ff);
            }
            (Self::Ansi, _) => unreachable!(),
        }

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
            secondary: Color::Cyan,
            secondary_foreground: Color::Black,
            muted: Color::Grey,
            muted_foreground: Color::DarkGrey,
            accent: Color::Yellow,
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
            secondary: Color::DarkCyan,
            secondary_foreground: Color::White,
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
}

/// Props for the built-in theme provider. The global theme context key is
/// supplied automatically, so callers only provide the theme value.
#[derive(Clone, Default)]
pub struct ThemeProviderProps {
    pub value: Attr<Theme>,
}

/// Provide a theme through the global [`theme_context`] without creating a
/// host element.
pub fn theme_provider(_cx: &mut ComponentContext, props: &Props<ThemeProviderProps>) -> Node {
    let value = props.value.as_ref().cloned().expect("missing theme value");
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
        Self::from_parts(mode, colors, typography, ThemeSpacing::default(), borders)
    }

    pub fn from_parts(
        mode: ThemeMode,
        colors: ThemeColors,
        typography: ThemeTypography,
        spacing: ThemeSpacing,
        borders: ThemeBorders,
    ) -> Self {
        Self {
            mode,
            colors,
            typography,
            spacing,
            borders,
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

static THEME_CONTEXT: OnceLock<ContextKey<Theme>> = OnceLock::new();

pub fn theme_context() -> &'static ContextKey<Theme> {
    THEME_CONTEXT.get_or_init(|| create_context(Theme::default()))
}

impl ComponentContext {
    pub fn use_theme(&self) -> Theme {
        self.use_context(theme_context)
    }
}
