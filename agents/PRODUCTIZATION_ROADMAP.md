# Productization roadmap

This roadmap turns the audit into small, ordered implementation slices. The artifacts in `agents/` are implementation plans, not framework feature work.

## Program rules

- One behavioral concern per PR where possible.
- Every PR identifies the finding IDs it closes or advances.
- Characterization/regression tests land in the same PR as a behavior change, or immediately before it.
- Source moves and algorithm changes are separate commits/PRs.
- No benchmark claim without a controlled before/after artifact.
- No compatibility alias owns logic; it delegates to the canonical API.
- No P2 cleanup delays a P0 fix unless it is a prerequisite for safely expressing that fix.
- All user work already present in the dirty tree must be preserved and reviewed separately from these planned changes.

## Release states

### State 0 — current: audit-only

Do not label production-ready. Existing checks pass, but P0 gates remain open.

### State 1 — safety preview

Allowed after all P0 issues are closed and feature/build matrices pass. API may still carry deprecations and advanced-tier aliases. Suitable for controlled adopters with explicit version pinning.

### State 2 — API candidate

Allowed after P1 correctness and API work is complete, old/internal paths are no longer used in-repo, and downstream compile fixtures pass. Performance budgets must be stable on a controlled runner.

### State 3 — product-ready `0.x`

Allowed only after fuzz/stress time budgets, shutdown/native tests, supply-chain checks, license metadata, package-content checks, and rollback rehearsals pass. This does not imply `1.0` semver stability.

## Phase 0 — freeze evidence and guard the worktree

Target: first preparatory PR

### Work

1. Record current benchmark results with commit/worktree identity and runner information.
2. Add exact regression tests for SAF-01 through SAF-07 before changing behavior where those tests can be made to fail deterministically.
3. Add test-only runtime counters: live worker count, queue high-water marks, node visits, layout visits, text shaping calls, renderer cells examined, allocation/output bytes where feasible.
4. Add a public-API snapshot/allowlist and downstream fixture crates for high-level, advanced, macro, no-default-features, and native-feature builds.
5. Add CI jobs for formatting, all-target clippy, all-target tests, feature matrix, dependency audit, package dry-run/content allowlist, and controlled benchmark smoke tests.
6. Define conservative default `ResourceLimits` values based on expected application workloads; values are configuration decisions and must be reviewed independently from enforcement code.

### Exit criteria

- Each reproduced correctness issue has a failing focused test.
- CI can distinguish pure-Rust and native builds.
- Benchmark output is attributable to an exact revision and machine class.
- The package-content check rejects accidental inclusion of `agents/`, large demo assets, caches, and local planning data unless explicitly allowed.

### Rollback

Instrumentation can be disabled behind test/bench configuration if it affects release code. Failing regression tests stay; they define known debt until fixes land.

## Phase 1 — input and event correctness

Target: four to six focused PRs

### PR 1.1 — separate focus domains (SAF-01)

- Add `TerminalFocusEvent` at application scope.
- Restrict widget `FocusEvent` to dispatcher ownership changes.
- Preserve focused ID across terminal activation unless explicit policy says otherwise.
- Cancel capture/suspend cursor separately on terminal loss.

Gate: two-input focus suite and terminal-loss capture suite.

### PR 1.2 — require actual key/paste target (SAF-04)

- Remove root fallback for targeted key/paste.
- Add explicit application-global key listener.
- Add post-publication autofocus/focus request.
- Remove input's inference of focus from received key/paste.

Gate: unfocused root input ignores edits; global shortcut still works.

### PR 1.3 — atomic edit outcomes (SAF-02, SAF-03)

- Give edit outcomes explicit command intent and `changed`.
- Redraw from `changed`, independent of callbacks.
- Only explicit Cut emits clipboard Cut.
- Remove dead submit outcome if unused.

Gate: paste redraw matrix, Cut/Delete matrix, controlled/uncontrolled tests.

### PR 1.4 — panic/reentrancy-safe event delivery (SAF-14)

- Add an explicit nested-event queue or typed reentry rejection.
- Add RAII propagation state.
- Invoke callbacks without structural locks held.
- Convert listener panic to controlled runtime termination.

Gate: nested and panicking callback tests complete without deadlock/state leakage.

### PR 1.5 — shared event semantics (API-04)

- Introduce `EventContext` and `DispatchOutcome` internally.
- Move built-in focus/scroll/edit actions behind prevent-default handling.
- Keep compatibility callback adapters only where exact.

