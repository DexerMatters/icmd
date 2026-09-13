# icmd

A retained, terminal-native UI framework for Rust.

Highlights:

- Declarative composition through a small `ui!` macro and ordinary components.
- Unicode-aware text layout: grapheme clusters, wide cells, emoji merging, and wrapping.
- Transactional frame validation: invalid batches are rejected before any renderer mutation.
- Enforceable resource budgets for trees, images, caches, and emitted output.
- Joinable, named pipeline workers with typed stage errors and acknowledged shutdown.
- Optional native raster rendering behind the `native-raster` feature; the default
  pure-Rust build needs no native toolchain.

```rust
use icmd::{render, ui, RuntimeConfig};

fn app(_cx: &mut icmd::ComponentContext, _props: &icmd::Props<()>) -> icmd::Node {
    ui! { <icmd::paragraph>"Hello, terminal"</icmd::paragraph> }
}

fn main() -> Result<(), icmd::RenderError> {
    render(app.apply(()), RuntimeConfig::default())
}
```

## Features

- `native-raster` (default): Chafa-backed Kitty/Sixel/iTerm2 payloads and symbol
  rasterization. Requires `pkg-config`, the Chafa development package, and libclang.
- Disable default features for a pure-Rust build that emits cell output only and
  reports unsupported native protocols with a typed error.

## License

Licensed under either of MIT or Apache-2.0, at your option.
