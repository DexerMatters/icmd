//! Compilation gate for every displayed snippet.
//!
//! Declared only in test builds. Each snippet is a real module file rather
//! than an `include!`, so its own documentation and imports stay valid Rust,
//! and a snippet that stops compiling fails this crate's tests.
#![allow(dead_code, unused_imports, clippy::all)]

use crate::snippets::ALL;

#[path = "../snippets/activity_effect.rs"]
mod activity_effect;
#[path = "../snippets/canvas_chart.rs"]
mod canvas_chart;
#[path = "../snippets/counter_state.rs"]
mod counter_state;
#[path = "../snippets/event_ledger.rs"]
mod event_ledger;
#[path = "../snippets/first_component.rs"]
mod first_component;
#[path = "../snippets/geometry_readout.rs"]
mod geometry_readout;
#[path = "../snippets/image_gallery.rs"]
mod image_gallery;
#[path = "../snippets/layout_anatomy.rs"]
mod layout_anatomy;
#[path = "../snippets/lifecycle_timeline.rs"]
mod lifecycle_timeline;
#[path = "../snippets/notification_badge.rs"]
mod notification_badge;
#[path = "../snippets/panel_children.rs"]
mod panel_children;
#[path = "../snippets/preferences_form.rs"]
mod preferences_form;
#[path = "../snippets/publish_operation.rs"]
mod publish_operation;
#[path = "../snippets/release_card.rs"]
mod release_card;
#[path = "../snippets/status_row.rs"]
mod status_row;
#[path = "../snippets/task_list.rs"]
mod task_list;
#[path = "../snippets/test_commit.rs"]
mod test_commit;
#[path = "../snippets/theme_customize.rs"]
mod theme_customize;
#[path = "../snippets/ui_syntax.rs"]
mod ui_syntax;
#[path = "../snippets/unicode_rows.rs"]
mod unicode_rows;
#[path = "../snippets/wrapping_specimens.rs"]
mod wrapping_specimens;

/// Each snippet is non-empty, documented, and carries Rust source.
#[test]
fn every_displayed_snippet_is_real_rust_source() {
    for (name, source) in ALL {
        assert!(!source.trim().is_empty(), "snippet `{name}` is empty");
        assert!(
            source.contains("icmd"),
            "snippet `{name}` must exercise the public icmd API"
        );
        assert!(
            source.contains("fn "),
            "snippet `{name}` must define a function"
        );
    }
}