Gate: phase/default/propagation conformance matrix for every event family.

### Rollback

Each PR retains an internal adapter to the previous listener storage so it can be reverted independently. Do not remove public old names until the full event suite is green.

## Phase 2 — resource budgets and raster safety

Target: five to seven focused PRs

### PR 2.1 — checked resource-policy type (SAF-09, SAF-10, API-12)

- Add validated `ResourceLimits` and integrate with runtime builder internals.
- Add typed limit errors with configured/requested values.
- Reject invalid configuration before starting threads or terminal mode.

Gate: generated config validation has no side effects on failure.

### PR 2.2 — tree node/depth enforcement (SAF-10)

- Iteratively validate initial roots.
- Enforce counters during component expansion.
- Propagate typed Lower errors.

Gate: exact-limit/over-limit and recursive-component tests; no worker death.

### PR 2.3 — encoded/decode limits (SAF-09)

- Enforce source file/read budgets and image reader header/dimension/pixel limits.
- Use checked arithmetic and fallible reserves.
- Preserve path/source errors.

Gate: oversized-header and arithmetic property tests allocate only bounded memory.

### PR 2.4 — shared in-flight/transform/cache accounting (SAF-09)

- Add RAII byte reservations shared by workers and renderer.
- Make hard total versus evictable target semantics explicit.
- Validate every FFI integer conversion.

Gate: concurrent stress never observes budget excess; cancellation/panic releases reservations.

### PR 2.5 — retryable bounded image pipeline (SAF-05)

- Distinguish queued/backpressured/closed.
- Add bounded deduplicated pending work and bounded results.
- Add request generations/cancellation.

Gate: blocked-worker saturation eventually services every live request without cache poisoning.

### PR 2.6 — output budget and atomic assembly (SAF-09)

- Bound ANSI/native payload construction.
- Reject before partial protocol output.
- Update presented state only after complete output assembly succeeds.

Gate: over-budget output writes zero bytes and leaves presented state unchanged.

### PR 2.7 — path semantics (SAF-07)

- Open exact caller path.
- Canonicalize only successful-open identity for cache keying.
- Make source keying uniform.

Gate: symlink/`..` topology test and error path preservation.

### Rollback

Resource accounting additions should first run in assert/telemetry mode under tests, then become enforcing. Keep the old image scheduler behind a test-only comparison harness until saturation/cancellation parity is proven.

## Phase 3 — renderer validation, native boundary, and lifecycle

Target: five focused PRs

### PR 3.1 — surface-kind validation (SAF-06)

- Track `SurfaceKind` in validation metadata.
- Reject wrong-kind operations with operation index.
- Preserve full transactionality.

Gate: adversarial operation-sequence property tests leave renderer state unchanged on error.

### PR 3.2 — Chafa feature boundary (SAF-16)

- Feature-gate native raster support.
- Make pure-Rust/no-default build independent of Chafa/pkg-config/libclang.
- Enforce minimum ABI for native builds.

Gate: clean pure-Rust container and supported native environment both pass.

### PR 3.3 — native RAII and unsafe proof/removal (SAF-13)

- Construct/use/drop term info on renderer thread, eliminating unsafe `Send`, or encode a versioned upstream guarantee.
- Wrap all native objects in RAII.
- Validate nulls, dimensions, strides, crop arithmetic.

Gate: no unexplained unsafe trait implementation; native sanitizer stress passes.

### PR 3.4 — direct channels and named workers (SAF-08, PERF-13)

- Remove per-edge bridge threads.
- Retain named `JoinHandle`s and typed stage errors.
- Expose queue capacities/high-water marks for tests.

Gate: expected worker count and capacity-one backpressure tests.

### PR 3.5 — acknowledged shutdown (SAF-08)

- Add shutdown policies (drain accepted work or cancel pending work).
- Make terminal/native cleanup an acknowledged renderer step.
- Join all workers; surface panic/stall/timeout distinctly.

Gate: fake panic/stall tests and terminal output ordering. Successful shutdown leaves zero live workers.

### Rollback

Keep pure-Rust cell renderer as the mandatory fallback throughout native changes. Runtime builder can temporarily use the old wiring behind a private test flag, but release builds must have one lifecycle owner before State 1.

## Phase 4 — low-risk consolidation and dependency cleanup

Target: four focused PRs

