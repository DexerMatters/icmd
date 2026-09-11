# Rendering benchmark procedure

The rendering benchmark uses fixed scene data, a 240×80 viewport, symbol
raster output, and an explicit 8×16 cell size. Run it on an otherwise quiet
machine with the same Rust toolchain before and after a rendering change:

```sh
cargo bench --bench rendering -- --save-baseline before
cargo bench --bench rendering -- --baseline before
```

Run each command three times and compare the median Criterion estimates. The
acceptance target is a 2× improvement for the single-patch and pipeline leaf
updates, a 25% improvement for dense text redraw, no regression beyond 5% for
the other cases, and no increase in ANSI bytes for contiguous row updates.
