# Performance engineering plan

This is a measurement-first plan for improving frame time, allocation volume, output volume, and latency without weakening Unicode, damage, or resource-safety invariants.

## Measurement rules

Performance changes are accepted only when all of these conditions hold:

1. The benchmark records workload size, terminal viewport, feature set, Rust version, build profile, CPU, OS, and commit/worktree identity.
2. The before/after comparison runs on the same machine with the same governor and background-load policy.
3. Wall time is accompanied by allocation count/bytes where practical, output bytes, and relevant operation counts such as visited nodes or shaped glyphs.
4. Correctness tests run before benchmarks. A faster renderer that skips damage, clips text incorrectly, or weakens limits is a regression.
5. A structural optimization lands with a benchmark whose input size exposes its claimed complexity improvement.
6. Noise thresholds are chosen from repeated baseline runs; Criterion's historical directory alone is not treated as provenance.

Store a machine-readable baseline artifact in CI or release automation, not only local Criterion history. Use median and confidence intervals for time, plus exact counters for allocations/bytes/visits.

## Observed local baseline

Environment observed during the audit: Linux x86_64, Rust/Cargo 1.98.0. Current intervals:

| Benchmark | Current interval | Historical comparison signal |
|---|---:|---:|
| `renderer/single_patch_2000_fragments` | 170.85–171.59 µs | +15.927% |
| `renderer/move_fragment_2000_fragments` | 172.33–172.96 µs | +11.952% |
| `renderer/dense_full_redraw` | 605.51–607.18 µs | +3.5425% |
| `renderer/overlap_256_layers` | 4.7109–4.7166 ms | −3.4322% |
| `renderer/symbols_raster` | 296.00–297.35 µs | within local noise |
| `pipeline/text_leaf_change` | 247.44–250.43 µs | −25.007% |
| `pipeline/large_wrapped_text_change` | 271.49–274.68 µs | −63.653% |

The percentage column compares with an existing Criterion baseline whose machine and worktree provenance is unknown. It identifies the single-patch and move paths as immediate investigation candidates, but should not be used as a release assertion.

## Required benchmark matrix

Before large changes, add the following parameterized groups:

| Subsystem | Workloads | Sizes |
|---|---|---|
| Renderer damage | one cell, short span, 10% sparse, full redraw | 80×24, 240×80, 500×200 |
| Layer composition | non-overlap, dense overlap, move one layer, change z-order | 10, 100, 1,000 layers |
| Keyed reconciliation | append, reverse, rotate, random reorder, replace 1% | 100, 1,000, 10,000 siblings |
| Layout | deep chain, wide siblings, nested percentages, scrollbar toggle | depth/width 10, 100, 1,000 within safety limits |
| Text | no-wrap offscreen line, wrapped prose, emoji/combining, styled spans | 1 KiB, 100 KiB, configured maximum |
| Input editing | cursor, word move, insertion, paste, max-length rejection | 100, 10,000, maximum display units |
| Events | hit test, deep bubbling, focus move, large paste | 100, 1,000, 10,000 regions |
| Canvas | fill, line, text, sparse cells | 80×24 through 500×200 |
| Images | cache hit/miss, transform, saturation, active eviction | small, budget boundary, concurrent jobs |
| Runtime | event-to-dispatch, event-to-present, shutdown | idle and continuous event/render pressure |

For each group, add deterministic counters. Examples: viewport cells scanned, `DomId` lookups, intrinsic measurements, shaped glyphs, bytes cloned, emitted bytes, and reserved image bytes.

## PERF-01 — incremental rendering still performs viewport-wide memory passes

Priority: P1

### Evidence

`src/runtime/renderer.rs:1006` begins `render_diff`. The current path takes a scratch buffer, clears/copies the full previous frame into it, resets viewport-sized dirty/changed structures, and later scans the desired frame to encode differences. Consequently a one-cell patch retains O(viewport cells) copying and scanning.

