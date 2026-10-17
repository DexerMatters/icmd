//! Public width and elision helpers.

use icmd::{display_width, fit, fit_right};

#[test]
fn display_width_counts_cells_not_bytes() {
    assert_eq!(display_width("abc"), 3);
    assert_eq!(display_width("界"), 2);
    assert_eq!(display_width("👩‍💻"), 2);
    assert_eq!(display_width("e\u{301}"), 1);
    assert_eq!(display_width("a\nb\tc"), 3);
}

#[test]
fn fit_keeps_the_head_and_ellipsis_within_budget() {
    assert_eq!(fit("hello", 10), "hello");
    assert_eq!(fit("hello", 5), "hello");
    assert_eq!(fit("hello", 4), "hel…");
    assert_eq!(fit("hello", 1), "…");
    assert_eq!(fit("hello", 0), "");
    assert_eq!(fit("界界界", 5), "界界…");
    // A wide glyph that would cross the budget is dropped, never split.
    assert_eq!(fit("a界", 3), "a界");
    assert_eq!(fit("a界", 2), "a…");
}

#[test]
fn fit_right_keeps_the_tail() {
    assert_eq!(fit_right("hello", 10), "hello");
    assert_eq!(fit_right("hello", 4), "…llo");
    assert_eq!(fit_right("hello", 1), "…");
    assert_eq!(fit_right("hello", 0), "");
    assert_eq!(fit_right("a界b", 3), "…b");
    assert_eq!(fit_right("a界b", 4), "a界b");
    assert_eq!(fit_right("xx界b", 4), "…界b");
}

#[test]
fn an_already_fitting_value_is_borrowed() {
    matches!(fit("hello", 10), std::borrow::Cow::Borrowed(_));
    matches!(fit_right("hello", 10), std::borrow::Cow::Borrowed(_));
}
