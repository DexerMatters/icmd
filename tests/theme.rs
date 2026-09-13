//! Theme palettes and the `Text` default style they drive.
//!
//! Colours are compared as escape sequences, so these tests force crossterm's
//! `NO_COLOR` handling off. They live in their own integration test binary so
//! that forcing it cannot affect any other test's frames.

use std::time::Duration;

use crossterm::style::Color;
use icmd::{
    Attr, BadgeProps, BadgeVariant, Commit, Component, Lower, Node, Renderer, Runtime, Size, Span,
    Text, badge,
    theme::{Theme, ThemeColors, ThemeMode, ThemePreset},
    theme_provider, ui, view,
};

/// crossterm disables every colour escape when `NO_COLOR` is set, memoizing the
/// answer for the process. Rendering a colour assertion needs it re-enabled.
fn force_color() {
    crossterm::style::Colored::set_ansi_color_disabled(false);
}

fn render(node: Node, viewport: Size) -> String {
    let (commit, _) = Commit::new(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start();
    input.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .expect("runtime did not produce a frame")
        .expect("renderer failed")
}

fn render_in(preset: ThemePreset, mode: ThemeMode, node: Node) -> String {
    let theme = preset.theme(mode);
    render(
        ui! { <theme_provider value={theme}>{node}</theme_provider> },
        Size::new(60, 8),
    )
}

fn badge_row() -> Node {
    ui! {
        <view>
            <badge text="primary" variant={BadgeVariant::Primary} />
            <badge text="secondary" variant={BadgeVariant::Secondary} />
            <badge text="accent" variant={BadgeVariant::Accent} />
            <badge text="muted" variant={BadgeVariant::Muted} />
            <badge text="danger" variant={BadgeVariant::Destructive} />
        </view>
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb { r, g, b }
}

/// A `Text`'s own style is the default for its spans. Setting it and setting the
/// equivalent span style must produce the same pixels - otherwise a component
/// that colours itself with `Text::foreground` (every badge, alert title, and
/// skeleton) silently renders in the inherited colour.
#[test]
fn text_level_style_paints_like_a_span_style() {
    force_color();
    let text_level = render(
        ui! {
            <view>
                {Text::new("X")
                    .foreground(Color::Red)
                    .background(Color::Blue)
                    .bold()}
            </view>
        },
        Size::new(6, 2),
    );
    let span_level = render(
        ui! {
            <view>
                {Text::from_spans([Span::new("X")
                    .foreground(Color::Red)
                    .background(Color::Blue)
                    .bold()])}
            </view>
        },
        Size::new(6, 2),
    );
    assert!(
        text_level.contains("38;5;9"),
        "the Text foreground must paint: {text_level:?}"
    );
    assert!(
        text_level.contains("48;5;12"),
        "the Text background must paint: {text_level:?}"
    );
    assert_eq!(
        text_level, span_level,
        "a Text default and an equivalent span style must agree"
    );
}

/// The bug this guards: with `Text`'s style dropped, every badge rendered with
/// the inherited colour and all eight presets produced identical frames.
#[test]
fn badges_differ_between_themes() {
    force_color();
    let mut frames = Vec::new();
    for preset in ThemePreset::ALL {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let frame = render_in(preset, mode, badge_row());
            // The ANSI preset uses terminal names (`38;5;n`); the rest use
            // truecolour (`38;2;`). Either way the badge must paint a themed
            // foreground and background rather than inherit them.
            assert!(
                frame.contains("38;") && frame.contains("48;"),
                "{} {mode:?} badges must paint themed colours: {frame:?}",
                preset.name()
            );
            frames.push((preset.name(), mode, frame));
        }
    }
    for left in 0..frames.len() {
        for right in left + 1..frames.len() {
            let (a_name, a_mode, a) = &frames[left];
            let (b_name, b_mode, b) = &frames[right];
            // Same theme in different modes may legitimately coincide only if
            // the palettes do; distinct presets never should.
            if a_name != b_name {
                assert_ne!(
                    a, b,
                    "{a_name} {a_mode:?} and {b_name} {b_mode:?} must not render identical badges"
                );
            }
        }
    }
}

/// A preset that only overrides some of its colours silently inherits the ANSI
/// defaults for the rest, which is what made dark palettes carry white-on-white
/// badge pairs.
#[test]
fn every_preset_defines_every_colour() {
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let ansi = ThemeColors::ansi(mode);
        for preset in ThemePreset::ALL {
            if preset == ThemePreset::Ansi {
                continue;
            }
            let c = preset.theme(mode).colors;
            let inherited: Vec<&str> = [
                ("background", c.background, ansi.background),
                ("foreground", c.foreground, ansi.foreground),
                ("card", c.card, ansi.card),
                ("card_foreground", c.card_foreground, ansi.card_foreground),
                ("popover", c.popover, ansi.popover),
                (
                    "popover_foreground",
                    c.popover_foreground,
                    ansi.popover_foreground,
                ),
                ("primary", c.primary, ansi.primary),
                (
                    "primary_foreground",
                    c.primary_foreground,
                    ansi.primary_foreground,
                ),
                ("secondary", c.secondary, ansi.secondary),
                (
                    "secondary_foreground",
                    c.secondary_foreground,
                    ansi.secondary_foreground,
                ),
                ("muted", c.muted, ansi.muted),
                (
                    "muted_foreground",
                    c.muted_foreground,
                    ansi.muted_foreground,
                ),
                ("accent", c.accent, ansi.accent),
                (
                    "accent_foreground",
                    c.accent_foreground,
                    ansi.accent_foreground,
                ),
                ("destructive", c.destructive, ansi.destructive),
                (
                    "destructive_foreground",
                    c.destructive_foreground,
                    ansi.destructive_foreground,
                ),
                ("border", c.border, ansi.border),
                ("input", c.input, ansi.input),
                ("ring", c.ring, ansi.ring),
            ]
            .into_iter()
            .filter(|(_, value, default)| value == default)
            .map(|(name, _, _)| name)
            .collect();
            assert!(
                inherited.is_empty(),
                "{} {mode:?} inherits ANSI defaults for {inherited:?}",
                preset.name()
            );
        }
    }
}

