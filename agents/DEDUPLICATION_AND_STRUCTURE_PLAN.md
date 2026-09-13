# Semantic deduplication and structural consolidation plan

This report targets duplicated meaning and piecemeal ownership. It deliberately does not recommend abstraction merely because two functions look similar. A consolidation is justified only when one invariant or behavior currently has multiple owners.

## Consolidation rules

For every candidate:

- Identify the semantic invariant, not only matching syntax.
- Name one canonical owner.
- Preserve behavior with characterization tests before deletion.
- Migrate callers in small mechanical steps.
- Do not force semantically distinct components through a generic abstraction that erases their policy.
- Treat public compatibility independently from internal canonicalization: old names can delegate during a transition without remaining implementation owners.

## DUP-01 — terminal glyph validation has multiple owners

Priority: P2

### Evidence and overlap

- `src/data.rs:230-303` validates the text represented by a terminal cell.
- `src/props.rs:186-245` performs closely overlapping validation for `Fill`.
- Scrollbar glyph configuration enforces the same essential single-terminal-glyph/width constraint through its own public type path.

The shared invariant is: normalized content must represent one printable terminal grapheme with an allowed display width and no control behavior. `Cell` then has additional cell-specific style/storage behavior; `Fill` has fill-specific width policy; scrollbar glyphs require width one.

### Canonical owner

Create one crate-private validator/value layer in the text/cell data domain, for example:

```rust
enum AllowedGlyphWidth { One, OneOrTwo }

struct ValidatedTerminalGlyph {
    text: Arc<str>,
    width: u8,
}

fn validate_terminal_glyph(
    value: &str,
    width: AllowedGlyphWidth,
) -> Result<ValidatedTerminalGlyph, TerminalGlyphError>;
```

`Cell`, `Fill`, and scrollbar configuration remain public domain types. They translate the internal error to their existing public errors during compatibility, so this is not a forced public error merger.

### Migration

1. Add table-driven characterization tests that run identical valid/invalid values through all three current paths and record intended policy differences.
2. Implement the private canonical validator.
3. Switch `Fill`; switch `Cell`; then switch scrollbar glyphs.
4. Delete duplicate width/control/grapheme helpers only after the table proves parity.
5. Cache validated width in `Fill` so validation consolidation also removes repeated width calculation.

### Compatibility risk

Medium. Unicode segmentation and width policy are behaviorally sensitive. Any existing disagreement must be classified as intended domain policy or a bug before consolidation. Do not “fix” disagreement implicitly.

### Acceptance gate

There is one implementation of normalization, grapheme count, control rejection, and width calculation. Domain wrappers contain only their extra rules.

## DUP-02 — Input and Textarea duplicate editor-host translation

Priority: P2

### Evidence and overlap

`src/elements/input/mod.rs` exposes sibling input/textarea component paths that translate public props into the same internal editor model/view mechanics with repeated style, placeholder, event, and value wiring. The underlying editing domain is shared, while single-line submission/newline policy and multi-line wrapping/navigation are distinct.

### Canonical owner

A private editor-host adapter owns shared prop translation:

```rust
enum EditorMode {
    SingleLine { submit: SubmitPolicy },
    MultiLine { wrap: WrapMode },
}

struct EditorHostConfig { /* already-normalized common fields */ }

fn editor_host(config: EditorHostConfig, mode: EditorMode) -> Node;
```

Public `InputProps` and `TextareaProps` remain separate. They express different semantic controls and should not become one enormous public props type.

### Migration

1. Add shared behavior tests parameterized over both controls: focus, selection, paste, disabled/read-only, controlled/uncontrolled value, placeholder, and style precedence.
2. Record intentional differences: newline handling, submit, vertical movement, wrapping, and height.
3. Extract only identical prop normalization and listener wiring.
4. Encode differences in `EditorMode` and small mode-specific functions rather than conditionals spread through view code.
5. Remove obsolete per-wrapper helpers.

### Compatibility risk

Medium. Input behavior is stateful; a broad refactor can disturb hook ordering and controlled drafts. Preserve component hook order or introduce stable named hooks before moving code.

### Acceptance gate

Common editor state/wiring has one owner, while every single-line/multi-line policy difference is explicit and separately tested.

## DUP-03 — generic provider duplicates `ContextKey::provider`

Priority: P1 because the duplicate is panic-prone

### Evidence and overlap

- `src/basic/context.rs` offers direct `ContextKey` provider construction.
- The generic provider component stores required key/value in defaultable props and calls `expect` when omitted around lines 67-77.