The local `single_patch_2000_fragments` result is about 171 µs and carries the strongest historical regression signal in the existing suite.

### Target design

Keep two synchronized cell buffers but mutate only damage spans:

1. Normalize damage into row-local non-overlapping spans.
2. For each span, recomposite the desired cells and compare only that span with the presented buffer.
3. Encode changed runs directly; after successful output construction, copy or swap only the changed spans into presented state.
4. Use generation stamps for temporary per-cell ownership/dirty metadata, avoiding full-buffer `fill(false)` resets.
5. Preserve a separate full-redraw path selected when damage coverage crosses a measured threshold.

Atomicity matters. If output assembly fails its byte budget, presented state must remain unchanged. Either stage changed spans until output succeeds or encode from desired state and apply to presented state only after success.

### Validation

- Exact ANSI snapshots for wide glyph repair, style resets, cursor moves, overlap, and resize remain identical or intentionally improved.
- Counter `cells_examined` for a one-cell patch stays bounded by damage expansion, independent of viewport area.
- Benchmark the same one-cell change at all three viewport sizes. Target near-flat scaling and at least 2× improvement over the local 240×80-style workload before accepting complexity claims.
- Dense redraw must not regress more than the measured noise budget; select the full path when it wins.

## PERF-02 — layout repeats recursive measurement and placement

Priority: P1

### Evidence

- `src/runtime/commit/layout.rs:123-279` recursively computes intrinsic sizes.
- Child layout and scrollbar convergence repeatedly invoke measurement; scrollbar resolution can run multiple passes around `src/runtime/commit/layout.rs:281-370`.
- Paint-time element work requests layout-derived data again, so descendants can be revisited across measure, place, scrollbar convergence, and paint.

A tree with repeated ancestor constraints can approach O(nodes × depth), and convergence multiplies it. Deep/wide/scrollbar cases are absent from current Criterion coverage.

### Target design

Build one frame-local `LayoutTree`:

```rust
struct LayoutEntry {
    intrinsic: IntrinsicSize,
    rect: Rect,
    content_rect: Rect,
    scroll_extent: Size,
    scrollbar_state: ScrollbarState,
}
```

- Bottom-up pass computes intrinsic measurements once for a constraint/style/content version.
- Top-down pass assigns rectangles and resolved overflow.
- A bounded convergence step updates only nodes whose scrollbar presence changes. Detect oscillation and return an internal layout error rather than looping indefinitely.
- Paint and event-region publication consume the same entries; neither remeasures.
- Cache keys must include all semantically relevant constraints and versions: width/height constraint, inherited style metrics, text layout key, and children/content generation.

Do not introduce a global cache until frame-local reuse is measured; stale invalidation across frames is harder than recomputation. Add cross-frame caching only for stable subtrees with explicit version keys.

### Validation

- Counter each node's intrinsic and placement visits; ordinary layouts should be approximately one of each per frame.
- Golden tests cover percentage sizing, min/max constraints, nested scrolling, scrollbar appearance/disappearance, and zero-sized viewports.
- Complexity benchmarks should show linear node visits for deep and wide trees within configured safety limits.

## PERF-03 — text layout allocates and reshapes the same content repeatedly

Priority: P1

### Evidence

- `src/basic/text_layout.rs:203` normalizes a span via two `replace` calls.
- `ShapedGlyph` owns a `String`; `TextLayout::layout` also concatenates a complete string at `src/basic/text_layout.rs:281-284` and clones symbols into items around lines 397-405.
- Boundary positions are collected and sorted around lines 431-442 even though source traversal can produce them in order.
- `src/runtime/commit/text.rs:37-60` computes natural and wrapped layouts even for modes where one is sufficient.
- Paint lays out again at `src/runtime/commit/text.rs:158-160`.
- Input rendering builds another layout in `src/elements/input/view.rs:229`.

### Target design

Introduce a shared immutable shaped representation:

