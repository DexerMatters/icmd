# Product-safety and correctness audit

This report covers behavior, failure containment, resource limits, FFI boundaries, thread lifecycle, and build portability.

## Safety policy to adopt

The framework should enforce five rules consistently:

1. Ordinary caller input returns a typed error or a defined clipped/no-op outcome; it never panics.
2. All potentially unbounded work has a configured budget checked before allocation or recursion.
3. A background stage can report success, a typed failure, or a panic; it cannot disappear silently.
4. State transitions are explicit. Terminal, DOM, pointer-capture, keyboard-target, and application-global state are not represented by the same signal.
5. Unsafe native state is confined behind a small safe facade with written invariants that are enforced by construction.

The proposed common error envelope is illustrative rather than a demand for one giant enum. Each subsystem should keep a focused error type, while the runtime uses a stable stage wrapper:

```rust
pub enum RuntimeError {
    Lower(LowerError),
    Commit(CommitError),
    Render(FrameError),
    StagePanicked { stage: Stage },
    StageClosed { stage: Stage },
    ShutdownTimeout { pending: Vec<Stage> },
}
```

Do not convert these errors to strings at the subsystem boundary. Preserve sources, paths, operation indexes, IDs, configured limits, and observed values.

## SAF-01 — terminal focus is confused with widget focus

Priority: P0 release blocker

### Evidence

- `src/basic/events.rs:75-79` defines `FocusEvent::{Gained, Lost}`.
- `src/basic/events.rs:264` stores one `focus_event` listener on a region.
- `src/runtime/event.rs:330-331` maps Crossterm `FocusGained` and `FocusLost` to `dispatch_terminal_focus`.
- `src/runtime/event.rs:1203-1250` broadcasts that event to every region with a focus listener.
- `src/runtime/event.rs:339-383` uses the same callback for ordinary DOM focus and blur changes.
- `src/elements/input/view.rs:525-543` uses that callback to set the editor's local focused state.

### Failure mode

When the terminal window regains focus, every input listener receives `FocusEvent::Gained` and can render itself focused even though the dispatcher still has at most one focused DOM ID. On terminal focus loss, every input renders unfocused while the dispatcher can retain a focused ID. Cursor state, selection styling, and subsequent key routing can therefore disagree.

### Root cause

Two independent state machines share one event:

- application/terminal activation, which belongs at runtime or application scope;
- focus ownership among DOM regions, which belongs to the event dispatcher.

### Feasible repair

1. Add a distinct `TerminalFocusEvent` delivered through an application-level hook, not region focus listeners.
2. Keep `FocusEvent` exclusively for a change in the dispatcher-owned focused DOM ID.
3. On terminal loss, cancel transient pointer capture and optionally suspend cursor display. Do not mutate DOM focus unless a separately named policy requests it.
4. On terminal gain, restore rendering based on the retained focused ID; do not synthesize focus for every region.
5. Make terminal-focus tracking explicit in `EventState` if redraw behavior depends on it.

### Tests

- Mount two inputs; focus one; dispatch terminal lost/gained; assert exactly that input remains the DOM focus owner.
- Assert each input receives DOM focus callbacks only when ownership actually changes.
- Assert terminal focus callbacks are delivered once at application scope.
- Assert capture cancellation on terminal loss does not fabricate a widget blur/focus cycle.

### Acceptance gate

No event type can mean both terminal activation and DOM focus. The invariant `number_of_focused_widgets <= 1` is checked after every focus-related event in tests.

## SAF-02 — paste can mutate without repainting

Priority: P1 high

### Evidence

In `src/elements/input/view.rs:399-435`, paste mutates the model at lines 419-425. The redraw request at lines 427-432 is nested under the `on_change` callback check.

### Failure mode

An uncontrolled input without `on_change` accepts paste into its model but continues showing the old frame until another event happens to trigger rendering. The visible value and edit state diverge.

### Repair

Compute one `changed` result from the model operation. When true:

