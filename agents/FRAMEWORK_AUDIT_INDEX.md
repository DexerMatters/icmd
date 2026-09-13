# `icmd` framework audit and product-safety verdict

Audit date: 2026-09-13

Scope: the current working tree, including the user's uncommitted changes. This audit does not modify framework source code. The only additions are the reports in `agents/`, so the review cannot overwrite or disguise work already in progress.

## Executive verdict

`icmd` has a promising retained terminal UI core, unusually thoughtful Unicode handling, transactional frame validation, and a useful benchmark seed. It is not yet product-safe for a public framework release.

The release blockers are not ordinary polish issues. They include an unproved FFI `Send` contract, unbounded image and tree resource use, detached workers without reliable shutdown or panic reporting, silently accepted operations for the wrong surface type, and image-queue saturation that becomes a permanent failure. Several input paths also have observable correctness faults: terminal focus is confused with widget focus, a root input can edit without focus, paste may not redraw, and Backspace/Delete can be reported as Cut.

All baseline tests, formatting, and clippy pass. That is useful evidence, but it does not cover the failure modes above. Packaging lacks essential ownership/license metadata, and the crate currently requires a native Chafa development environment for every build. Documentation quality is intentionally outside this plan at the user's direction.

The recommended decision is:

- Do not advertise API stability or production readiness yet.
- Fix the P0 safety/lifecycle items before performance restructuring.
- Treat the next release as an explicitly breaking `0.x` API regularization release.
- Require every fix to add the focused regression test named in these reports.
- Establish measured budgets before claiming performance improvements.

## Report set

| Document | Purpose |
|---|---|
| [`PRODUCT_SAFETY_AUDIT.md`](PRODUCT_SAFETY_AUDIT.md) | Correctness, resource exhaustion, lifecycle, panic, FFI, and build portability findings with concrete remediations. |
| [`PERFORMANCE_ENGINEERING_PLAN.md`](PERFORMANCE_ENGINEERING_PLAN.md) | Current benchmark evidence, complexity hot spots, proposed representations, benchmark design, and acceptance budgets. |
| [`DEDUPLICATION_AND_STRUCTURE_PLAN.md`](DEDUPLICATION_AND_STRUCTURE_PLAN.md) | Semantic duplication and coarse/piecemeal design, including the canonical owner for each concept and safe migration scope. |
| [`PUBLIC_API_REGULARIZATION_PLAN.md`](PUBLIC_API_REGULARIZATION_PLAN.md) | A coherent public module tree, naming and error policy, migration compatibility, and proposed API sketches. |
| [`PRODUCTIZATION_ROADMAP.md`](PRODUCTIZATION_ROADMAP.md) | Ordered implementation program, release gates, CI matrix, fuzzing, packaging, and rollback rules. |

The finding IDs are stable within this audit. Implementation PRs should quote them in their descriptions and regression-test names where practical.

## Severity and evidence conventions

- **P0 — release blocker:** can violate memory/soundness assumptions, corrupt interaction state, silently lose work, strand the terminal, or allow unbounded resource use through supported input.
- **P1 — high:** user-visible incorrect behavior, a public API trap, or a scaling problem likely in normal applications.
- **P2 — medium:** maintainability, consistency, packaging, or performance debt that should be fixed before claiming a stable API.
- **P3 — low:** cleanup whose value is primarily clarity or dependency reduction.

“Evidence” means a source location, a repeatable command, or a benchmark result observed in this working tree. A suspected risk is explicitly described as unproved rather than asserted as an already reproduced defect.

## Release-blocker matrix

| ID | Priority | Finding | Required gate |
|---|---:|---|---|
| SAF-01 | P0 | Terminal focus events are broadcast through the same callback as DOM focus, so multiple inputs can believe they are focused. | Two-input terminal-focus regression suite passes and the two event types are distinct. |
| SAF-05 | P0 | A full bounded image work queue is treated as permanent load failure. | Saturation test demonstrates eventual service or explicit retryable backpressure, never permanent cache poisoning. |
| SAF-06 | P0 | Frame validation accepts cell patches for raster surfaces and raster clips for cell surfaces; application silently does nothing. | Validation rejects every operation/surface mismatch transactionally with typed errors. |
| SAF-08 | P0 | Pipeline threads are detached and shutdown waits for one quiet timeout rather than an acknowledged cleanup. | Named workers are joinable; shutdown, panic, timeout, and Kitty cleanup ordering are tested. |
| SAF-09 | P0 | Image decode, transform, result queues, and native output do not share enforceable resource limits. | Checked limits and concurrent accounting tests cover encoded, decoded, transformed, cached, and output bytes. |
| SAF-10 | P0 | Public trees can be arbitrarily deep or large while core traversal is recursive. | Configured node/depth limits fail with typed errors before recursive processing. |
| SAF-13 | P0 | `unsafe impl Send for TermInfo` has no documented or tested thread-transfer proof. | Upstream contract is cited and encoded as invariants, or the raw handle never crosses its creating thread. |
| SAF-16 | P0 | Chafa is unconditional and build discovery unwraps; deployment prerequisites are neither gated nor validated. | Minimal build works without Chafa, and native-raster builds enforce a documented minimum ABI in CI. |

