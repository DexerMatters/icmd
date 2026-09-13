use std::{sync::Arc, time::Duration};

use crossterm::style::Color;
use icmd::{
    AlertProps, Attr, BadgeProps, CheckboxProps, Commit, Component, ComponentContext, Dimension,
    DomProps, Fill, Layout, Lower, Node, Overflow, Percent, Props, RadioProps, Renderer, Runtime,
    ScrollAreaProps, ScrollAxes, ScrollbarGlyph, ScrollbarStyle, ScrollbarVisibility, Size,
    SkeletonProps, SpinnerProps, Style, SwitchProps, canvas, column, empty, fragment, progress_bar,
    scroll_area, style_patch, text,
    theme::{Theme, ThemeMode, ThemePreset, theme},
    theme_provider, ui, view,
};

fn root_empty(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    ui! { <view dom={props.dom.clone()}>{""}</view> }
}

fn children(_cx: &mut ComponentContext, props: &Props<()>) -> Node {
    ui! { <view dom={props.dom.clone()}>{props.children.clone().into_iter().collect::<Node>()}</view> }
}

#[derive(Default)]
struct CounterProps {
    start: Attr<u64>,
}

fn counter(_cx: &mut ComponentContext, props: &Props<CounterProps>) -> Node {
    text((props.start | 0).to_string())
}