1. request redraw unconditionally;
2. invoke `on_change` only if present;
3. emit any selection/cursor notification from the same committed edit result.

Do not use callback presence as an invalidation policy. Apply this rule to every editing action, not only paste.

### Tests and gate

- Paste into an uncontrolled input with no listener and assert the next frame contains the pasted text.
- Paste a no-op/fully rejected value and assert no redraw is requested.
- Paste into a controlled input and assert exactly one change notification and one coalesced redraw.

## SAF-03 — Backspace and Delete can be reported as Cut

Priority: P1 high

### Evidence

- `src/elements/input/model.rs:611-625` makes `delete_range` populate the clipboard field.
- Backspace and Delete use that helper at `src/elements/input/model.rs:425-457`.
- `src/elements/input/view.rs:358-362` interprets any clipboard value in the general edit result as `TextClipboardAction::Cut`.
- The model test at `src/elements/input/model.rs:850-870` encodes the internal clipboard payload for ordinary deletion, but does not verify the public semantic event.

### Failure mode

Hosts can overwrite the system clipboard or report an accessibility/telemetry Cut action when the user merely pressed Backspace or Delete.

### Repair

Make command intent explicit:

```rust
enum EditAction {
    Insert,
    DeleteBackward,
    DeleteForward,
    Cut,
    Paste,
    Replace,
}

struct EditOutcome {
    changed: bool,
    action: EditAction,
    removed_text: Option<Arc<str>>,
}
```

Only the Cut command maps `removed_text` to `TextClipboardAction::Cut`. Backspace/Delete may keep removed text internally for undo, but the public action must remain deletion. Remove the unused `EditOutcome.submit` field unless it becomes part of a real submit transition.

### Tests and gate

- Ctrl-X with selection emits one Cut with selected text.
- Backspace with and without selection emits no clipboard action.
- Delete with and without selection emits no clipboard action.
- Undo bookkeeping, if added, may retain removed text without crossing the clipboard boundary.

## SAF-04 — root input edits without actual focus

Priority: P1 high

### Evidence

- `src/runtime/event.rs:835-852` chooses `state.focused.or_else(|| state.root())` for key delivery.
- Paste uses the same root fallback at `src/runtime/event.rs:1171-1178`.
- `src/elements/input/view.rs:296-300` sets its local focused flag after receiving a key.
- Paste also sets local focus at `src/elements/input/view.rs:417-419`.

### Failure mode

If the host region for an input is the root and no region is focused, typing or paste edits that input. Local input state then claims focus even though dispatcher state does not. A root application shortcut and a widget editing command are indistinguishable.

### Repair

- Target-specific key, text, and paste delivery must require `state.focused`.
- Application-global shortcuts need a separate listener/channel whose name communicates that it is not a focused target.
- If initial focus is desired, expose an explicit autofocus/focus request processed after event-region publication.
- A widget must derive focus only from a dispatcher focus transition, never infer focus from receiving arbitrary input.

### Tests and gate

- A root input with no focused ID ignores typing and paste.
- An explicit autofocus request focuses it and then editing succeeds.
- A global shortcut fires with no focus without sending the key to the input.
- Reconciliation removing the focused node clears focus before the next key event.

## SAF-05 — image queue saturation becomes permanent failure

Priority: P0 release blocker

### Evidence

- `src/runtime/image/manager.rs:26` creates a bounded job queue of 32 with two workers.
- `src/runtime/image/manager.rs:43-45` reduces `try_send` to a boolean.
- `src/runtime/image/manager.rs:150-183` records a failed request when scheduling returns false, and the cache then prevents retry.
- The result channel at `src/runtime/image/manager.rs:27` is unbounded.

### Failure mode

A burst exceeding available slots makes otherwise valid images permanently fail merely because the queue was momentarily full. Separately, decoded results can accumulate without a bound and retain large pixel buffers.

### Repair

Replace the boolean with a precise state:

```rust
enum ScheduleResult {
    Queued,
    Backpressured,
    Closed,
}
```