### PR 4.1 — remove redundant derive dependency (DUP-13)

- Manual opaque `Debug` for event listeners.
- Remove `fmt-derive`; update lockfile.
- Re-run dependency audit.

Gate: no RUSTSEC-2024-0370 path remains.

### PR 4.2 — canonical terminal glyph validator (DUP-01)

- Add cross-type policy table.
- Move shared normalization/grapheme/control/width rules to one internal owner.
- Retain domain errors and extra rules.

Gate: characterization table unchanged and property strings do not panic.

### PR 4.3 — provider and attribute ownership (DUP-03, DUP-07)

- Make typed context provider canonical.
- Add explicit Attr operations and migrate internals.
- Correct false/clear override semantics with truth tables.

Gate: no provider missing-value panic; style/focus precedence tests pass.

### PR 4.4 — theme preset completeness and scroll merge (DUP-04, DUP-05)

- Convert presets to complete palette data without changing resolved values.
- Merge scroll caller style once.

Gate: token snapshots and style precedence matrix unchanged except explicitly fixed duplicate merge behavior.

## Phase 5 — measured performance work

Target: independent PRs, ordered by dependency and audit signal

### PR 5.1 — damage-span renderer (PERF-01)

- Add viewport-scaling/cell-visit counters.
- Mutate/compare/encode damage spans only.
- Retain measured full-redraw crossover.

Gate: approximately viewport-independent one-cell work and at least 2× local improvement; dense cases within approved bound.

### PR 5.2 — linear keyed reconciliation (PERF-04)

- Per-parent key map; claimed-entry tracking; explicit duplicate-key policy.

Gate: O(N) lookup count and state identity property tests.

### PR 5.3 — indexed event publication (PERF-06)

- ID map, one route buffer, shared paste text.
- Add spatial index only if profiles still justify it.

Gate: O(depth) ID probes and no per-ancestor payload clone.

### PR 5.4 — shared committed layouts and remove root clone (PERF-09, immediate PERF-14)

- Return/retain `Arc<TextLayout>` rather than deep cloning.
- Replace unused stored root `Node` with mount state.

Gate: allocation counters prove removal; behavior unchanged.

### PR 5.5 — shaped text representation (PERF-03, PERF-08)

- One-pass normalization and shared backing text.
- Range-based glyphs, linear boundaries, binary row/item indexes.
- Property comparison to old/reference layout.

Gate: shape once per unchanged leaf; allocation and query targets met.

### PR 5.6 — visible-only text raster (PERF-07)

- Seek shaped items by visible cell window.
- Allocate only viewport slots plus repair margin.

Gate: fixed-viewport allocation remains bounded from 1 KiB through max document.

### PR 5.7 — frame-local layout tree (PERF-02)

- Bottom-up measure/top-down placement.
- Shared paint/event consumption.
- Bounded scrollbar convergence.

Gate: near-one measure/place visit per node and layout golden parity.

### PR 5.8 — composition/cache/canvas refinements (PERF-10, PERF-11, PERF-12)

- Land separately where profiles prove impact.
- Never couple cache speed changes with hard-budget semantics.

Gate: each micro/structural benchmark meets its declared threshold without correctness regression.

## Phase 6 — public API transition

Target: transition release followed by one breaking release

### Transition release

- Add `widgets`, `style`, `events`, `image`, `theme`, and `advanced` facades.
- Add canonical `CellSurface`, `raster_image`, text/canvas methods, event names, runtime builder/handle, state reference, opaque IDs, and typed frame builder.
- Migrate every in-repo caller to canonical paths.
- Retain deprecated delegates only for exact semantic matches.
- Do not alias terminal-focus/global-key behavior to targeted widget events.

### Breaking release

- Remove root advanced reexports and macro helper pollution.
- Privatize invariant-bearing placement/config fields.
- Remove tuple constructors, operator-based mutations, ambiguous image/text/canvas names, generic provider, and raw fabricable IDs.
- Put raw operations behind `advanced`; typed frame construction becomes canonical.
- Complete control interaction semantics or rename presentation-only components.

### Gates

- Public API snapshot matches the intended allowlist.
- Downstream high-level fixture imports no `advanced` items.
- Feature-off/native advanced fixtures compile and run applicable tests.
- Deprecated items have zero in-repo uses before removal.
- The control conformance and event conformance suites pass.

## Phase 7 — adversarial and release hardening

### Fuzz/property targets