```rust
struct ShapedText {
    normalized: Arc<str>,
    glyphs: Vec<GlyphRecord>,
    style_runs: Vec<StyleRun>,
    source_boundaries: Vec<Boundary>,
}

struct GlyphRecord {
    normalized_range: Range<u32>,
    source_range: Range<u32>,
    width: u8,
    flags: GlyphFlags,
}
```

Glyphs refer to ranges instead of individually allocated strings. Model tabs/replacement behavior in flags or an enum. Build boundaries monotonically in one traversal. Then produce row layout for a given wrap width without renormalizing or reshaping.

Cache `Arc<ShapedText>` by content/style-shaping key and `Arc<TextLayout>` by shaped identity plus wrap width/alignment. Measurement returns or retains the exact layout paint will consume. No-wrap measurement uses one pass.

The current source contains a semantic contradiction: a `layout_text` comment near `src/basic/text_layout.rs:167-169` describes separated content differently from the module/implementation. Resolve behavior with tests before changing representation; do not let an allocation optimization choose semantics accidentally.

### Validation

- Existing Unicode, emoji, control-replacement, tab, wrapping, hit-test, and cursor tests remain exact.
- Allocation benchmark reports allocations per glyph approaching zero after the shared backing allocation.
- Measure-plus-paint shapes once for an unchanged leaf.
- Rewrapping at a new width reuses shaping and recomputes only row placement.

## PERF-04 — keyed reconciliation is quadratic

Priority: P1

### Evidence

`src/runtime/lower.rs:236-277` processes new children. For each keyed new child, lines 248-253 scan old children to find a matching key. Reordering N keyed siblings can therefore perform O(N²) comparisons.

### Target design

For each parent reconciliation:

1. Build `HashMap<Key, FiberId>` or `HashMap<Key, old_index>` once from keyed old children.
2. Preserve the existing positional/index rule for unkeyed children.
3. Track claimed old entries so duplicate new keys produce a deterministic policy/error.
4. Reconcile in new order, then remove unclaimed old entries.

Use the crate's existing key hashing semantics. Do not silently accept duplicate keys with last-wins behavior; that produces state migration surprises. Prefer a typed development/runtime error, or at minimum a deterministic first-wins rule plus diagnostic hook.

### Validation

- Property test reconciliation identity against a simple reference model.
- State-bearing children retain state across reverse/rotate/random reorder.
- Duplicate keys, mixed keyed/unkeyed lists, insert/remove, and empty keys have explicit outcomes.
- Comparison/lookup counters and Criterion runs at 100/1,000/10,000 demonstrate expected O(N) growth.

## PERF-05 — context resolution clones ancestor data and values

Priority: P1

### Evidence

- `src/runtime/lower.rs:445-457` walks ancestors and extends context maps during component rendering.
- `src/basic/context.rs:201-209` clones the requested `T` from context.
- `src/theme.rs:750-753` therefore clones a complete `Theme` on `use_theme`.

### Target design

Make context structural sharing explicit:

- Each provider creates a persistent node `{ parent: Option<Arc<ContextFrame>>, key, value: Arc<dyn Any + Send + Sync> }`, or a persistent map if lookup profiles justify it.
- Each fiber stores the inherited context identity/version rather than a freshly extended map.
- Add `use_context_ref`/`use_context_arc` as the internal canonical path. Retain value-cloning access only as a convenience compatibility method.
- Store/share `Theme` as `Arc<Theme>`; theme selection should not clone all tokens per consumer.

Measure lookup depth. If provider chains are shallow, a linked chain can beat a persistent hash map by avoiding map copies. If lookup becomes hot, cache resolved `(context_identity, key)` results for the frame.

### Validation

- Nested provider shadowing and provider removal retain exact semantics.
- Count theme clones/allocations across 10,000 themed leaves; shared reads should not clone the theme.
- Context lookup benchmark varies provider depth and consumer width.

## PERF-06 — event lookup and route construction repeat linear scans

Priority: P1

### Evidence