## High-priority correctness matrix

| ID | Priority | Finding | Required gate |
|---|---:|---|---|
| SAF-02 | P1 | Paste redraw depends on installing `on_change`. | Uncontrolled input without callback repaints immediately after paste. |
| SAF-03 | P1 | Backspace/Delete can emit a public Cut clipboard action. | Only the explicit Cut command emits Cut. |
| SAF-04 | P1 | A root input receives editing keys/paste when no widget is focused. | Targeted input delivery requires an actual focused target; global shortcuts use a separate route. |
| SAF-07 | P1 | Lexical file-path normalization changes paths containing symlinks and `..`. | File access preserves OS resolution semantics; cache canonicalization occurs only after a successful open. |
| SAF-11 | P1 | Required context/theme props panic at render time despite default-constructible macro props. | Missing ordinary input is typed, defaulted, or returned as an error; it cannot panic a worker. |
| SAF-12 | P1 | The public style assignment DSL panics on invalid dynamic fill glyphs. | Dynamic glyph validation is fallible and no public input reaches `expect`. |
| SAF-14 | P1 | Listener locks are held during callbacks and propagation cleanup is not panic-safe. | Nested dispatch and panicking-listener tests terminate deterministically and restore propagation state. |
| SAF-15 | P1 | Event polling can add the full render poll interval to input latency and can starve rendering. | A fairness/latency benchmark meets an explicit budget under continuous input. |

## Performance matrix

| ID | Priority | Current cost | Proposed direction |
|---|---:|---|---|
| PERF-01 | P1 | A one-cell render still clears/copies/scans viewport-sized buffers. | Damage-span double buffering and changed-span encoding. |
| PERF-02 | P1 | Layout recursively remeasures subtrees across intrinsic, placement, scrollbar convergence, and paint. | One frame-local layout tree with memoized measurement constraints. |
| PERF-03 | P1 | Text shaping normalizes, concatenates, clones glyph strings, sorts boundaries, and then repeats for paint. | Shared normalized storage, byte ranges, linear boundary construction, and reusable shaped layouts. |
| PERF-04 | P1 | Keyed child reconciliation scans old children for every keyed new child: O(n²). | Per-parent key index with stable unkeyed fallback. |
| PERF-05 | P1 | Context resolution repeatedly walks ancestors and clones maps and values, including `Theme`. | Persistent context chain/snapshot with `Arc` value access. |
| PERF-06 | P1 | Event routing repeatedly linearly searches regions and rebuilds ancestor routes. | Published ID index and one reusable ancestry traversal. |
| PERF-07 | P1 | Offscreen no-wrap text allocates by document width rather than visible width. | Seek directly to visible shaped items and rasterize only the viewport window. |
| PERF-08 | P2 | Input navigation and constraints repeatedly allocate complete display-unit vectors and candidate strings. | Boundary iterators/cache, one-pass normalization, and budgeted editing. |
| PERF-09 | P2 | Input pointer/vertical queries deeply clone a committed text layout. | Share one immutable `Arc<TextLayout>` across paint and queries. |
| PERF-10 | P2 | Compositing clones all layers/native image nodes and rebuilds viewport ownership data. | Borrow compact scene descriptors and retain/update compositor state. |
| PERF-11 | P2 | Commit orders the scene more than once and clones old order. | Produce one canonical sorted order per commit. |
| PERF-12 | P2 | Canvas validates and patches per cell; retained components redraw a full image on each render. | Validated batch primitives and memoized/retained canvas content. |
| PERF-13 | P2 | Pipeline bridge threads add channel hops and obscure backpressure. | Direct stage ownership with measured queue capacity. |
| PERF-14 | P2 | Lower retains a full cloned root only to remember whether one exists; host DOM rebuilds broadly. | Replace with mount state immediately, then measure persistent dirty-subtree DOM. |
| PERF-15 | P3 | Crop/diff/default-cell/fill hot paths repeat small allocations or work. | Batch only profile-proven mechanical optimizations. |

