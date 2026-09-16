//! Displayed Rust source, stored as real files and included verbatim.
//!
//! Every substantial example the guide shows lives under `snippets/` as an
//! ordinary Rust file. The guide includes those files as strings, so the
//! displayed source is exactly the source, and the `compile` test module below
//! includes the same files as Rust items, so visible code cannot silently drift
//! from the framework's public API.

/// Static release-status card.
pub(crate) const RELEASE_CARD: &str = include_str!("../snippets/release_card.rs");
/// Smallest complete program.
pub(crate) const FIRST_COMPONENT: &str = include_str!("../snippets/first_component.rs");
/// `ui!` syntax tour.
pub(crate) const UI_SYNTAX: &str = include_str!("../snippets/ui_syntax.rs");
/// Reusable status row with typed props.
pub(crate) const STATUS_ROW: &str = include_str!("../snippets/status_row.rs");
/// Reusable panel with arbitrary children.
pub(crate) const PANEL_CHILDREN: &str = include_str!("../snippets/panel_children.rs");
/// Keyed task list.
pub(crate) const TASK_LIST: &str = include_str!("../snippets/task_list.rs");
/// Controlled counter.
pub(crate) const COUNTER_STATE: &str = include_str!("../snippets/counter_state.rs");
/// Effect-owned worker with cleanup.
pub(crate) const ACTIVITY_EFFECT: &str = include_str!("../snippets/activity_effect.rs");
/// Layout box vocabulary.
pub(crate) const LAYOUT_ANATOMY: &str = include_str!("../snippets/layout_anatomy.rs");
/// Absolutely positioned notification badge.
pub(crate) const NOTIFICATION_BADGE: &str = include_str!("../snippets/notification_badge.rs");
/// Committed geometry readout.
pub(crate) const GEOMETRY_READOUT: &str = include_str!("../snippets/geometry_readout.rs");
/// Unicode cell-width specimens.
pub(crate) const UNICODE_ROWS: &str = include_str!("../snippets/unicode_rows.rs");
/// Wrap, align, and truncation specimens.
pub(crate) const WRAPPING_SPECIMENS: &str = include_str!("../snippets/wrapping_specimens.rs");
/// Controlled preferences form.
pub(crate) const PREFERENCES_FORM: &str = include_str!("../snippets/preferences_form.rs");
/// Bounded event ledger inside a scroll area.
pub(crate) const EVENT_LEDGER: &str = include_str!("../snippets/event_ledger.rs");
/// Publish operation driven by one state model.
pub(crate) const PUBLISH_OPERATION: &str = include_str!("../snippets/publish_operation.rs");
/// Canvas activity chart.
pub(crate) const CANVAS_CHART: &str = include_str!("../snippets/canvas_chart.rs");
/// Bundled image through every fit and mode.
pub(crate) const IMAGE_GALLERY: &str = include_str!("../snippets/image_gallery.rs");
/// Theme customization through the builder.
pub(crate) const THEME_CUSTOMIZE: &str = include_str!("../snippets/theme_customize.rs");
/// Lifecycle hooks in every phase.
pub(crate) const LIFECYCLE_TIMELINE: &str = include_str!("../snippets/lifecycle_timeline.rs");
/// Fixed-viewport commit test.
pub(crate) const TEST_COMMIT: &str = include_str!("../snippets/test_commit.rs");

/// Every displayed snippet, in the order the registry introduces them.
///
/// Only the snippet compilation gate consumes this list, so it is not part of
/// the shipped binary.
#[cfg(test)]
pub(crate) const ALL: &[(&str, &str)] = &[
    ("release_card", RELEASE_CARD),
    ("first_component", FIRST_COMPONENT),
    ("ui_syntax", UI_SYNTAX),
    ("status_row", STATUS_ROW),
    ("panel_children", PANEL_CHILDREN),
    ("task_list", TASK_LIST),
    ("counter_state", COUNTER_STATE),
    ("activity_effect", ACTIVITY_EFFECT),
    ("layout_anatomy", LAYOUT_ANATOMY),
    ("notification_badge", NOTIFICATION_BADGE),
    ("geometry_readout", GEOMETRY_READOUT),
    ("unicode_rows", UNICODE_ROWS),
    ("wrapping_specimens", WRAPPING_SPECIMENS),
    ("preferences_form", PREFERENCES_FORM),
    ("event_ledger", EVENT_LEDGER),
    ("publish_operation", PUBLISH_OPERATION),
    ("canvas_chart", CANVAS_CHART),
    ("image_gallery", IMAGE_GALLERY),
    ("theme_customize", THEME_CUSTOMIZE),
    ("lifecycle_timeline", LIFECYCLE_TIMELINE),
    ("test_commit", TEST_COMMIT),
];