- Region lookup and hit collection are linear around `src/runtime/event.rs:1405-1409`.
- Ancestor route construction repeatedly searches at `src/runtime/event.rs:1548-1568`.
- `is_pointer_interactive` performs several route checks and allocations around `src/runtime/event.rs:1477-1487`.
- Paste clones its `String` for routed listeners around `src/runtime/event.rs:1179-1182`.

### Target design

Publish one immutable `EventIndex` with each committed event tree:

```rust
struct EventIndex {
    regions: Vec<EventRegion>,
    by_id: HashMap<DomId, usize>,
    paint_order: Vec<usize>,
}
```

Use `by_id` for ancestry and focused/captured lookup. Build a route once into a reused small buffer, then consult all event capabilities along it. Represent large immutable event payloads such as paste text as `Arc<str>` so bubbling does not clone content.

Do not add a spatial index until hit-test profiles show region scanning dominates. If required, begin with viewport row buckets or a uniform grid whose memory is bounded by viewport and region limits; preserve paint-order resolution.

### Validation

- Event ordering/propagation snapshots remain stable.
- `DomId` lookups during one deep route are O(depth), not O(depth × regions).
- Bench 10,000 regions, depth 1/100/1,000, and a large paste. Count ID probes, allocations, and cloned bytes.

## PERF-07 — offscreen text allocates by document width

Priority: P1

### Evidence

`src/runtime/commit/text.rs:217-235` builds `glyphs_at` with length based on the full text rectangle width. Editor rasterization similarly constructs cells toward `rect.width` and only afterward selects the visible window. A long no-wrap input with a narrow viewport therefore allocates and touches memory proportional to the document, not what can be shown.

### Target design

- Use row item cell offsets to binary-seek the first item intersecting the horizontal viewport.
- Rasterize directly into a buffer sized to visible width plus the small wide-glyph repair margin.
- Apply horizontal offset while translating item positions; never create blank slots for the entire offscreen prefix/suffix.
- Clamp selection/caret painting to the visible range without building an intermediate full row.

This work depends on faster text-layout indexes described below, but can be introduced behind the existing layout representation first.

### Validation

At fixed 120-cell viewport, render 1 KiB, 100 KiB, and maximum-length no-wrap input scrolled to start/middle/end. Output and temporary raster allocation should remain approximately viewport-sized; time may include a logarithmic seek but not a full blank scan.

## PERF-08 — text layout queries and input navigation scan or allocate repeatedly

Priority: P2

### Evidence

- `row_of_source` scans rows at `src/basic/text_layout.rs:543-553`.
- caret and hit queries scan ranges at lines 621-700; maximum row width is rescanned around lines 531-537.
- `src/data.rs:117-131` creates a `Vec` of display units.
- Input model unit/clamp/previous/next/word operations repeatedly rebuild or scan these units.
- Normalization uses multiple replacements/collections around `src/elements/input/model.rs:176-193`.
- Insertion constraints rebuild candidate text/display units around lines 696-735 and may do so repeatedly.

### Target design

- Store monotonically ordered row source ranges and use `partition_point`/binary search.
- Store row item cell/source indexes and cached row/max widths.
- Expose a non-allocating display-boundary iterator. For an active editor, retain a boundary index invalidated or incrementally updated on edit.
- Normalize newline/control policy in one pass.
- Keep one document buffer; represent controlled drafts and selection snapshots without cloning the entire text on every transition.
- Enforce input byte and display-unit limits before expensive candidate construction.

Insertion constraints are Unicode-sensitive: inserting text can merge grapheme clusters across both boundaries. Do not optimize by simply subtracting old unit counts. Use a localized rescan with enough neighboring boundary context, validated by property tests against a full recomputation reference.

### Validation

- Random edit sequences compare optimized boundaries/limits with a simple full-recompute oracle.
- Cursor/hit operations on large documents perform logarithmic lookup plus row-local scan.
- Editing benchmarks record bytes copied and boundary rescans in addition to time.

## PERF-09 — committed input layout is deeply cloned