Use a manager-owned bounded pending queue with deduplication by stable source key. A full worker queue leaves the request pending and retryable. A disconnected queue produces a terminal runtime error, not an image-decode failure. Bound the result channel by both item count and reserved decoded bytes. Drain results before submitting more jobs to avoid circular pressure.

Cancellation needs a generation/token. If a source is no longer referenced, its pending job may be discarded; a result for an old generation must not overwrite a newer request.

### Tests

- Block both workers, request more than 34 unique images, release workers, and assert every referenced image eventually reaches ready or a real decode error.
- Drop references for pending images and assert capacity is reclaimed.
- Disconnect the queue and assert `Closed`, not `DecodeFailed`.
- Fill the result side under a small byte budget and assert producer memory stays within the configured allowance.

### Acceptance gate

Temporary queue pressure cannot poison the image cache. Queue/result capacity and byte ownership are exposed to runtime metrics.

## SAF-06 — frame validation ignores surface kind

Priority: P0 release blocker

### Evidence

- `src/runtime/renderer.rs:488-601` validates IDs and dimensions but does not retain whether each surface contains terminal cells or raster data.
- Application branches at `src/runtime/renderer.rs:455-477` pattern-match `Surface::Cells`; for another kind they return zero damage instead of an error.
- A raster clip can likewise be associated with a cell surface without validation rejecting it.

### Failure mode

A frame can pass validation, partially execute, and silently ignore a semantically invalid patch. The caller receives success while the display remains stale. Because this is an advanced public protocol, malformed operation sequences are feasible without unsafe code.

### Repair

Track metadata during validation:

```rust
struct ValidatedSurface {
    size: Size,
    kind: SurfaceKind,
}

enum SurfaceKind { Cells, Raster }
```

Each operation declares the required kind. Return a precise error containing the operation index and surface ID:

```rust
FrameError::WrongSurface {
    operation: usize,
    id: ImageId,
    expected: SurfaceKind,
    actual: SurfaceKind,
}
```

Keep validation transactional: the whole frame is rejected before renderer mutation. In the API regularization phase, typed cell/raster handles or separate builders should make most mismatches unrepresentable.

### Tests and gate

- Reject rectangle and cell patches targeting raster surfaces.
- Reject raster clip operations targeting cell surfaces.
- Reject use-after-remove and duplicate-create operations with operation indexes.
- Snapshot renderer state before every invalid batch and assert it is unchanged afterward.

## SAF-07 — lexical path normalization changes filesystem semantics

Priority: P1 high

### Evidence

- File loading in `src/raster.rs:156-158` calls `normalize_path`.
- `src/raster.rs:212-245` folds `.` and `..` lexically.
- The public `ImageSource::File` representation also permits callers to construct sources without the normal constructor path.

### Failure mode

On filesystems with symlinks, `symlink/../target` is resolved by the OS after traversing the symlink. Lexically replacing it with the parent path can select a different file. This is a correctness and potentially a trust-boundary problem when caller-selected paths are involved.

### Repair

- Open the exact caller path.
- After successful open, derive a canonical path or file identity only for cache deduplication if needed.
- Keep the original path in errors.
- Do not require canonicalization to succeed: sandboxed or deleted-after-open files can still be valid handles.
- Make `ImageSource` construction opaque enough that all variants share the same keying behavior.

### Tests and gate

On Unix, create a directory/symlink topology where lexical and kernel resolution differ; assert the intended file is loaded. Also test nonexistent paths, permission failure, relative paths, and cache identity after a successful open.

## SAF-08 — detached pipeline workers and heuristic shutdown

Priority: P0 release blocker

### Evidence

- `src/runtime/pipeline.rs:10-21` and `src/runtime/pipeline.rs:109-131` discard worker `JoinHandle`s.
- Connecting stages adds a bridge thread per edge. Lower, Commit, and Renderer therefore use five detached threads: three workers and two bridges.
- `src/app.rs:238-245` stops receiving after one one-second quiet timeout even though a renderer can still be decoding, blocked, or preparing cleanup.
- Runtime channel closure is collapsed into broad errors in `src/app.rs:75-97`.

