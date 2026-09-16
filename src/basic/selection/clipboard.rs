//! The framework's own clipboard, and the mirror into the terminal's clipboard.
//!
//! A terminal application cannot read the operating system clipboard, so the
//! copy and cut chords fill this process-wide buffer and the paste chord reads it
//! back; a copy is also mirrored into the terminal's own clipboard through the
//! writer a running session installs (an OSC 52 write), while applications that
//! own a real clipboard still receive every copy and cut through
//! `TextClipboardEvent`. The buffer is bounded so a large copy cannot retain
//! memory for the process lifetime.

use std::sync::{Arc, Mutex};

/// Retained bytes are capped; a longer copy is truncated on a character boundary
/// rather than refused, so a copy always leaves something pasteable.
const MAX_BYTES: usize = 1 << 20;

/// The terminal payload is capped far lower than the local buffer: the OSC 52
/// sequence travels through the terminal and any multiplexer in between, and
/// several of those drop an oversized sequence or truncate it mid-encoding. A
/// larger copy is still retained locally; only the mirror is shortened.
const MAX_SYSTEM_BYTES: usize = 48 * 1024;

/// Writes a copy into the terminal's own clipboard. Installed by the session
/// that owns the terminal; absent when there is no terminal to write to.
type SystemWriter = Arc<dyn Fn(&str) + Send + Sync>;

static CLIPBOARD: Mutex<Option<String>> = Mutex::new(None);
static SYSTEM_WRITER: Mutex<Option<SystemWriter>> = Mutex::new(None);

/// Truncate to a byte ceiling on a character boundary.
fn retain(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Fill the clipboard. Empty text clears it, so copying nothing cannot paste a
/// stale value; a non-empty copy is mirrored into the terminal's clipboard.
pub fn store(text: &str) {
    let retained = if text.is_empty() {
        None
    } else {
        Some(retain(text, MAX_BYTES))
    };
    let mirror = retained
        .as_deref()
        .map(|text| retain(text, MAX_SYSTEM_BYTES));
    *CLIPBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = retained;
    if let Some(text) = mirror
        && let Some(writer) = SYSTEM_WRITER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    {
        writer(&text);
    }
}

/// The clipboard contents, if anything has been copied or cut.
pub fn load() -> Option<String> {
    CLIPBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Install or clear the writer that mirrors a copy into the terminal's own
/// clipboard; with none installed a copy only fills this process's buffer.
///
/// Public only so the runtime and the crate's tests can install one; not a
/// stable interface.
#[doc(hidden)]
pub fn set_system_writer(writer: Option<SystemWriter>) {
    *SYSTEM_WRITER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = writer;
}

/// Test and diagnostics helper: forget the retained text.
#[doc(hidden)]
pub fn clear() {
    let mut slot = CLIPBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = None;
}
