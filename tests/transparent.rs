//! The transparent style: a color slot set to `transparent()` paints the
//! terminal's own default, and `background_blend` decides whether that default
//! covers the lower layers or leaves them visible.
//!
//! Colors are compared as escape sequences, so these tests force crossterm's
//! `NO_COLOR` handling off. They live in their own integration test binary so
//! that forcing it cannot affect any other test's frames.

use std::time::Duration;

use crossterm::style::Color;
use icmd::advanced::{Commit, Lower, Renderer, Runtime};
use icmd::{BackgroundBlend, Dimension, ElementRef, Node, Size, Style, transparent, ui, view};

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

/// A blue box holding a transparent child with one visible glyph, so the child
/// region always paints a cell and the composite is observable.
fn layered(blend: BackgroundBlend) -> Node {
    ui! {
        <view style={|style: &mut Style| {
            style.background /= Color::Blue;
            style.width /= Dimension::Cells(4);
            style.height /= Dimension::Cells(2);
        }}>
            "A"
            <view style={|style: &mut Style| {
                style.background /= transparent();
                style.background_blend /= blend;
                style.width /= Dimension::Cells(2);
                style.height /= Dimension::Cells(1);
            }}>"B"</view>
        </view>
    }
}

#[test]
fn transparent_is_the_terminal_default_color() {
    assert_eq!(transparent(), Color::Reset);
}

#[test]
fn transparent_background_overrides_the_inherited_backdrop() {
    let parent_ref = ElementRef::new();
    let child_ref = ElementRef::new();
    let child_handle = child_ref.clone();
    let node = ui! {
        <view
            element_ref={parent_ref.clone()}
            style={|style: &mut Style| {
                style.background /= Color::Blue;
                style.width /= Dimension::Cells(4);
                style.height /= Dimension::Cells(2);
            }}
        >
            <view
                element_ref={child_handle}
                style={|style: &mut Style| {
                    style.background /= transparent();
                    style.background_blend /= BackgroundBlend::Blend;
                    style.width /= Dimension::Cells(2);
                    style.height /= Dimension::Cells(1);
                }}
            />
        </view>
    };
    let (commit, _) = Commit::new(Size::new(8, 4));
    let (input, output) = Runtime::new(Lower::default()).then(commit).start();
    input.send(node).unwrap();
    let _ = output
        .recv_timeout(Duration::from_secs(2))
        .expect("commit frame");

    let parent = parent_ref.current().expect("parent committed");
    assert_eq!(parent.style().background, Some(Color::Blue));
    assert_eq!(
        parent.style().background_blend,
        BackgroundBlend::Cover,
        "an unset blend slot resolves to the covering default"
    );

    let child = child_ref.current().expect("child committed");
    assert_eq!(
        child.style().background,
        Some(Color::Reset),
        "transparent() must be an explicit value that stops backdrop inheritance"
    );
    assert_eq!(child.style().background_blend, BackgroundBlend::Blend);
}

#[test]
fn cover_background_culls_the_lower_layer() {
    force_color();
    let frame = render(layered(BackgroundBlend::Cover), Size::new(6, 3));
    assert!(
        frame.contains("\u{1b}[48;5;12m"),
        "the parent background must paint: {frame:?}"
    );
    assert!(
        frame.contains("\u{1b}[49m"),
        "Cover must paint the terminal default over the lower layer: {frame:?}"
    );
}

#[test]
fn blend_background_shows_the_lower_layer() {
    force_color();
    let frame = render(layered(BackgroundBlend::Blend), Size::new(6, 3));
    assert!(
        frame.contains("\u{1b}[48;5;12m"),
        "the parent background must paint: {frame:?}"
    );
    assert!(
        !frame.contains("\u{1b}[49m"),
        "Blend must contribute nothing, leaving the lower layer's style: {frame:?}"
    );
}

#[test]
fn transparent_foreground_emits_the_terminal_default_foreground() {
    force_color();
    let frame = render(
        ui! {
            <view style={|style: &mut Style| style.text.foreground /= Color::Red}>
                "A"
                <view style={|style: &mut Style| style.text.foreground /= transparent()}>"B"</view>
            </view>
        },
        Size::new(4, 3),
    );
    assert!(
        frame.contains("\u{1b}[38;5;9m"),
        "the inherited red text must paint: {frame:?}"
    );
    assert!(
        frame.contains("\u{1b}[39m"),
        "the transparent foreground must reset to the terminal default: {frame:?}"
    );
}