1. `Cell`/`Fill`/scrollbar glyph validation over arbitrary UTF-8.
2. Text normalization, source mapping, wrapping, caret, and hit testing against a simple oracle.
3. Random input edit sequences and boundary limits against full recomputation.
4. Raw frame operation sequences; validation is transactional and never panics.
5. Random accepted trees within/above node/depth limits.
6. Raster headers, crop/resize arithmetic, cache keys, and source generation/cancellation.
7. Event sequences including removal, nested dispatch, focus/capture, terminal activation, and listener panic.

Set bounded CI smoke durations and longer scheduled stress runs. Save minimizing seeds as ordinary regression cases.

### Concurrency testing

- Model queue/backpressure/shutdown state with Loom where abstractions are compatible.
- Stress repeated runtime create/render/shutdown and worker panic.
- Run ThreadSanitizer/AddressSanitizer/UndefinedBehaviorSanitizer on feasible Rust/native configurations.
- Use Miri for pure-Rust unsafe-free core paths and any internal unsafe introduced later; do not claim it covers Chafa.

### Platform/feature matrix

- Linux pure Rust, no default features.
- Linux native raster against the minimum supported Chafa.
- Linux native raster against the newest CI-available compatible Chafa.
- Compile/test on each additional claimed OS; if native backend is unsupported, capability selection must fail deterministically rather than at link/runtime surprise.
- Debug and release test profiles for arithmetic/panic differences.

### Supply-chain and package gates

- `cargo audit` has no known vulnerabilities; unmaintained direct paths require an explicit replacement or risk decision.
- License compatibility is checked for Rust and linked native dependencies; the crate declares its own license.
- Package content uses an allowlist/exclude policy so demo media, local plans, caches, and unrelated assets do not ship accidentally.
- A clean packaged crate builds/tests in an environment that does not have the repository checkout.
- Minimum supported Rust is selected and checked mechanically if the project intends to support one.

## Release-blocking CI checklist

Every release candidate must pass:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --no-default-features
cargo test --features native-raster           # on configured native runner
cargo audit
cargo package
packaged-crate clean build/test
public API allowlist/snapshot
downstream fixture crates
focused fuzz corpus replay
shutdown/native stress suite
controlled performance regression suite
```

Exact feature names may change, but absence/native coverage is mandatory.

## Runtime observability required for product operation

Expose structured, bounded metrics/hooks without logging sensitive input contents:

- frame submitted/committed/presented/dropped counts;
- stage queue depth/high-water mark and stage failure;
- frame time by lower/layout/paint/compose/encode/write;
- damage cells versus viewport cells and emitted bytes;
- live/pending image jobs, decode/transform time, reserved/cache bytes, evictions, backpressure;
- node count/depth and limit rejections;
- event queue depth and input-to-present latency;
- shutdown phase and timeout/panic stage.

Metrics labels must have bounded cardinality: use stage/error kind, never raw `DomId`, path, text, URL, or widget key as a label. Detailed errors can be delivered to the application error handler separately.

## Failure and rollback policy

- If a new renderer/layout/text algorithm fails parity, retain the old implementation behind a private comparison test until minimized; do not ship a user-visible runtime toggle that doubles support surface.
- If a performance PR misses its declared target, revert or keep only independently valuable safety/clarity pieces.
- If native cleanup cannot be acknowledged, terminate rendering, run idempotent terminal RAII cleanup, and return a typed failure; do not continue in an unknown protocol state.
- If a budget is exceeded, reject the unit of work before allocation/output and preserve the last valid presented frame.
- If a callback/stage panics, stop accepting work, perform best-effort cleanup, join what can be joined, and report the stage/callback category.
- Never silently raise hard resource defaults to make a test pass. Any default change needs workload evidence and a memory/output impact calculation.

## Final product-ready gate

The framework is ready for a product-safety claim only when:

- every P0 and P1 safety finding is closed;
- no ordinary public input path can panic or silently succeed with no effect;
- all owned resources and work queues are bounded and observed;
- worker shutdown/panic/native cleanup are deterministic;
- high-level and advanced APIs are mechanically separated;
- ambiguity around focus, image types, controls, configuration, and runtime ownership is removed;
- performance grows according to the complexity targets in the benchmark matrix;
- fuzz, concurrency, sanitizer, feature, package, license, and dependency gates pass;
- no release gate depends on the user's current dirty edits being discarded or overwritten.