## API and structure matrix

| ID | Priority | Problem | Canonical direction |
|---|---:|---|---|
| API-01 | P1 | Root exports mix widgets, renderer protocol, runtime internals, and macro helpers. | Small stable root plus explicit `widgets`, `style`, `events`, `image`, and `advanced` tiers. |
| API-02 | P1 | `Image` and `RasterImage`, plus `image()`, name different layers ambiguously. | `CellSurface` for terminal cells and `RasterImage`/`raster_image` for pixels. |
| API-03 | P1 | Public structs expose mutable fields whose invariants can become inconsistent. | Private fields, validated constructors/builders, and accessors. |
| API-04 | P1 | Focus/global keyboard event names and dispatch outcomes are inconsistent. | Separate targeted and application events; shared propagation/default-action outcome. |
| API-05 | P1 | Controls named button/checkbox/radio/switch do not consistently own interaction semantics. | Either complete accessible interactive contracts or rename them as presentation-only views. |
| API-06 | P2 | Operator-based property assignment (`/=`, `|`, `|=`) is surprising and partly panicking. | Explicit setter/default APIs; keep macro syntax as desugaring, not public semantic machinery. |
| API-07 | P2 | Runtime/commit constructors return anonymous tuples with varying shapes. | Named handles/builders with shutdown and typed errors. |
| API-08 | P2 | Errors erase sources and public shared state exposes `Arc<Mutex<T>>`. | Source-preserving errors and an encapsulated reference type. |
| API-09 | P2 | Theme mutation can leave derived public style tokens stale. | Explicit independent tokens or a palette builder with deterministic derivation. |
| API-10 | P2 | Text/canvas setter names and canvas error behavior are inconsistent. | Domain-qualified names and one uniform drawing error/clipping model. |
| API-11 | P1 | Numeric DOM IDs are fabricable and programmatic focus authority is unclear. | Opaque generation-aware handles and checked focus requests. |
| API-12 | P1 | Public configuration fields permit invalid combinations before startup. | Private validated configuration builders. |
| API-13 | P3 | Key, props payload, and component-context construction have overlapping entry points. | One allocation-aware key path and one props/context access convention. |
| API-14 | P1 | Raw frame operations make invalid sequences easy to construct. | Typed cell/raster scene handles while retaining transactional validation. |
| DUP-01 | P2 | Cell, fill, and scrollbar glyph validation duplicate terminal-glyph invariants. | One internal validator/value object; public errors remain domain-specific. |
| DUP-02 | P2 | Input/Textarea duplicate editor host translation. | One private editor-host adapter parameterized by semantic mode. |
| DUP-03 | P2 | Generic context provider duplicates `ContextKey::provider` and adds panic-prone required props. | `ContextKey::provider` is the canonical owner. |
| DUP-04 | P2 | Theme presets are piecemeal mutation scripts. | Complete preset data literals so new fields fail at compile time. |
| DUP-05 | P2 | Scroll-area style merging applies caller style twice and obscures precedence. | Fresh semantic defaults, one canonical caller merge, then required invariants. |
| DUP-06 | P2 | Commit/runtime constructors duplicate wiring and obscure ownership. | One builder and named ownership-bearing result. |
| DUP-07 | P2 | `Attr` and `Props<T>` expose overlapping access/mutation routes. | One explicit-state attribute API and one props payload convention. |
| DUP-08 | Keep | Typed event-slot repetition preserves exhaustive callback typing. | Keep explicit; generate privately only if omissions become a problem. |
| DUP-09 | Keep | Selection controls share mechanics but have distinct domain semantics. | Share private visuals/activation, not one public generic control. |
| DUP-10 | Keep | Cell and raster images are distinct despite similar names. | Rename for clarity; do not merge representations. |
| DUP-11 | Keep | Screen/image coordinates differ intentionally in signedness/domain. | Preserve distinct types. |
| DUP-12 | Keep | Themed wrappers repeat semantic token choices. | Extract only stable mechanics, leaving policy with each component. |
| DUP-13 | P3 | One derive dependency owns no unique behavior. | Manual opaque `Debug` and dependency removal. |
| DUP-14 | P3 | Dead/stale fields and helpers can make tests own redundant state. | Prove usage, migrate assertions, then delete only unused production representation. |

