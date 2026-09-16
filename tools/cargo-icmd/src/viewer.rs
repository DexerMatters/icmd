//! Entry point for the interactive documentation viewer.

use std::error::Error;

pub(crate) fn run() -> Result<(), Box<dyn Error>> {
    crate::shell::run()?;
    Ok(())
}