#[test]
fn style_patches_and_host_overrides_are_composable() {
    let patch = style_patch(|style| {
        style.width /= Dimension::Cells(3);
        style.text.attr.bold /= true;
    });
    let mut defaults = Style::default();
    defaults /= patch.clone();

    let caller = icmd::style(|style| style.width /= Dimension::Cells(5));
    let merged = defaults.with_overrides(&caller);
    assert_eq!(merged.width, Attr::Set(Dimension::Cells(5)));
    assert_eq!(merged.text.attr.bold, Attr::Set(true));

    let props = Props::new(());
    assert_eq!(props.host_props(DomProps::default()), DomProps::default());
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

#[test]
fn style_fill_tiles_single_and_wide_unicode_graphemes() {
    let single = root_empty
        .style(|style| {
            style.width /= Dimension::Cells(4);
            style.height /= Dimension::Cells(1);
            style.fill /= "·";
        })
        .apply(());
    let single_frame = render(single, Size::new(4, 1));
    assert_eq!(single_frame.matches('·').count(), 4, "{single_frame:?}");

    let wide = root_empty
        .style(|style| {
            style.width /= Dimension::Cells(5);
            style.height /= Dimension::Cells(1);
            style.fill /= "界";
        })
        .apply(());
    let wide_frame = render(wide, Size::new(5, 1));
    assert_eq!(wide_frame.matches('界').count(), 2, "{wide_frame:?}");

    assert!(Fill::new("👩‍💻").is_ok());
    assert!(Fill::new("ab").is_err());
}

#[test]
fn percentages_can_resolve_against_available_space_or_the_viewport() {
    let available = root_empty
        .style(|style| {
            style.width /= Dimension::Percent(Percent::available(50));
            style.height /= Dimension::Percent(Percent::available(50));
            style.fill /= "·";
        })
        .apply(());
    let available = children
        .style(|style| {
            style.layout /= Layout::Absolute;
            style.width /= Dimension::Cells(10);
            style.height /= Dimension::Cells(4);
        })
        .children([available]);
    let frame = render(available, Size::new(20, 10));
    assert_eq!(frame.matches('·').count(), 10, "{frame:?}");

    let viewport = root_empty
        .style(|style| {
            style.width /= Dimension::Percent(Percent::viewport(50));
            style.height /= Dimension::Percent(Percent::viewport(50));
            style.fill /= "•";
        })
        .apply(());
    let viewport = children
        .style(|style| {
            style.layout /= Layout::Absolute;
            style.width /= Dimension::Cells(10);
            style.height /= Dimension::Cells(4);
            style.overflow /= Overflow::Visible;
        })
        .children([viewport]);
    let frame = render(viewport, Size::new(20, 10));
    assert_eq!(frame.matches('•').count(), 50, "{frame:?}");
}

#[test]
fn progress_bar_uses_the_nearest_theme() {
    let theme = Theme::ansi(ThemeMode::Dark).customize(|theme| {
        theme.colors.primary = Color::Rgb {
            r: 0x58,
            g: 0xa6,
            b: 0xff,
        };
    });
    assert_eq!(
        theme.colors.primary,
        Color::Rgb {
            r: 0x58,
            g: 0xa6,
            b: 0xff
        }
    );
    let node = ui! {
        <theme_provider value={theme}>
            {progress_bar
                .extra(|props| {
                    props.value /= 1;
                    props.max /= 2;
                    props.width /= 4;
                    props.show_percentage /= false;
                })
                .node()}
        </theme_provider>
    };
    let frame = render(node, Size::new(8, 1));
    assert_eq!(frame.matches('█').count(), 2, "{frame:?}");
    assert_eq!(frame.matches('░').count(), 2, "{frame:?}");
}

#[test]
fn theme_starts_with_ansi_defaults_and_accepts_mutable_customization() {
    assert_eq!(Theme::default(), Theme::ansi(ThemeMode::Light));
    assert_eq!(Theme::default().borders.kind, icmd::BorderKind::Single);

    let custom = theme(|theme| {
        theme.colors.primary = Color::Magenta;
        theme.spacing.md = 2;
    });
    assert_eq!(custom.colors.primary, Color::Magenta);
    assert_eq!(custom.spacing.md, 2);
}

#[test]
fn built_in_theme_presets_are_named_and_geek_is_matrix_green() {
    assert_eq!(ThemePreset::ALL.len(), 8);
    assert_eq!(ThemePreset::Geek.name(), "Geek");
    assert_eq!(ThemePreset::Nord.name(), "Nord");

    let geek = ThemePreset::Geek.theme(ThemeMode::Dark);
    assert_eq!(
        geek.colors.primary,
        Color::Rgb {
            r: 0x39,
            g: 0xff,
            b: 0x14,
        }
    );
    assert_eq!(
        geek.colors.background,
        Color::Rgb {
            r: 0x01,
            g: 0x0b,
            b: 0x04,
        }
    );
}

#[test]
fn canvas_draws_unicode_text_and_shapes() {
    let node = canvas
        .extra(|props| {
            props.width /= 8;
            props.height /= 3;
            props
                .draw
                .set(Arc::new(|drawing: &mut icmd::CanvasContext| {
                    drawing.stroke_rect(0, 0, 8, 3).unwrap();
                    drawing.fill_text("界", 3, 1).unwrap();
                }));
        })
        .node();
    let frame = render(node, Size::new(8, 3));
    assert!(frame.contains('┌'), "{frame:?}");
    assert_eq!(frame.matches('─').count(), 12, "{frame:?}");
    assert!(frame.contains("界"));
    assert!(frame.contains('┘'));
}

#[test]
fn extra_modifiers_compose_and_full_props_stay_available() {
    let chained = counter
        .extra(|props| props.start /= 1)
        .extra(|props| props.start /= 2)
        .style(|style| style.width /= Dimension::Cells(4))
        .node();
    let frame = render(chained, Size::new(4, 1));
    assert!(frame.contains('2'));

    let full = counter.apply(Props::new(CounterProps {
        start: Attr::Set(9),
    }));
    let frame = render(full, Size::new(4, 1));
    assert!(frame.contains('9'));
}

#[test]
fn generic_node_helpers_compose_with_builtin_components() {
    let node = column.children([text("one"), fragment([text("two"), empty()])]);
    let frame = render(node, Size::new(8, 3));
    assert!(frame.contains('o'));
    assert!(frame.contains('t'));
}

#[test]
fn widget_props_are_unset_until_extra_modifies_them() {
    let progress: icmd::ProgressBarProps = Default::default();
    assert!(!progress.value.is_set());
    assert!(!progress.max.is_set());
    assert!(!progress.width.is_set());
    assert!(!progress.show_percentage.is_set());
    assert!(!progress.label.is_set());

    let scroll: ScrollAreaProps = Default::default();
    assert!(!scroll.axes.is_set());
    assert!(!scroll.scrollbar_visibility.is_set());
    assert!(!scroll.offset.is_set());
    assert!(!scroll.enable_mouse.is_set());
    assert!(!scroll.enable_wheel.is_set());
    assert!(!scroll.enable_keyboard.is_set());

    let spinner: SpinnerProps = Default::default();
    assert!(!spinner.frame.is_set());
    assert!(!spinner.label.is_set());
    let badge: BadgeProps = Default::default();
    assert!(!badge.text.is_set());
    assert!(!badge.variant.is_set());
    let alert: AlertProps = Default::default();
    assert!(!alert.title.is_set());
    assert!(!alert.message.is_set());
    assert!(!alert.variant.is_set());
    assert!(!SkeletonProps::default().width.is_set());
    assert!(!CheckboxProps::default().checked.is_set());
    assert!(!RadioProps::default().selected.is_set());
    assert!(!SwitchProps::default().on.is_set());

    let canvas = icmd::CanvasProps::default();
    assert!(!canvas.width.is_set());
    assert!(!canvas.height.is_set());
    assert!(!canvas.draw.is_set());
}

#[test]
fn scroll_area_is_the_integrated_scroll_component() {
    let node = scroll_area
        .props(ScrollAreaProps {
            axes: Attr::Set(ScrollAxes::Vertical),
            scrollbar_visibility: Attr::Set(ScrollbarVisibility::Hidden),
            ..ScrollAreaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(4);
            style.height /= Dimension::Cells(2);
        })
        .children([text("one"), text("two"), text("three")]);
    let frame = render(node, Size::new(4, 2));
    assert!(frame.contains('o'));
    assert!(!frame.contains('│'));
}

#[test]
fn scrollbar_glyphs_validate_early_and_theme_styles_are_overridable() {
    assert!(ScrollbarGlyph::new("界").is_err());
    assert!(ScrollbarGlyph::new("ab").is_err());

    let scrollbar = ScrollbarStyle {
        vertical_track: ScrollbarGlyph::new("!").unwrap(),
        vertical_thumb: ScrollbarGlyph::new("#").unwrap(),
        ..ScrollbarStyle::default()
    };
    let themed = Theme::default().customize(|theme| theme.scrollbar = scrollbar.clone());
    assert_eq!(themed.scrollbar.vertical_track.symbol(), "!");

    let node = scroll_area
        .props(ScrollAreaProps {
            axes: Attr::Set(ScrollAxes::Vertical),
            scrollbar_visibility: Attr::Set(ScrollbarVisibility::Always),
            scrollbar_style: Attr::Set(scrollbar),
            ..ScrollAreaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(4);
            style.height /= Dimension::Cells(2);
        })
        .children([text("one"), text("two"), text("three")]);
    let frame = render(node, Size::new(4, 2));
    assert!(frame.contains('!') || frame.contains('#'));
}

// PERF-12: batched primitives must produce exactly the same cells as the
// per-cell path they replaced.
#[test]
fn batched_canvas_primitives_match_cell_by_cell_drawing() {
    // Draw with the batched primitive.
    let batched = canvas
        .extra(|props| {
            props.width /= 6;
            props.height /= 3;
            props
                .draw
                .set(Arc::new(|drawing: &mut icmd::CanvasContext| {
                    drawing.fill_rect(0, 0, 6, 3, "░").unwrap();
                    drawing.line(0, 0, 5, 2, "*").unwrap();
                }));
        })
        .node();
    let batched_frame = render(batched, Size::new(6, 3));

    // The same picture drawn one cell at a time.
    let scalar = canvas
        .extra(|props| {
            props.width /= 6;
            props.height /= 3;
            props
                .draw
                .set(Arc::new(|drawing: &mut icmd::CanvasContext| {
                    for row in 0..3 {
                        for column in 0..6 {
                            drawing.set(column, row, "░").unwrap();
                        }
                    }
                    // Bresenham from (0,0) to (5,2).
                    let (mut x, mut y) = (0i32, 0i32);
                    let (x1, y1) = (5i32, 2i32);
                    let dx = (x1 - x).abs();
                    let sx = if x < x1 { 1 } else { -1 };
                    let dy = -(y1 - y).abs();
                    let sy = if y < y1 { 1 } else { -1 };
                    let mut error = dx + dy;
                    loop {
                        drawing.set(x, y, "*").unwrap();
                        if x == x1 && y == y1 {
                            break;
                        }
                        let twice = 2 * error;
                        if twice >= dy {
                            error += dy;
                            x += sx;
                        }
                        if twice <= dx {
                            error += dx;
                            y += sy;
                        }
                    }
                }));
        })
        .node();
    let scalar_frame = render(scalar, Size::new(6, 3));

    assert_eq!(
        batched_frame, scalar_frame,
        "batching must not change the painted cells"
    );
    assert!(batched_frame.contains('*'), "the line must be painted");
}