### Failure mode

A stage panic can look like an unexplained closed channel. Shutdown can return before terminal cleanup is emitted, and alternate-screen/raw-mode teardown may race a late renderer write. There is no way to join workers or determine which stage stalled.

### Repair

1. Connect adjacent stages directly with bounded channels; remove bridge threads.
2. Spawn named Lower, Commit, and Renderer threads and retain their handles.
3. Return a `RuntimeHandle` containing input, output/events, a shutdown token, and joins.
4. Model shutdown as a protocol: stop accepting roots, flush accepted work according to a stated policy, emit terminal cleanup, acknowledge, then join.
5. Convert panics at the stage boundary with `catch_unwind` into `StagePanicked { stage }` after best-effort cleanup. Do not resume normal rendering after a stage panic.
6. A timeout is an explicit `ShutdownTimeout`; it does not masquerade as successful completion. The caller decides whether to abandon the process/thread.
7. Ensure terminal RAII teardown runs after renderer cleanup acknowledgement. Make duplicate cleanup idempotent.

### Tests

- Fake each stage panic and assert the correct stage is reported.
- Fake a stalled renderer and assert timeout includes the pending stage.
- Record writes and assert Kitty cleanup precedes alternate-screen/raw-mode teardown.
- Repeated shutdown is harmless.
- Dropping the handle without explicit shutdown initiates best-effort shutdown and never blocks forever.
- A normal run joins all workers; a test-only live-worker counter returns to zero.

### Acceptance gate

No production thread is intentionally detached. Successful shutdown means all accepted work has followed the selected drain/cancel policy, cleanup was acknowledged, and worker joins completed.

## SAF-09 — image and output memory are not comprehensively bounded

Priority: P0 release blocker

### Evidence

- `src/raster.rs:123-125` reads a complete file into memory before decoding.
- `src/raster.rs:116-120` uses `image::load_from_memory`; in the resolved `image 0.25.10` source, default limits include a 512 MiB allocation ceiling but no application-specific width/height policy.
- `src/raster.rs:508-527` computes transform dimensions and then allocates a pixel vector. Saturating multiplication prevents arithmetic wrap but can turn an invalid request into a huge allocation.
- Public `RendererConfig.cell_pixel_size` combines with public dimensions, so valid integer inputs can request multi-gigabyte transforms.
- `src/runtime/renderer.rs:718-774` enforces cache size only after allocation and keeps active transforms pinned, making the configured cache limit a soft target.
- SAF-05 identifies an unbounded decoded-result channel.

### Failure mode

Large or adversarial image headers, dimensions, cell pixel sizes, concurrent jobs, and active transforms can exceed acceptable memory before cache eviction. Allocation failure can abort the process. Very large ANSI/native payloads can similarly create output spikes.

### Repair

Create a single validated `ResourceLimits` policy owned by runtime configuration:

```rust
pub struct ResourceLimits {
    pub max_input_bytes: usize,
    pub max_nodes: usize,
    pub max_tree_depth: usize,
    pub max_encoded_image_bytes: usize,
    pub max_source_width: u32,
    pub max_source_height: u32,
    pub max_source_pixels: u64,
    pub max_decoded_image_bytes: usize,
    pub max_in_flight_image_bytes: usize,
    pub max_transform_pixels: u64,
    pub max_cache_bytes: usize,
    pub max_output_bytes_per_frame: usize,
}
```

Implementation requirements:

