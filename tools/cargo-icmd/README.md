# cargo-icmd

`cargo icmd docs` opens an interactive, terminal-native field guide to the
[`icmd`](https://crates.io/crates/icmd) UI framework: ten chapters that move from
a first component to a shipped application, with live demonstrations, complete
Rust source, API strips, and a `Ctrl+K` search index over every shipped widget
and documented hook.

The browser is itself an `icmd` application and uses only the framework's public
API.

## Prerequisites

`cargo-icmd` enables `icmd`'s `native-raster` feature so the guide can
demonstrate real terminal image protocols. That feature is opt-in and *not* part
of the framework's default build, so building this tool needs native
prerequisites that a plain `icmd` application does not:

- **Chafa development library** — provides `libchafa` and its `pkg-config` file.
  - Debian/Ubuntu: `sudo apt-get install -y libchafa-dev`
  - Fedora: `sudo dnf install chafa-devel`
  - macOS (Homebrew): `brew install chafa`
- **`pkg-config`** — used by `chafa-sys`'s build script to discover the library.
  - Debian/Ubuntu: `sudo apt-get install -y pkg-config`
  - macOS (Homebrew): `brew install pkg-config`
- **libclang** — required by `bindgen` while generating the FFI bindings.
  - Debian/Ubuntu: `sudo apt-get install -y clang`
  - Fedora: `sudo dnf install clang`
  - macOS: `xcode-select --install` (or `brew install llvm`)

The guide also enables `markdown`, so the document chapter renders real
CommonMark instead of a mock.

## Install

From the repository, during development:

```console
cargo install --path tools/cargo-icmd --locked
cargo icmd docs
```

Once published:

```console
cargo install cargo-icmd --locked
cargo icmd docs
```

## Command

```console
cargo icmd docs       # open the interactive field guide
cargo-icmd docs       # the same thing, invoked directly
cargo icmd help       # usage
cargo icmd --version  # the installed version
```

## Navigation and search

Every navigation action is also a focusable button in the header or the index,
so the keyboard is an accelerator rather than the only way in.

| Shortcut | Action |
| --- | --- |
| `Ctrl+K` | Open or close search |
| `Ctrl+B` | Expand or collapse the index |
| `Alt+Left` / `Alt+Right` | Previous / next chapter |
| `Alt+Up` / `Alt+Down` | Previous / next section |
| `Ctrl+C` | Exit, restoring the terminal |

Search matches chapter titles, section titles, component names, hook names, type
names, and curated aliases. Results are ranked exact before prefix before
substring, capped at eight, and preserve chapter order for ties. `Up`/`Down`
move the highlighted result, `Enter` navigates to it, and `Escape` closes the
overlay.

Bare letters, digits, and arrow keys are deliberately unbound, so typing inside
a demonstrated input or textarea never triggers shell navigation.

Selection works inside the document: drag with the pointer, extend with
`Shift` and the arrow keys, select all with `Ctrl+A`, and copy with
`Ctrl+Shift+C`.

## Framework features versus this tool

| Concern | `icmd` | `cargo-icmd` |
| --- | --- | --- |
| Default features | none, pure Rust | enables `native-raster` and `markdown` |
| Markdown | opt-in behind `markdown` | enables `markdown` |
| Terminal image protocol | application's choice | `ImageProtocol::Auto` at runtime |
| Assets | none | one small bundled PNG, decoded once |

The distinction matters when vendoring: an application builds `icmd` with no
native dependencies at all by default and opts in only if it wants terminal
images, while this tool intentionally does not.

## Package contents

The published crate ships its own documentation assets:

- `assets/guide.png` — the guide's original sample raster image, with its source
  and license note in `assets/README.md`.
- `snippets/*.rs` — the runnable examples the guide displays verbatim. They are
  compiled by this crate's tests, so visible code cannot drift from the
  framework's public API.

## License

MIT OR Apache-2.0, matching the framework.