Priority: P2

### Evidence

`src/elements/input/view.rs:665-673` dereferences an `Arc<TextLayout>` and clones the full layout for pointer/vertical operations. Commit also clones a layout into an `Arc` around `src/runtime/commit/text.rs:322-324` while retaining other ownership.

### Repair

Make the committed layout API return `Arc<TextLayout>` or a scoped borrow. Produce one `Arc` at layout creation and pass it through paint, event hit testing, and editor hooks. Never clone glyph/item vectors for a read-only query.

### Validation

Pointer movement and vertical navigation allocate no layout-sized buffers. Concurrency/lifetime tests prove the previous frame layout cannot be mutated or observed partially.

## PERF-10 — scene composition clones data and rebuilds ownership

Priority: P2

### Evidence

- `compose_damage` around `src/runtime/renderer.rs:880` clones the layer vector.
- Native tile collection clones `ImageNode` values around the native rendering path.
- A viewport-sized `cell_owners` structure is allocated/populated while footprints are scanned.
- Image cache eviction at `src/runtime/renderer.rs:718-774` repeatedly builds pinned sets and scans for least-recently-used entries.

### Target design

- Split renderer state into fields that can be borrowed independently, allowing layers to be iterated without cloning.
- Store compact native descriptors containing shared pixel handles and crop metadata; avoid cloning fallback cell surfaces.
- Reuse ownership/rank buffers with generation stamps, and update only damaged rows where z-order is unchanged.
- Replace scan-based eviction with an intrusive/generational LRU queue plus explicit active reference accounting. Implement this after SAF-09 establishes correct hard-budget semantics.
- Append native payloads by borrow or shared string rather than cloning full encoded strings.

### Validation

Bench non-overlap/overlap/move/z-order cases with allocations. Verify the optimization does not retain stale `Arc`s that defeat cache eviction.

## PERF-11 — commit sorts scene keys twice

Priority: P2

### Evidence

Scene diffing sorts next keys around `src/runtime/commit.rs:79-81`; frame construction collects and sorts order again around lines 240-242. Old order is also cloned near line 12.

### Repair

Have scene diffing produce a canonical `ScenePlan` containing the already sorted new order and operations. Move old order with `mem::take` when ownership permits. `frame_for` consumes the plan rather than rediscovering order.

### Validation

An operation-order golden test protects deterministic frame output. A counter asserts one sort per changed scene and zero sort for a no-op frame if possible.

## PERF-12 — canvas drawing validates and patches per cell

Priority: P2

### Evidence

Canvas `set` validates and patches a single cell. `fill_rect` validates and then calls `set` repeatedly; line drawing validates once but still pays per-cell method/patch overhead; text constructs a cell and `set` reconstructs it. The canvas component recreates a full cell image and reruns its draw callback on component render around `src/canvas.rs:412-421`.

### Target design

- Add a private `put_validated` that writes a prevalidated cell into a known-in-bounds slot.
- Each primitive validates bounds/style/glyph once, clips once, then writes row spans directly.
- Accumulate one dirty region or row-span set per primitive/batch rather than one patch per cell.
- Let retained canvas content carry a version. Re-run a pure draw callback only when dimensions, theme/style dependencies, or draw identity/version changes.
- Coordinate with API-regularization work so drawing errors are either returned by the callback or impossible after validated construction; do not discard them.

### Validation

Pixel/cell golden results for clipping, wide glyphs, intersecting primitives, and text remain unchanged. Bench fill/line/text at multiple viewport sizes and record patch count.

## PERF-13 — the pipeline uses unnecessary bridge threads and channel hops

Priority: P2, coupled to SAF-08

### Evidence

The generic connection abstraction in `src/runtime/pipeline.rs` creates a forwarding thread between each worker stage. A three-stage pipeline uses five threads and two extra queues/hops.

### Repair

