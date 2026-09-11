//! `raw_input`, `input`, and `textarea`.
//!
//! This module owns text-entry behavior. `raw_input` is the single primitive;
//! `input` and `textarea` are thin policy/theme wrappers over it.

pub(crate) mod model;
pub(crate) mod view;

pub use view::{RawInputAppearance, RawInputMode, RawInputProps, raw_input};

/// A text value carried by change and submit events.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextValueEvent {
    pub value: String,
}

/// The clipboard gesture a [`TextClipboardEvent`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextClipboardAction {
    #[default]
    Copy,
    Cut,
}

/// A clipboard request produced by the editor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextClipboardEvent {
    pub action: TextClipboardAction,
    pub text: String,
}
