// API-01 gate: the high-level facade must be usable without importing anything
// from `advanced`, and every documented tier must resolve.
#![allow(unused_imports)]

use icmd::Attr;
use icmd::events::{
    DispatchOutcome, EventHandlers, EventListener, FocusEvent, KeyboardEvent, PasteEvent,
    PointerButton, PointerEvent, ScrollEvent, TerminalFocusEvent, WheelEvent,
};
use icmd::image::{
    ImageMode, ImageProtocol, ImageSource, RasterImage, RasterImageError, RasterPlacement,
};
use icmd::style::{
    Align, Attributes, BorderKind, BorderStyle, Dimension, Fill, FillError, Justify, Layout,
    Overflow, Style, TextAlign, TextStyle, TextWrap,
};
use icmd::theme::{Theme, ThemeColors, ThemeMode, ThemePreset};
use icmd::widgets::{column, input, raster_image, scroll_area, text, view};
use icmd::{Component, ComponentContext, Node, Props, RuntimeConfig, run};

// A high-level application fixture: it uses only the curated tiers, plus the
// `ui!` macro, and never names an `advanced` item.
fn high_level_app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    icmd::ui! {
        <column>
            "hello"
            <input />
        </column>
    }
}

#[test]
fn high_level_facade_resolves_and_renders() {
    let node = high_level_app.apply(());
    let _ = node;
}

#[test]
fn style_and_event_tiers_are_constructible() {
    let style = Style::default();
    let dim = Dimension::Cells(3);
    let _fill = Fill::new("█").expect("a block glyph is a valid fill");
    assert_eq!(style.overflow_x, Attr::Unset);
    let _ = dim;
    let _ = TextWrap::Soft;
    let _ = TextAlign::Start;
    let _ = Attributes::default();
    let _ = BorderStyle::default();
    let _ = Justify::Start;
    let _ = Align::Start;
    let _ = Layout::Vertical;
    let _ = BorderKind::Single;
}

// API-06: explicit attribute operations replace operator-based mutation, and an
// explicit `false` can override a default `true`.
#[test]
fn attr_operations_distinguish_unset_default_and_explicit() {
    let mut attr: Attr<bool> = Attr::Unset;
    assert!(!attr.is_explicit());
    assert!(!attr.is_set());

    attr.set_default(true);
    assert_eq!(attr, Attr::Set(true));
    assert!(attr.is_explicit());

    // `set_default` never overwrites an existing value.
    attr.set_default(false);
    assert_eq!(attr, Attr::Set(true));

    // `set` does overwrite, including to an explicit false.
    attr.set(false);
    assert_eq!(attr, Attr::Set(false));

    attr.clear();
    assert_eq!(attr, Attr::Unset);
    assert!(!attr.is_explicit());
}

#[test]
fn focus_override_can_clear_a_default_true() {
    // The old `focusable: bool` merged with boolean OR, so a caller could never
    // turn a default-focusable node off.
    let focusable_default = || icmd::DomProps::default().with_focusable(true);
    let override_off = icmd::DomProps::default().with_focusable(false);
    let merged = focusable_default().with_overrides(&override_off);
    assert_eq!(
        merged.focusable,
        Attr::Set(false),
        "an explicit false must override a default true"
    );

    // An unset override leaves the default in place.
    let merged = focusable_default().with_overrides(&icmd::DomProps::default());
    assert_eq!(merged.focusable, Attr::Set(true));

    // An explicit true overrides a default true and a default false.
    let off_default = icmd::DomProps::default().with_focusable(false);
    let on_override = icmd::DomProps::default().with_focusable(true);
    assert_eq!(
        off_default.with_overrides(&on_override).focusable,
        Attr::Set(true)
    );
}