Construct Lower, Commit, and Renderer with the receiver for their direct upstream bounded channel and sender for their direct downstream channel. Runtime ownership is explicit in `RuntimeHandle`; stage traits can remain testable without a universal bridge.

Select channel capacities from backpressure behavior, not arbitrary constants. Publish counters for queue depth/high-water mark. Coalesce replaceable roots/frames only when semantics allow; never reorder focus/input/shutdown controls.

### Validation

- Thread-count assertion: three workers for three stages, plus an event reader only if configured.
- End-to-end leaf update latency improves or remains within noise.
- Backpressure and orderly shutdown tests pass with capacity one.

## PERF-14 — full logical root and host DOM are cloned more than needed

Priority: P2

### Evidence

- `src/runtime/lower.rs:81` stores `root_node: Option<Node>`.
- `lower` clones the complete root at line 114.
- The only later use identified is `.is_some()` near line 562.
- `build_dom` around lines 173-213 recursively constructs/clones host DOM data for rendered subtrees.

### Repair

Immediate low-risk change: replace the retained root node with a `mounted: bool` or infer it from the root fiber. This removes a full logical tree clone without semantic change.

Larger measured change: represent committed host nodes with persistent `Arc<DomNode>` subtrees and generation/version fields. Rebuild only dirty ancestor paths. Commit consumes shared immutable nodes. This requires a precise invalidation model for inherited styles, context, layout, and event handlers; do not land it as an unmeasured broad rewrite.

### Validation

Count Node/DomNode clones for a single-leaf update in a 10,000-node tree. The immediate change should remove the root clone; persistent DOM work should make clones proportional to dirty depth/subtree rather than total nodes. Identity/state behavior must match reconciliation tests.

## PERF-15 — small hot-path inefficiencies worth batching after profiles

Priority: P3

These are feasible cleanups but should not preempt the P1 work:

- `Image::diff_patch_rect` scans full surfaces; track producer dirty spans where available and retain full comparison as fallback.
- Cropping converts cell surfaces into rows and rebuilds/validates them; add an internal direct `CellSlot` crop with proven invariants.
- `Image::from_rows` clones an owned cell before deciding it is blank; classify then move.
- `CellSlot::is_default` constructs/compares with a blank cell; test fields or a canonical static blank representation directly.
- `Fill` repeatedly computes display width; store the validated width as `Cell` already does.

Each item needs a microbenchmark or a profile showing material contribution. Syntactic shortening alone is not a performance result.

## Ordered implementation sequence

1. Add benchmark provenance, counters, resource limits, and safety regression tests.
2. Fix renderer damage scaling (PERF-01); it has a clear local regression signal and bounded scope.
3. Index keyed reconciliation and events (PERF-04, PERF-06).
4. Eliminate root/layout deep clones with low-risk ownership changes (PERF-09, immediate PERF-14).
5. Build shared text shaping and visible-only rasterization (PERF-03, PERF-07, PERF-08).
6. Introduce the frame-local layout tree (PERF-02) after text layout keys are stable.
7. Refine composition/cache/canvas (PERF-10, PERF-12) after resource accounting is correct.
8. Apply profile-proven P3 micro-optimizations.

## Performance release gates

Use calibrated noise bounds from the controlled runner. Initial goals:

- One-cell render work is proportional to damage and improves by at least 2× on the audit workload.
- Keyed reorder at 10,000 siblings completes with linear lookup count and at least an order-of-magnitude improvement over the quadratic implementation.
- Measure-plus-paint shapes each unchanged text leaf once; no-wrap offscreen temporary cells remain O(viewport width).
- Layout visit counts are O(nodes) for benchmarked deep/wide trees within limits.
- Event route ID probes are O(route depth); large paste payload bytes are not cloned per ancestor.
- No accepted optimization regresses dense/full redraw or pipeline leaf time by more than 5% unless a documented safety tradeoff is approved with separate budgets.
- Image/cache work never violates SAF-09 limits to improve throughput.
- Input-to-present p99 remains within the selected interactive budget under continuous event pressure.