Both create the same context-provider node, but the component wrapper weakens construction invariants.

### Canonical owner

`ContextKey::provider(value, child)` is the canonical public operation because the type system requires both key identity and value at the call site.

### Migration

- Route legacy component construction through the key method only after validating fields without panic.
- Deprecate the generic component API for one compatibility cycle if it has external users.
- Migrate internal/theme call sites to typed provider constructors.
- Remove its props type and missing-field branches after the compatibility window.

### Compatibility risk

Low to medium. Source migration is simple, but UI macro users may need a different expression shape. Cover both shapes with downstream compile fixtures during the transition.

## DUP-04 — theme presets are piecemeal mutation programs

Priority: P2

### Evidence and problem

Preset construction in `src/theme.rs` starts from an ANSI/default theme and then mutates subsets of color/style fields for each mode. This duplicates the mapping logic across presets and creates a completeness hole: adding a new theme token compiles even if a preset forgets to initialize it, silently inheriting an unrelated base value.

This is semantic duplication because each preset partially owns the complete color-token invariant.

### Canonical owner

Use complete preset data:

```rust
struct ThemePalette {
    background: Color,
    foreground: Color,
    primary: Color,
    // every palette token, no defaults
}

const fn palette_for(preset: ThemePreset, mode: ThemeMode) -> ThemePalette;
```

Then one `Theme::from_palette` derives default typography, borders, selection, and scrollbar tokens. Complete literals make a newly added palette field a compile error for every preset.

### Migration

1. Snapshot every current `(preset, mode)` resolved token.
2. Translate each to a complete palette literal without changing values.
3. Centralize derivation once.
4. Decide API-09's policy for later direct mutation: independent full tokens versus palette-derived builder. Do not combine that behavior change with the mechanical preset conversion.

### Compatibility risk

Low for the data-literal conversion if snapshots are exact; high if derived-token semantics change at the same time. Split those commits.

## DUP-05 — scroll-area style merging applies caller style twice

Priority: P2

### Evidence

`src/elements/scroll.rs:34-45` clones caller style into defaults and later sends caller style through host-prop merging again. This is both duplicated work and unclear precedence. Required overflow behavior can also become dependent on merge order.

### Canonical owner

The scroll component owns semantic defaults and required overflow policy; the generic host-prop merger owns caller override application. Each should run once:

1. construct a fresh default scroll style;
2. merge caller style exactly once through the canonical style merge;
3. apply non-overridable scroll invariants afterward, or represent them outside style if callers should never change them.

### Migration and tests

Characterize background/fill/dimensions/overflow precedence with default, caller override, and explicit clear/unset cases. Then remove the initial caller clone. This must coordinate with API-06/API focus override semantics because Boolean/default merging currently cannot always express a false override.

## DUP-06 — runtime and commit construction duplicate wiring paths

Priority: P2

### Evidence

Commit exposes four constructor variants (`new`, event-aware, config-aware, and config-plus-events) whose tuple results vary. Runtime startup returns additional anonymous tuple parts. The same channel/stage wiring and optional dependency selection is spread among these paths.

### Canonical owner

One internal `RuntimeBuilder`/`CommitBuilder` owns defaults, validation, channel capacities, event publication, resource limits, and thread naming. One `build/start` path produces named handles.

Public convenience functions may remain thin delegates; they must not recreate wiring.

### Migration

1. Introduce builder internals without public changes.
2. Make every current constructor delegate and assert equivalent configurations in tests.
3. Introduce named public handles as described in API-07.
4. Deprecate tuple constructors, then remove them in the planned breaking release.
5. Delete bridge/wiring code made obsolete by direct stage channels.

### Acceptance gate

There is exactly one place that selects defaults, validates runtime configuration, creates channels, and owns worker joins.

## DUP-07 — attribute access and props representation have overlapping APIs

Priority: P2

### Evidence

- Repeated helpers clone values from `Attr<T>` and listener slots instead of one canonical accessor.
- `Props<T>` exposes the extra/user payload through a public field, `extra()`, `extra_mut()`, dereferencing, `with_extra`, and `map`-style APIs.
- Attribute assignment/default operators provide another path to similar state mutations.

Multiple representations are not just cosmetic: they make it unclear which operations preserve explicit-vs-default state, and they enlarge the compatibility surface.

### Canonical owner