- Use `checked_mul`/`checked_add`; overflow is `LimitExceeded`, never saturation followed by allocation.
- Check dimensions and pixels from the decoder header before allocating the output buffer.
- Configure `image::io::Reader` limits from framework policy instead of relying on dependency defaults.
- Stream from `File` where supported; otherwise reject by metadata/read budget before buffering.
- Reserve decoded and transformed bytes in a shared atomic/accounting object before work begins; release with RAII on every return path.
- Use `Vec::try_reserve_exact` and convert capacity failure to a typed resource error.
- Define whether cache bytes include active entries. Prefer a hard total budget with explicit pinned/in-use accounting; if eviction alone is soft, name it `evictable_cache_target_bytes`.
- Bound encoded terminal payload assembly. If a single valid render exceeds the frame output budget, fail before writing a partial escape sequence.
- Validate Chafa integer conversions, row strides, crop coordinates, and cell-to-pixel multiplication before FFI.

### Tests

- Exact-boundary success and boundary-plus-one failure for every limit.
- Crafted huge image headers with tiny files; no large allocation occurs.
- Multiplication overflow combinations for source size, cell size, and RGBA stride.
- Multiple workers reserve concurrently; observed total never exceeds the budget.
- Active/pinned cache entries cannot silently push total owned bytes above the hard budget.
- Output exceeding its budget produces no partial terminal write.
- Property test: any accepted dimensions produce allocation lengths representable in `usize` and FFI lengths representable in the destination integer type.

### Acceptance gate

The framework can state and mechanically verify an upper bound for memory owned by image loading/rendering for a given configuration. All failures occur before the oversized allocation or partial terminal protocol write.

## SAF-10 — recursive processing accepts unbounded public trees

Priority: P0 release blocker

### Evidence

Lowering recursively builds, reconciles, removes, and resolves inherited state in `src/runtime/lower.rs`. Commit recursively measures and paints in `src/runtime/commit/layout.rs`, `src/runtime/commit/text.rs`, and related traversal. Public `Node` construction permits arbitrary depth and node count.

### Failure mode

A deeply nested tree can overflow a worker stack. A very wide tree can exhaust memory or cause extreme frame latency. Because workers are detached today, a stack overflow/panic can also collapse into a generic closed runtime.

### Repair

- Add `max_nodes` and `max_tree_depth` to the shared limits policy.
- Validate an incoming root iteratively before recursive lowering, using an explicit stack and checked node count.
- Return `LowerError::TreeTooDeep { limit, observed_at_least }` or `TreeTooLarge` through the typed runtime channel.
- Only after the guard exists, convert high-risk traversals to iterative forms where it simplifies control flow. A limit remains necessary even with heap stacks because work/memory must be bounded.
- Ensure component expansion counts toward limits. Recheck incrementally during expansion, because the initial logical node does not reveal all rendered descendants.

### Tests and gate

- Trees exactly at node/depth limits render.
- Limit-plus-one trees return typed errors without worker death.
- A component expanding recursively is stopped by the same depth counter.
- Reconciliation removal of a deep accepted tree completes under the configured limit.
- Fuzz-generated trees never crash or hang; they render or return a budget error.

## SAF-11 — default-constructible provider props contain required values

Priority: P1 high

### Evidence

- `src/basic/context.rs:67-77` calls `expect` when generic provider key/value props are absent.
- `src/theme.rs:566-568` calls `expect` when the theme provider value is absent.
- The UI macro constructs default props and applies only the supplied attributes, so omission is syntactically valid and fails later on a worker.

### Failure mode

Ordinary public component construction compiles and panics during rendering. With detached workers, the visible symptom can be a closed runtime rather than a useful error.

### Repair

- Make `ContextKey::provider(value, child)` the canonical typed construction path.
- Remove or deprecate the generic `provider` component whose required generic values cannot be represented by `Default` safely.
- For a theme provider, either use `Theme::default()` when omission has coherent semantics or require a typed constructor outside the permissive macro path.
- If the macro must support required props, add compile-time required-field tracking; do not substitute a later `expect`.
- Until migration completes, convert legacy omission to a render error with component name and missing field.

### Tests and gate

Compile tests prove required constructors cannot omit values. Legacy macro omission returns a typed error during the compatibility window and never panics a stage.

## SAF-12 — invalid fill glyphs panic through public assignment

Priority: P1 high

### Evidence

