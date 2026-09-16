# icmd

A retained, terminal-native UI framework for Rust.

Highlights:

- Declarative composition through a small `ui!` macro and ordinary components.
- Unicode-aware text layout: grapheme clusters, wide cells, emoji merging, and wrapping.
- Transactional frame validation: invalid batches are rejected before any renderer mutation.
- Enforceable resource budgets for trees, images, caches, and emitted output.
- Joinable, named pipeline workers with typed stage errors and acknowledged shutdown.
- Ordered application lifecycle phases (`boot`, `mount`, `ready`, `unmount`, `exit`)
  plus component mount/unmount effects, so startup and exit cleanup are explicit.
- `cx.use_handle()` gives an event handler a cloneable session handle, so a button
  press can request a graceful exit instead of stranding the terminal.
- Optional native raster rendering behind the `native-raster` feature; the default
  pure-Rust build needs no native toolchain.
- Optional Markdown rendering behind the `markdown` feature. Markdown output is
  rendered as selectable terminal-native components.
- `cx.use_element_ref()` reads a host element's latest committed cell geometry,
  resolved style, clipping, and scroll state; `on_element_change` observes changes.

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

## Visual documentation

The interactive guide is itself built with `icmd`. During development, install
the companion Cargo command from this repository and launch it from any
terminal:

```console
cargo install --path tools/cargo-icmd --locked
cargo icmd docs
```

The guide is a ten-chapter field guide rather than a reference dump: it teaches
the mental model, then walks components, state, layout, text, controls,
feedback, scrolling, media, themes, and production with live demonstrations and
complete Rust source. `Ctrl+K` searches every chapter, section, widget, hook,
and type; `Ctrl+B` collapses the index; `Alt+Arrow` moves between chapters and
sections; every action is also a focusable button.

Because the guide demonstrates real terminal image protocols, `cargo-icmd`
requires `icmd`'s `native-raster` prerequisites (`pkg-config`, the Chafa
development package, and libclang). See
[`tools/cargo-icmd/README.md`](tools/cargo-icmd/README.md) for details.

## Features

- `native-raster` (default): Chafa-backed Kitty/Sixel/iTerm2 payloads and symbol
  rasterization. Requires `pkg-config`, the Chafa development package, and libclang.
- `markdown` (opt-in): parses CommonMark with tables, task lists, strikethrough,
  footnotes, and GFM alerts into themed, selectable components.
- Disable default features for a pure-Rust build that emits cell output only and
  reports unsupported native protocols with a typed error.

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
