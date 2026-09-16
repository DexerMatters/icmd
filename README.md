# icmd

**A retained, terminal-native UI framework for Rust.** Describe the interface as
components, and the runtime resolves it into terminal cells — repainting only
what actually changed.

- **Compose, don't paint.** Components are ordinary functions returning nodes, so
  an interface is data you can read, test, and split apart.
- **The cell is the unit.** Grapheme clusters, wide CJK glyphs, and emoji are
  measured and merged correctly, with soft and hard wrapping built in.
- **Only what changed is drawn.** Every commit diffs against the last frame, so a
  quiet interface stays quiet on the wire.
- **Broken frames never land.** Batches are validated before the renderer mutates
  anything, and trees, images, caches, and output stay inside declared budgets.
- **Ask the terminal where things are.** An element ref exposes the committed
  rectangle, resolved style, clipping, and scroll state of any host.
- **Text behaves like text.** Selection, clipboard, single-line inputs, and
  textareas share one engine, hit tested against the committed frame.
- **Lifecycle you can see.** Ordered `boot`, `mount`, `ready`, `unmount`, and
  `exit` phases, component effects, and a session handle for graceful exit.
- **Native images, optional.** The default build is pure Rust; one opt-in
  feature adds Chafa-backed Kitty, Sixel, and iTerm2 output, and symbol rendering
  is always available.

## If you know React

The mental model will feel familiar; the target is a cell grid instead of a DOM.

| React | icmd |
| --- | --- |
| Function components returning JSX | Components returning nodes through `ui!` |
| Props with defaults | Props with defaults, overridden per call |
| `useState`, `useRef`, `useMemo` | `cx.use_state`, `cx.use_ref`, `cx.use_memo` |
| `useEffect` with dependencies | `cx.use_effect(deps, ..)`, `use_mount_effect`, `use_unmount` |
| Context provider and `useContext` | `cx.provide(key, value)` and `cx.use_context(key)`, where the key carries a typed default |
| Controlled and uncontrolled inputs | `value` + `on_change` versus `default_value` |
| A ref to a DOM node | `cx.use_element_ref()` for committed cell geometry |
| Reconciliation against the DOM | A retained tree laid out in cells; only changed cells reach the terminal |

What differs is everything below the component layer: there is no virtual DOM
standing in for a browser document. The runtime keeps a retained tree, resolves
it into terminal cells, and diffs committed frames, so "rendering" means writing
the cells that actually changed. Cells, clipping, wide glyphs, image protocols,
focus, key routing, and text selection are first-class, because a terminal has
all of those and no CSS.

## Installation

```console
cargo add icmd
```

The default build is pure Rust: it emits text cells and needs no native
toolchain. Two features are opt-in.

```console
cargo add icmd --features native-raster   # Chafa-backed terminal images
cargo add icmd --features markdown        # CommonMark as selectable components
```

`native-raster` links a native library, so that build also needs `pkg-config`,
the Chafa development package, and libclang:

- Debian/Ubuntu: `sudo apt-get install -y pkg-config libchafa-dev clang`
- Fedora: `sudo dnf install -y pkgconf-pkg-config chafa-devel clang`
- macOS (Homebrew): `brew install pkg-config chafa` and `xcode-select --install`

`icmd` requires Rust 1.88 or newer.

## Quick start

```rust
use icmd::{Component, RuntimeConfig, paragraph, render, ui};

fn app(_cx: &mut icmd::ComponentContext, _props: &icmd::Props<()>) -> icmd::Node {
    ui! { <paragraph>"Hello, terminal"</paragraph> }
}

fn main() -> Result<(), icmd::RenderError> {
    render(app.apply(()), RuntimeConfig::default())
}
```

Element measurements are available after the first commit through a stable ref:

```rust,no_run
use icmd::{ComponentContext, Dimension, ElementSnapshot, Node, Props, Style, ui, view};

fn measured_panel(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let panel_ref = cx.use_element_ref();
    ui! {
        <view
            element_ref={panel_ref.clone()}
            on_element_change={move |snapshot: Option<ElementSnapshot>| {
                let _latest = panel_ref.current();
                if let Some(snapshot) = snapshot {
                    let _width = snapshot.bounding_rect().width;
                }
            }}
            style={|style: &mut Style| style.width /= Dimension::Cells(20)}
        >
            "Measured content"
        </view>
    }
}
```

## Documentation: the interactive field guide

`cargo icmd docs` opens a ten-chapter field guide to the framework, built with
`icmd` itself. It teaches the mental model first, then walks components and state,
layout, text, controls, feedback, scrolling, media, themes, and production — with
live demonstrations you can type into, the complete Rust source behind each one,
API strips, and a search index over every shipped widget and documented hook.

```console
cargo install cargo-icmd --locked   # from crates.io
cargo icmd docs
```

Installing from this repository instead:

```console
cargo install --path tools/cargo-icmd --locked
cargo icmd docs
```

`Ctrl+K` searches every chapter, section, widget, hook, and type; `Ctrl+B`
expands or collapses the index; `Alt+Arrow` moves between chapters and sections;
`Ctrl+C` exits and restores the terminal. Every action is also a focusable
button, so the keyboard is an accelerator rather than the only way in.

Because the guide demonstrates real terminal image protocols, `cargo-icmd`
enables `icmd`'s opt-in `native-raster` and `markdown` features, so building it
needs the native prerequisites listed above even though the framework's own
default build does not. See
[`tools/cargo-icmd/README.md`](tools/cargo-icmd/README.md) for the command,
navigation, and packaging details.

## Features

- Default: pure Rust. Cell output only, with no native or parser dependency, so
  a constrained build environment needs nothing but the crate.
- `native-raster` (opt-in): Chafa-backed Kitty/Sixel/iTerm2 payloads and symbol
  rasterization. Requires `pkg-config`, the Chafa development package, and
  libclang. Without it, an unsupported native protocol is reported as a typed
  error and images fall back to text cells.
- `markdown` (opt-in): parses CommonMark with tables, task lists, strikethrough,
  footnotes, and GFM alerts into themed, selectable components.

Enable the opt-in `markdown` feature, then supply Markdown through the
component's `text` prop:

```rust
use icmd::{ComponentContext, Node, Props, markdown, ui};

fn app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    ui! { <markdown text="# Hello\n\nThis is **selectable** Markdown." /> }
}
```

## License

Licensed under either of MIT or Apache-2.0, at your option.