/// Relative luminance of a colour, with the ANSI names mapped to their sRGB
/// values so an inherited default cannot slip past the check.
fn luminance(color: Color) -> Option<f64> {
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

fn contrast(a: Color, b: Color) -> f64 {
    let (Some(a), Some(b)) = (luminance(a), luminance(b)) else {
        return f64::INFINITY;
    };
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// Every filled pairing a component actually paints must stay readable. The
/// threshold is deliberately below WCAG's 4.5 because terminal cells are small
/// glyphs on a dark or light field; 3.0 is the "large text" floor.
#[test]
fn filled_pairs_stay_readable() {
    const FLOOR: f64 = 3.0;
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for preset in ThemePreset::ALL {
            let c = preset.theme(mode).colors;
            let pairs = [
                ("primary", c.primary, c.primary_foreground),
                ("secondary", c.secondary, c.secondary_foreground),
                ("accent", c.accent, c.accent_foreground),
                ("destructive", c.destructive, c.destructive_foreground),
                ("card", c.card, c.card_foreground),
                ("popover", c.popover, c.popover_foreground),
                ("background", c.background, c.foreground),
            ];
            for (name, background, foreground) in pairs {
                let ratio = contrast(background, foreground);
                assert!(
                    ratio >= FLOOR,
                    "{} {mode:?}: {name} contrast {ratio:.2} is below {FLOOR} \
                     ({background:?} on {foreground:?})",
                    preset.name()
                );
            }
        }
    }
}

/// The theme's own helper styles must be consistent with the palette, so a
/// component that uses `Theme::primary_style` matches one that reads
/// `theme.colors.primary` directly.
#[test]
fn theme_helper_styles_match_the_palette() {
    for preset in ThemePreset::ALL {
        let theme = preset.theme(ThemeMode::Dark);
        assert_eq!(
            theme.primary_style().background,
            Attr::Set(theme.colors.primary)
        );
        assert_eq!(
            theme.primary_style().text.foreground,
            Attr::Set(theme.colors.primary_foreground)
        );
        assert_eq!(
            theme.destructive_style().background,
            Attr::Set(theme.colors.destructive)
        );
        assert_eq!(
            theme.destructive_style().text.foreground,
            Attr::Set(theme.colors.destructive_foreground)
        );
        assert_eq!(
            theme.muted_style().background,
            Attr::Set(theme.colors.muted)
        );
        // The border background must match the surface the border is drawn on.
        assert_eq!(theme.borders.background, theme.colors.background);
        assert_eq!(theme.borders.foreground, theme.colors.border);
    }
    let _ = BadgeProps::default();
    let _ = rgb(0, 0, 0);
}

/// The solid roles are painted as text too - an alert title, a `kbd`, a stat
/// number - not only as badge and button fills. They must stay readable on both
/// the page and a card, which is what caught the white-on-white `Mono` dark
/// primary and the near-invisible `Nord` light secondary.
#[test]
fn solid_roles_stay_readable_as_text() {
    const FLOOR: f64 = 3.0;
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for preset in ThemePreset::ALL {
            let c = preset.theme(mode).colors;
            for (name, color) in [
                ("primary", c.primary),
                ("secondary", c.secondary),
                ("accent", c.accent),
                ("destructive", c.destructive),
                ("muted_foreground", c.muted_foreground),
                ("foreground", c.foreground),
            ] {
                // The 16-colour ANSI fallback is exempt from the card floor
                // and gets a lower page floor: its only "raised" surface is
                // ANSI 8, a mid grey that collides with red and blue, and it
                // has no truecolour to escape with. The curated presets do, so
                // they meet the full floor on every surface.
                let ansi = preset == ThemePreset::Ansi;
                let floor = if ansi { 2.0 } else { FLOOR };
                let surfaces: &[(&str, Color)] = if ansi {
                    &[("background", c.background)]
                } else {
                    &[("background", c.background), ("card", c.card)]
                };
                for (surface_name, surface) in surfaces {
                    let ratio = contrast(color, *surface);
                    assert!(
                        ratio >= floor,
                        "{} {mode:?}: {name} on {surface_name} is {ratio:.2}, below {floor}",
                        preset.name()
                    );
                }
            }
        }
    }
}

