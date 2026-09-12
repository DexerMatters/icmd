//! Removed API names must fail to compile with ordinary diagnostics.
use icmd::{TextAreaProps, TextEditHandler, text_area};

fn main() {
    let _ = TextAreaProps::default();
    let _ = TextEditHandler::new(|_: u8| {});
    let _node = text_area;
}