- `Attr<T>` owns explicit/default/unset semantics and supplies `as_ref`, `cloned`, `copied`, `set`, and `set_default`.
- `Props<T>` stores a private `data: T` and exposes either transparent `Deref`/`DerefMut` or named `data/data_mut`, not both plus a public field. Prefer named access for clearer generic diagnostics unless macro ergonomics require deref.
- UI macro expansion uses explicit attribute operations internally; public arithmetic traits are transitional compatibility only.

### Migration

Characterize explicit/default precedence first. Add canonical methods, migrate all internal call sites mechanically, deprecate redundant public routes, then remove them in the breaking release. Avoid changing merge semantics in the same commit as renaming accessors.

## DUP-08 — event field selection is repetitive but should stay explicit

Priority: keep, not consolidate

Pointer event slot selection and event-handler fields contain similar match arms. The repetition is an exhaustive mapping from event kind to a distinct semantic callback. A generic map keyed by event enum would erase type relationships, make missing cases runtime failures, and add lookup overhead.

Keep this code explicit. If boilerplate becomes error-prone, use a private macro that generates fields/accessors and exhaustive tests from one declaration, but retain typed public callbacks. The canonical owner remains `EventHandlers`; there is no second behavior implementation to delete.

## DUP-09 — checkbox, radio, and switch share mechanics but retain domain policy

Priority: maintain current limited consolidation

These controls share selection visuals and currently use a `selection_control` helper. That is appropriate mechanical reuse. Do not collapse them into one public generic selection component:

- radio has group/exclusive semantics;
- checkbox supports an independent Boolean or potentially mixed state;
- switch conventionally communicates immediate on/off state.

The actual problem is API-05: their interaction contract is incomplete. Complete each domain behavior on top of the shared private visual/activation primitive.

## DUP-10 — `Image` and `RasterImage` are not duplicates

Priority: rename, do not merge

The terminal-cell surface and decoded pixel image both represent visuals, but they have different storage, damage, clipping, validation, and renderer behavior. Merging them into an enum at the public high-level API would move every operation to runtime checks.

Keep distinct types. Rename the terminal-cell type to `CellSurface` and keep `RasterImage` for pixels, as proposed in API-02. Shared dimensions or resource helpers can remain private traits/functions.

## DUP-11 — coordinate types are intentionally distinct

Priority: keep

Signed screen positions and unsigned image positions prevent invalid coordinate domains. Superficial field similarity is beneficial type separation. Do not replace them with a universal point unless generic algorithms can preserve signedness and domain at compile time.

## DUP-12 — themed wrappers repeat semantic defaults intentionally

Priority: keep policy, extract only stable mechanics

Button, feedback, input, and other themed components repeat calls that select colors/borders based on state. Much of this repetition encodes component semantics. A universal “themed box” would make state precedence implicit and couple unrelated controls.

Safe extraction candidates are narrow and value-like:

- resolve disabled/active/focused visual state to an enum once;
- shared border/focus-ring overlay mechanics;
- shared host-prop merge invocation.

Each component should still own the mapping from semantic state to tokens.

## DUP-13 — `fmt-derive` is redundant for one Debug implementation

Priority: P3 dependency cleanup

### Evidence

The direct `fmt-derive` dependency is used for one `Debug` derive on the event-listener wrapper. It pulls `fmt-derive-proc`, `proc-macro-error 1.0.4`, and a Syn 1 toolchain. The audit reports RustSec RUSTSEC-2024-0370 as an unmaintained warning for that transitive dependency.

### Canonical owner and repair

Implement `Debug` manually at the wrapper, printing an opaque non-exhaustive listener marker rather than callback internals. Remove `fmt-derive` from `Cargo.toml` and regenerate the lockfile.

### Tests and gate

Assert the debug format is stable enough for tests but does not reveal implementation addresses. Run all-target clippy/test and dependency audit; the warning path should disappear.

## DUP-14 — stale helper methods/fields obscure canonical state

Priority: P3 after characterization

Candidates identified during inspection:

- `EditOutcome.submit` is present but not part of a live submit transition.
- Text-layout production structures retain fields/methods apparently used only by tests or old representations.
- Event focus-target, input surface-style, editor-measure, and text-row helper paths have overlapping or stale responsibilities.

Do not delete from a name-only search. For each candidate:

1. use `rg` plus compiler dead-code evidence across all targets/features;
2. determine whether tests are asserting a public invariant through the field;
3. migrate the assertion to a public/canonical query if necessary;
4. remove the unused storage and rerun size/allocation benchmarks.

The canonical state should be the minimum representation required by production behavior; tests should not force duplicate cached fields solely for convenient assertions.

## Coarse modules and proposed decomposition