`src/props.rs:635-645` implements string/character assignment using `Fill::new(...).expect(...)`. A caller-controlled glyph with invalid display width reaches the panic through a public, otherwise safe API.

### Repair

- Keep infallible assignment only for an already validated `Fill`.
- Add `TryFrom<&str>`, `TryFrom<char>`, or an explicit `set_fill` returning `FillError` for dynamic text.
- If the UI macro requires concise literals, give it a literal-specific checked expansion or make component construction return an error. Do not hide a runtime `expect` behind an operator.
- Audit all public `expect`, `unwrap`, indexing, and integer casts under the same rule.

### Tests and gate

Invalid empty, zero-width, control, and multi-cell/multi-grapheme fills return stable errors. Property-generated strings do not panic the public style path.

## SAF-13 — the Chafa handle has an unproved `Send` contract

Priority: P0 release blocker

### Evidence

`src/runtime/renderer.rs:126-133` contains `unsafe impl Send for TermInfo {}` without a safety justification. The wrapper owns a native Chafa pointer and is moved into renderer-related execution. Rust cannot validate whether that object is thread-affine or whether associated library initialization is thread-safe.

### Risk statement

This audit does not assert that Chafa's object is unsound to move. It asserts that the unsafe contract is unproved in the codebase, which is sufficient to block a product-safety claim.

### Repair options

Preferred: construct, use, and destroy the Chafa term-info object entirely inside the named renderer worker. Then the pointer does not require `Send`.

Alternative: cite the precise upstream versioned thread-safety guarantee and encode all required invariants in the wrapper. The unsafe implementation must have a `SAFETY:` explanation covering ownership, refcounting, library initialization, mutation, destruction thread, and concurrent access. Pin a minimum compatible Chafa version.

In both cases:

- Wrap native pointers in narrow RAII types so every early return releases them.
- Validate null pointers and all integer conversions before dereference/call.
- Do not expose native pointers or native lifetime requirements in the public API.
- Exercise native paths under address/undefined-behavior sanitizers where supported. Miri cannot validate the foreign library.

### Tests and gate

Native stress tests repeatedly initialize, render, and tear down on the supported execution model. Sanitizer jobs pass. The unsafe block has a reviewable version-specific invariant, or the unsafe `Send` implementation is gone.

## SAF-14 — listener invocation is not panic-safe or reentrancy-safe

Priority: P1 high

### Evidence

- `src/basic/events.rs:29-32` holds a `Mutex<FnMut>` while invoking the callback.
- Reentry into the same listener can deadlock because the non-reentrant mutex remains held.
- A panic poisons the mutex, and subsequent lock calls can panic.
- Keyboard propagation uses begin/end operations around thread-local state at `src/basic/events.rs:202-233`; dispatcher call sites such as `src/runtime/event.rs:857-878` can skip the end operation if a callback unwinds.

### Repair

First choose a deliberate callback model:

- Recommended: serialize event delivery through an explicit queue. Nested dispatch enqueues a new event and runs it after the active callback returns. The `FnMut` remains safe without recursive lock acquisition.
- If recursive delivery must be rejected, use `try_lock` and return `DispatchError::ReentrantListener` rather than blocking.

Then add an RAII propagation guard whose `Drop` always restores thread-local state. Catch panics at the application callback boundary, record listener/event identity, request terminal shutdown, and surface `RuntimeError::ApplicationPanicked`. Decide explicitly whether a poisoned listener is disabled or the whole runtime terminates; continuing in an unknown partially-mutated state is not safe.

Do not invoke arbitrary user callbacks while holding dispatcher/state locks. Snapshot the necessary callable/reference state, release structural locks, invoke, then apply queued transitions.

### Tests and gate

- A callback dispatching another event terminates without deadlock and ordering is deterministic.
- A panicking callback restores propagation state and triggers controlled runtime teardown.
- The next independent test/event does not inherit stop-propagation state.
- Focus/capture mutation queued from a callback is applied at the defined boundary.

## SAF-15 — main-loop latency and fairness are uncontrolled