/// A border is decoration, so it may be quieter than text - but a card's frame
/// still has to be visible against the card it encloses.
#[test]
fn borders_stay_visible_on_their_cards() {
    const FLOOR: f64 = 2.0;
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for preset in ThemePreset::ALL {
            let c = preset.theme(mode).colors;
            let ratio = contrast(c.border, c.card);
            assert!(
                ratio >= FLOOR,
                "{} {mode:?}: border on card is {ratio:.2}, below {FLOOR}",
                preset.name()
            );
        }
    }
}

/// A custom theme can put a role and its surface at the same colour. The
/// `on_card` guard is what keeps a coloured label readable in that case.
#[test]
fn on_card_falls_back_when_a_role_is_invisible() {
    let theme = icmd::theme::Theme::default().customize(|theme| {
        theme.colors.card = Color::Black;
        theme.colors.card_foreground = Color::White;
        theme.colors.secondary = Color::Black;
        theme.colors.accent = Color::Yellow;
    });
    assert_eq!(
        theme.on_card(theme.colors.secondary),
        Color::White,
        "an invisible role must fall back to the card foreground"
    );
    assert_eq!(
        theme.on_card(theme.colors.accent),
        Color::Yellow,
        "a readable role is kept unchanged"
    );
}

// DUP-04: presets are complete data. Every preset must resolve every palette
// token to a concrete value for both modes, with no ANSI leftovers.
#[test]
fn every_preset_defines_a_complete_palette_for_both_modes() {
    use icmd::theme::{ThemeColors, ThemeMode, ThemePreset};

    fn angle(left: crossterm::style::Color, right: crossterm::style::Color) -> bool {
        format!("{left:?}") == format!("{right:?}")
    }

    for preset in ThemePreset::ALL {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let theme = preset.theme(mode);
            let colors = theme.colors;
            let ansi = ThemeColors::ansi(mode);

            // The preset may legitimately equal the ANSI palette for `Ansi`,
            // but no other preset may silently inherit a token it forgot.
            if preset != ThemePreset::Ansi {
                let inherited = [
                    ("background", colors.background, ansi.background),
                    ("foreground", colors.foreground, ansi.foreground),
                    ("card", colors.card, ansi.card),
                    (
                        "card_foreground",
                        colors.card_foreground,
                        ansi.card_foreground,
                    ),
                    ("popover", colors.popover, ansi.popover),
                    (
                        "popover_foreground",
                        colors.popover_foreground,
                        ansi.popover_foreground,
                    ),
                    ("primary", colors.primary, ansi.primary),
                    (
                        "primary_foreground",
                        colors.primary_foreground,
                        ansi.primary_foreground,
                    ),
                    ("secondary", colors.secondary, ansi.secondary),
                    (
                        "secondary_foreground",
                        colors.secondary_foreground,
                        ansi.secondary_foreground,
                    ),
                    ("muted", colors.muted, ansi.muted),
                    (
                        "muted_foreground",
                        colors.muted_foreground,
                        ansi.muted_foreground,
                    ),
                    ("accent", colors.accent, ansi.accent),
                    (
                        "accent_foreground",
                        colors.accent_foreground,
                        ansi.accent_foreground,
                    ),
                    ("destructive", colors.destructive, ansi.destructive),
                    (
                        "destructive_foreground",
                        colors.destructive_foreground,
                        ansi.destructive_foreground,
                    ),
                    ("border", colors.border, ansi.border),
                    ("input", colors.input, ansi.input),
                    ("ring", colors.ring, ansi.ring),
                ];
                let leftovers: Vec<_> = inherited
                    .into_iter()
                    .filter(|(_, value, ansi_value)| angle(*value, *ansi_value))
                    .map(|(name, _, _)| name)
                    .collect();
                assert!(
                    leftovers.is_empty(),
                    "{preset:?}/{mode:?} silently inherited ANSI tokens: {leftovers:?}"
                );
            }

            // Derived tokens are coherent with the palette for every preset.
            assert_eq!(
                theme.typography.body.foreground,
                icmd::Attr::Set(colors.foreground),
                "{preset:?}/{mode:?} body text must use the palette foreground"
            );
        }
    }
}

// PERF-05: the theme is shared through the context. Many themed leaves must
// observe one shared theme allocation instead of a clone per consumer.
#[test]
fn themed_leaves_share_one_theme_instance() {
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct ProbeProps {
        seen: Arc<Mutex<Vec<usize>>>,
    }

    fn probe(cx: &mut icmd::ComponentContext, props: &icmd::Props<ProbeProps>) -> Node {
        let theme = cx.use_theme();
        props
            .user_defined
            .seen
            .lock()
            .unwrap()
            .push(Arc::as_ptr(&theme) as usize);
        icmd::text("x")
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    let children: Vec<Node> = (0..200)
        .map(|_| probe.props(ProbeProps { seen: seen.clone() }).node())
        .collect();
    let node = icmd::theme_provider
        .props(icmd::ThemeProviderProps {
            value: Attr::Set(Theme::light()),
        })
        .children(children);

    let _ = render(node, Size::new(20, 4));

    let observed = seen.lock().unwrap().clone();
    assert_eq!(observed.len(), 200, "every probe must run");
    let first = observed[0];
    assert!(
        observed.iter().all(|value| *value == first),
        "themed consumers must share one theme instance"
    );
}