Several files combine enough independent responsibilities to hinder safe changes. Split by invariant boundary, not arbitrary line count.

### `runtime/renderer.rs` (~2,200 lines)

Proposed private modules:

```text
runtime/renderer/
  mod.rs          Renderer orchestration and public advanced facade
  frame.rs        operation validation and transactional application
  scene.rs        retained surfaces/layers and damage production
  compose.rs      cell compositor and ownership/rank buffers
  ansi.rs         diff encoding, cursor/style state, output budgets
  native.rs       feature-gated Chafa safe facade and Kitty protocol
  cache.rs        raster transform cache and byte accounting
```

Key dependency direction: `frame -> scene -> compose -> ansi`; `native` consumes validated compact raster descriptors; `cache` cannot call terminal output. Keep `SurfaceKind` metadata in `frame/scene`, not duplicated in each backend.

Migration should be file moves plus visibility tightening first. Do not combine module splitting with the damage algorithm rewrite; otherwise review cannot distinguish moved behavior from changed behavior.

### `basic/text_layout.rs` (~2,000 lines including tests)

Proposed private modules:

```text
basic/text_layout/
  mod.rs          stable TextLayout facade
  normalize.rs    one-pass source normalization and source mapping
  shape.rs        glyph records and widths
  wrap.rs         row construction for width/mode
  index.rs        source/cell/row queries
  tests.rs        behavior and property reference model
```

Do not expose these phases publicly until their representations are proven. The public object should remain immutable and query-oriented.

### `runtime/event.rs` (~1,570 lines)

Proposed private modules:

```text
runtime/event/
  mod.rs          dispatcher facade and state transition queue
  index.rs        published regions, ID lookup, hit testing
  route.rs        ancestry and propagation/default-action machinery
  keyboard.rs     focused and application-global key/compose/paste routing
  pointer.rs      mouse, capture, click synthesis, wheel/default scroll
  focus.rs        DOM focus only
  terminal.rs     terminal activation/resize translation
```

The split encodes SAF-01/SAF-04: terminal events and DOM focus/key targeting cannot share an accidental helper.

### `elements/input/model.rs` (~1,550 lines)

Proposed private modules:

```text
elements/input/model/
  mod.rs          editor state and atomic edit transaction
  boundary.rs     display/grapheme/word index
  command.rs      command intent and edit outcomes
  selection.rs    selection/caret operations
  constraints.rs  byte/display/max-length checks
  tests.rs        oracle/property edit sequences
```

One `apply(command) -> EditOutcome` should own mutation. Clipboard, submit, invalidation, and observer signals are derived from explicit command intent, addressing SAF-02/03.

### `runtime/commit/text.rs` and `layout.rs`

Commit currently owns measurement, layout, painting, and hook publication across adjacent files. Establish explicit frame-local artifacts:

```text
DomNode -> LayoutTree -> PaintScene + EventRegions
```

Text shaping belongs in the text-layout service, while commit owns cache keys and placement. Editor callbacks receive shared committed layout handles rather than cloned internals.

## Canonical dependency direction

After consolidation, dependencies should follow this direction:

```text
public widgets / ui macro
        |
        v
typed props + events + style + context
        |
        v
lowered immutable DOM
        |
        v
frame-local layout tree
        |
        +--> event-region index
        v
paint scene / typed scene plan
        |
        v
retained renderer --> bounded ANSI/native output
```

State may flow back only through explicit event/hook invalidation queues. Renderer/native code must not depend on widget/component modules. Text shaping can be shared by input and commit, but it must not own focus or component state.

## Safe execution order

1. Add characterization tests for glyph policy, theme presets, editor parity, attr precedence, and constructor equivalence.
2. Remove the isolated `fmt-derive` dependency and unused root clone/fields.
3. Consolidate terminal glyph validation.
4. Consolidate provider construction and runtime wiring.
5. Extract shared editor host mechanics after input correctness fixes land.
6. Convert theme presets to complete data.
7. Split coarse files with behavior-preserving moves.
8. Land algorithmic changes from the performance plan in the newly defined owner modules.
9. Remove compatibility delegates only in the planned API-breaking release.

## Completion checklist

- Each shared invariant named above has one implementation owner.
- Public compatibility aliases delegate and contain no independent logic.
- Every deleted path has a characterization/regression test at the canonical path.
- Module dependencies follow the declared direction without widget/runtime cycles.
- Intentional repetition is explicitly retained where it preserves domain type safety.
- Formatting, all-target clippy, all-target tests, feature-matrix tests, and dependency audit pass.