Priority: P1 high

### Evidence

`src/app.rs:225-235` waits for renderer output for the configured poll interval before checking terminal events. The default interval is 50 ms. A zero duration can busy-spin. The subsequent event-drain loop has no per-tick budget and can starve rendering under continuous input.

### Repair

- Validate that polling/backoff values are nonzero and within a sane range.
- Prefer a dedicated Crossterm event reader feeding a bounded runtime event channel, then select fairly across renderer output, terminal events, and shutdown.
- If platform constraints require polling, cap events processed per iteration and always give ready render output a chance before the next drain batch.
- Coalesce redundant mouse-move/resize events, but never key, paste, focus, or shutdown events.
- Instrument input-to-dispatch and input-to-present latency histograms in benchmarks/tests.

### Tests and gate

Under a continuous synthetic mouse stream, renderer output continues to present and a key event stays below the selected p99 latency budget. Zero/invalid polling configuration is rejected rather than spinning.

## SAF-16 — native Chafa is an unconditional, fragile build dependency

Priority: P0 for portable product distribution

### Evidence

- `Cargo.toml` includes `chafa-sys` unconditionally.
- Its resolved build script calls `pkg_config::probe_library("chafa").unwrap()` and generates bindings from installed headers.
- This requires `pkg-config`, Chafa development headers/libraries, and bindgen/libclang at consumer build time.
- No minimum Chafa version or supported platform matrix is enforced.

### Failure mode

Consumers who need only cell rendering cannot build the crate without native prerequisites. Native ABI behavior depends on the locally installed headers. Discovery errors terminate with an upstream unwrap rather than an actionable framework capability error.

### Repair

- Put native raster support behind a named feature such as `native-raster`; keep the default feature set pure Rust unless raster is fundamental enough to justify a clearly stated default.
- Gate native modules and public variants consistently; provide a deterministic unsupported-capability error when the feature is absent.
- Enforce a minimum tested Chafa version through package discovery.
- Prefer released/generated bindings for the supported ABI or make bindgen an explicit maintainer feature, reducing consumer toolchain requirements.
- Test no-default-features and native-feature builds. Include at least Linux in native CI and compile-only jobs for other claimed platforms.
- Audit linked native-library licensing and redistribution separately from Rust crate licenses.

### Acceptance gate

`cargo test --no-default-features` runs on a clean Rust-only environment. The native feature fails early with a precise prerequisite error or passes against a declared ABI range.

## Cross-cutting panic audit

The findings above identify public-input panics with known impact. The implementation program should also classify every `unwrap`, `expect`, direct index, and unchecked cast into one of three buckets:

| Bucket | Required action |
|---|---|
| Proven internal invariant | Keep only with a nearby invariant assertion/test; avoid user-controlled values in the proof. |
| Recoverable external/runtime condition | Return a typed error with the operation and relevant value. |
| Process invariant during terminal teardown | Use best-effort cleanup and retain the first meaningful error; never skip teardown because a later cleanup step fails. |

A mechanical count is not the acceptance criterion. Each remaining panic site needs a reason it cannot be reached through the supported safe API.

## Required negative and adversarial testing

Add these suites before calling the runtime hardened:

- Property tests for arbitrary Unicode edits, selection ranges, wrapping modes, tabs, controls, combining marks, emoji sequences, and width changes.
- State-machine tests for focus, capture, terminal activation, reconciliation removal, and nested events.
- Sequence fuzzing for `Frame` operations, including invalid IDs, kinds, sizes, clips, moves, and removal ordering.
- Header-oriented image fuzzing with strict limits and no full decode for rejected inputs.
- Tree-shape generation for deep, wide, and component-expanded limits.
- Concurrency tests for image reservation, cancellation, shutdown, and stage panic.
- Terminal-protocol golden tests that verify cleanup and forbid partial oversized output.

The test harness should impose timeouts and track live threads/allocated budget tokens so a deadlock or leaked reservation produces a direct test failure.
