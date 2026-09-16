//! Cargo subcommand and interactive documentation browser for `icmd`.
//!
//! The `ui!` trees in the guide are deeply nested, so this crate raises the
//! macro expansion limit above the default.
#![recursion_limit = "512"]

use std::{env, error::Error, process::ExitCode};

mod assets;
mod chapters;
mod demos;
mod docs;
mod metadata;
#[cfg(test)]
mod screen;
mod search;
mod shell;
mod snippets;
#[cfg(test)]
mod snippets_compile;
mod viewer;

const HELP: &str = "Interactive documentation for icmd

Usage:
    cargo icmd docs
    cargo-icmd docs

Commands:
    docs        Open the interactive documentation browser
    help        Print this help

Options:
    -h, --help       Print this help
    -V, --version    Print the installed version";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Docs,
    Help,
    Version,
}

fn parse_command(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let first = args.next();
    let command = match first.as_deref() {
        Some("icmd") => args.next(),
        other => other.map(str::to_owned),
    };

    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument `{extra}`"));
    }

    match command.as_deref() {
        Some("docs") => Ok(Command::Docs),
        None | Some("help" | "-h" | "--help") => Ok(Command::Help),
        Some("-V" | "--version") => Ok(Command::Version),
        Some(value) => Err(format!("unknown command `{value}`")),
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match parse_command(env::args().skip(1))? {
        Command::Docs => viewer::run()?,
        Command::Help => println!("{HELP}"),
        Command::Version => println!("cargo-icmd {}", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cargo-icmd: {error}\n\n{HELP}");
            ExitCode::from(2)
        }
    }
}