## Reproducible baseline

The following commands were run from the repository root against the current dirty tree:

```text
cargo fmt --all -- --check                              PASS
cargo clippy --all-targets --all-features -- -D warnings PASS
cargo test --all-targets --all-features                 PASS
cargo package --allow-dirty --no-verify                  PASS with metadata warnings
cargo audit                                              0 vulnerabilities; 1 unmaintained warning
```

The dependency audit used `cargo-audit 0.22.2` and a RustSec database updated 2026-09-09. Its single warning is RUSTSEC-2024-0370 for `proc-macro-error 1.0.4`, reached through the direct `fmt-derive` dependency. This is not a known vulnerability result; it is a maintenance and supply-chain cleanup item.

The package command reported no license/license-file or repository ownership metadata. The generated package contains demo JPEG assets and an internal planning document, contributing to a 1.7 MiB unpacked package. There is no license, CI configuration, minimum-supported-Rust declaration, or dependency-policy configuration.

## Current benchmark snapshot

Criterion was run on Linux x86_64 with Rust 1.98.0. Absolute samples are useful as a local snapshot. Criterion's historical comparison baseline has unknown machine/worktree provenance, so its percentage deltas are signals to investigate, not proof of a regression or improvement.

| Benchmark | Current interval | Historical signal |
|---|---:|---:|
| `renderer/single_patch_2000_fragments` | 170.85–171.59 µs | +15.927% regression |
| `renderer/move_fragment_2000_fragments` | 172.33–172.96 µs | +11.952% regression |
| `renderer/dense_full_redraw` | 605.51–607.18 µs | +3.5425% regression |
| `renderer/overlap_256_layers` | 4.7109–4.7166 ms | −3.4322% improvement |
| `renderer/symbols_raster` | 296.00–297.35 µs | within local noise |
| `pipeline/text_leaf_change` | 247.44–250.43 µs | −25.007% improvement |
| `pipeline/large_wrapped_text_change` | 271.49–274.68 µs | −63.653% improvement |

The repository's existing benchmark targets ask for 2× improvement on single-patch and pipeline leaf paths, 25% on dense redraw, and less than 5% regression elsewhere. The detailed performance report converts those aspirations into controlled gates and fills missing coverage for viewport scaling, layout, event routing, keyed reconciliation, backpressure, allocation count, and emitted ANSI bytes.

## Existing strengths to preserve

The audit is deliberately strict, but the rewrite should not discard the following strong foundations:

- Frame validation is transactional: invalid batches are checked before mutation. Extend that model to surface kinds and resource budgets.
- Cell and image dimensions use checked allocation paths in several places, and pixel/cell buffers use shared ownership. Generalize those practices rather than introducing a second safety mechanism.
- Wide-cell damage repair and Unicode/input tests cover subtle terminal behavior well. New text-layout work must preserve these cases as golden behavior.
- Renderer damage is already represented explicitly. The performance work should make the existing damage model drive bounded copying and encoding instead of replacing it wholesale.
- Theme-aware presentation wrappers encode real semantic defaults. Deduplicate only shared mechanics, not domain distinctions.
- ANSI control injection through ordinary cell text is constrained by glyph validation/replacement. Preserve this boundary and add a focused invariant test before refactoring text storage.

## Definition of “product-safe” for this framework

A release is product-safe only when all of the following are true:

1. Every P0 item is closed with a regression test and reviewer sign-off.
2. No public operation panics on ordinary invalid input; failures are typed and documented.
3. Tree size/depth, input size, image decode/transform, cache, in-flight work, and emitted output all have enforceable budgets.
4. Worker panic and shutdown are observable, terminal cleanup is acknowledged, and every worker can be joined.
5. The minimal feature set builds without native Chafa; native builds are exercised on supported platforms.
6. Fuzz/property tests cover text boundaries, frame operation sequences, layout limits, and raster headers/cropping.
7. The supported API is mechanically separated from advanced/internal protocols and checked by downstream compile tests.
8. Performance gates use controlled baselines and include allocation/output-volume measurements, not only elapsed time.
9. License, ownership metadata, security contact/process, and CI are present.
10. A migration guide explains every public rename or behavior change and the compatibility period.
