# Public API regularization plan

Goal: make ordinary application construction small, predictable, and hard to misuse, while moving raw frame/runtime protocols into an explicitly advanced tier. Compile tests, deprecation checks, and behavior tests provide mechanical gates.

Because the crate is currently `0.1.0`, the cleanest path is a coordinated breaking `0.x` release after compatibility aliases have been exercised internally. Do not promise semver stability before this pass is complete.

## API principles

1. The root contains common application types, not every implementation layer.
2. Public fields cannot be mutated into internally inconsistent states.
3. Infallible methods cannot panic on ordinary caller values.
4. Names reveal domain: cell surfaces and raster pixels are not both “Image.”
5. Focus-target, application-global, and terminal events are separate types.
6. High-level widgets own the interaction semantics implied by their names.
7. Constructors return named handles/results rather than tuples whose positions encode ownership.
8. Configuration is validated before workers start.
9. Low-level operation protocols remain possible, but only in an `advanced` namespace with stronger typed builders.
10. Compatibility aliases delegate to one canonical implementation and have a scheduled removal release.

## API-01 — separate stable high-level and advanced surfaces

Priority: P1

### Current issue

`src/lib.rs:10-45` reexports high-level widgets and data beside `Lower`, `Commit`, `Renderer`, runtime/pipeline traits, raw `Frame`/`Operation`/`ImageId` types, and macro helper names such as `__ui_*`. Theme types are inconsistently available: the module is public but the main types are not aligned with the root/prelude.

This flat surface makes implementation protocols appear equally stable and discoverable as ordinary app construction. It also makes future renderer work a public compatibility event.

### Proposed tree

```rust
icmd::{run, App, Node, Result}

icmd::widgets::{
    button, checkbox, column, input, radio, raster_image,
    row, scroll_area, stack, switch, text, textarea,
}

icmd::style::{
    Attributes, Border, Color, Fill, Length, Overflow, Style,
    TextAlign, TextStyle, WrapMode,
}

icmd::events::{
    EventContext, FocusEvent, KeyEvent, MouseEvent, PasteEvent,
    TerminalFocusEvent,
}

icmd::image::{
    ImageSource, RasterImage, RasterImageError, RasterOptions,
}

icmd::theme::{
    Theme, ThemeColors, ThemeMode, ThemePreset,
}

icmd::advanced::{
    CellSurface, Commit, DomNode, FrameBuilder, Renderer,
    RuntimeBuilder, RuntimeHandle, SceneHandle,
}

#[doc(hidden)]
pub mod __private { /* macro expansion support only */ }
```

The `prelude` should be curated from high-level construction only. It must not wildcard-export the advanced tier. Macro helpers need public reachability for external expansions but can live under a hidden, explicitly unstable `__private` module instead of polluting the root.

### Migration

- First define modules as reexport facades; no implementation moves required.
- Migrate the crate's examples/tests to the new paths so they act as downstream compile coverage.
- Keep selected root aliases for the transition release.
- Move runtime protocol items to `advanced`; remove root aliases only in the breaking release.
- Add a downstream fixture crate that uses only the intended public API, ensuring macros do not depend on accidentally public internals.

### Gate

An API snapshot tool or explicit exported-item allowlist shows no internal helper at root. No high-level compile fixture imports `advanced`.

## API-02 — disambiguate cell surfaces and raster images

Priority: P1

### Current issue

- `Image` represents a terminal-cell surface.
- `RasterImage` represents decoded pixels.
- `image()` constructs the raster widget.

The same word therefore names three different layers. Error messages and methods such as crop/patch/source become hard to predict.

### Canonical names

- Rename cell `Image` to `CellSurface` (or `CellGrid`; choose once before migration).
- Keep `RasterImage` for decoded pixel data.
- Use `raster_image(...)` for the raster widget constructor.
- Use `SurfaceId`/typed handles in the renderer protocol rather than `ImageId` if IDs cover both kinds.

Do not merge the two data types. Their invariants and operations are intentionally different.

### Migration

```rust
#[deprecated(note = "use CellSurface")]
pub type Image = CellSurface;

#[deprecated(note = "use widgets::raster_image")]
pub fn image(/* old signature */) -> Node { raster_image(/* ... */) }
```

Aliases are temporary. Error types and debug output should use canonical terms immediately where compatibility permits. Update advanced `Operation` variants in the breaking release so their names state cell or raster intent.

### Gate

Compile tests prevent importing both old and new paths indefinitely by setting a release deadline. Renderer mismatch errors name actual/expected `SurfaceKind`.

## API-03 — close invariant-bearing image/source structs

Priority: P1

### Current issue

`RasterPlacement` at `src/raster.rs:324` exposes public source, width, height, and options while also offering source accessors and maintaining derived/invalidation state. Callers can mutate fields without updating `full_width` or invalid flags. Zero dimensions behave differently depending on loaded/file construction around `src/raster.rs:335-358`: one path clamps/marks invalid while another can retain zero.

`ImageSource` public enum variants similarly allow bypassing constructor normalization/keying behavior and freeze storage representation into the public contract.

### Proposed API

```rust
#[non_exhaustive]
pub struct RasterPlacement {
    source: ImageSource,
    size: RasterSize,
    options: RasterOptions,
}

impl RasterPlacement {
    pub fn try_new(
        source: impl Into<ImageSource>,
        size: RasterSize,
    ) -> Result<Self, RasterPlacementError>;

    pub fn source(&self) -> &ImageSource;
    pub fn size(&self) -> RasterSize;
    pub fn options(&self) -> &RasterOptions;
    pub fn with_options(self, options: RasterOptions) -> Result<Self, RasterPlacementError>;
}

#[derive(Clone)]
pub struct ImageSource(Arc<ImageSourceInner>);

impl ImageSource {
    pub fn file(path: impl Into<PathBuf>) -> Self;
    pub fn loaded(image: RasterImage) -> Self;
}
```

Choose one zero-size policy. Recommended: placement dimensions must be nonzero and within resource limits; `try_new` rejects zero. If “auto” size is desired, encode it explicitly as `RasterSize::Intrinsic` rather than overloading zero.

### Compatibility caution

Adding `#[non_exhaustive]` or privatizing fields breaks external struct literals. Schedule it for the breaking release; do not pretend it is additive. Before that release, stop using public fields internally and offer accessors/builders.

### Gate

Property tests mutate placements only through public builders and assert derived invariants. There is no public path to a placement accepted by construction but rejected because internal mirrors are stale.

## API-04 — regularize event taxonomy and dispatch outcomes

Priority: P1

### Current issue

- `key_down`/`key_up` coexist with `keyboard_event`, while resize/focus/paste use an `_event` suffix.
- `keyboard_event` behaves as a global/bubbling hook in places where targeted keys use other fields.
- Terminal and DOM focus share `FocusEvent` (SAF-01).
- Only keyboard propagation has an explicit stop mechanism; pointer/wheel cannot consistently prevent propagation or built-in scrolling.
- Dispatcher methods return `usize`, but the count has inconsistent inclusion (for example scroll callbacks differ by route), so callers cannot reliably interpret it.

### Proposed taxonomy

```rust
pub enum EventPhase { Capture, Target, Bubble }

pub struct EventContext<'a> {
    pub phase: EventPhase,
    pub target: ElementRef<'a>,
    pub current_target: ElementRef<'a>,
    // private propagation/default flags
}

impl EventContext<'_> {
    pub fn stop_propagation(&mut self);
    pub fn prevent_default(&mut self);
}

pub struct DispatchOutcome {
    pub delivered: usize,
    pub propagation_stopped: bool,
    pub default_prevented: bool,
    pub redraw_requested: bool,
}
```

Listener categories:

- targeted/bubbling: `on_key_down`, `on_key_up`, `on_paste`, `on_focus`, `on_blur`, pointer events;
- application scope: `on_app_key`, `on_terminal_focus`, `on_resize`;
- internal default actions: focus-on-click, wheel scrolling, text editing, invoked after routing unless prevented.

Use either separate focus/blur listener types or `FocusEvent { kind }`, but never send terminal activation through it. Listener naming should consistently use `on_*`; event values need not repeat `_event` in field names.

### Migration

1. Internally implement a shared dispatch context/outcome and route queue.
2. Add new listener setters/props and have old ones adapt where semantics match exactly.
3. Terminal focus and global keyboard behavior require explicit behavior-change migration; do not delegate them to targeted callbacks.
4. Replace public `usize` returns with `DispatchOutcome`; offer `.delivered` for old call sites.
5. Remove thread-local one-off keyboard propagation after shared RAII routing is proven.

### Gate

An event conformance matrix tests phase order, stop propagation, prevent default, redraw aggregation, focus ownership, removal during callback, and nested dispatch for every event family.

## API-05 — interactive control names must carry complete behavior

Priority: P1

### Current issue

- The button path in `src/elements/basic.rs:97-108` is primarily a themed view and lacks a dedicated `ButtonProps` contract for press, disabled, focus, and keyboard activation.
- Checkbox, radio, and switch primarily render glyph/label state. `disabled` changes presentation, but callers can still attach generic events whose semantics are not suppressed.
- Names imply interactive and accessible controls, but behavior is left piecemeal to callers.

### Decision required

Recommended: make these real controlled interactive widgets.

```rust
pub struct ButtonProps {
    pub on_press: Option<EventListener<PressEvent>>,
    pub disabled: bool,
    pub autofocus: bool,
    // label/content, style overrides
}

pub struct CheckboxProps {
    pub checked: CheckState,
    pub on_change: Option<EventListener<CheckState>>,
    pub disabled: bool,
    // label/content, style overrides
}
```

Required behavior:

- focusable only when policy allows;
- click/tap and Space/Enter activation as appropriate;
- one semantic callback per activation, regardless of physical input;
- disabled suppresses focus/activation/default action, not only color;
- radio supports group/exclusive semantics or is renamed until it does;
- semantic role, label, checked/disabled state are available to a future accessibility/export layer.

Alternative: rename current components to `button_view`, `checkbox_indicator`, etc. That is safer than retaining misleading names, but less useful as a framework.

### Gate

One parameterized control conformance suite covers pointer, keyboard, disabled, focus, duplicate activation prevention, and controlled-state changes.

## API-06 — replace operator-based props mutation with explicit operations

Priority: P2

### Current issue

`Attr` and style values use `/=`, `|`, and `|=` to mean assignment/default/merge behaviors. This is surprising in Rust code, error messages expose arithmetic traits, and string fill assignment can panic (`src/props.rs:635-645`). It also obscures whether false/none clears a default.

### Proposed API

```rust
impl<T> Attr<T> {
    pub fn set(&mut self, value: impl Into<T>);
    pub fn set_default(&mut self, value: impl Into<T>);
    pub fn clear(&mut self);
    pub fn is_explicit(&self) -> bool;
}

impl Style {
    pub fn merge(&mut self, overrides: &Style);
}
```

The UI macro can retain concise surface syntax while expanding to explicit methods. That syntax is a parser/desugaring concern, not a reason to make division operators the public semantic API. Fallible conversions must use `TryFrom` or return a component/build error.

### Focusability merge defect

`DomProps::with_overrides` at `src/props.rs:698-707` uses Boolean OR for `focusable`, so a caller cannot override a default `true` with `false` even though the method says “overrides.” Replace it with tri-state intent:

```rust
pub enum FocusPolicy { Inherit, Focusable, NotFocusable }
```

or `Attr<bool>`. Audit every Boolean/default field for the same inability to clear.

### Gate

Truth-table tests cover unset/default/explicit true/explicit false/clear for every mergeable Boolean or option. No public assignment conversion calls `expect`.

## API-07 — replace tuple constructors with named runtime handles

Priority: P1/P2, coupled to SAF-08

### Current issue

Commit exposes several constructors with different tuple shapes. Runtime start also returns anonymous senders/receivers. Ownership, shutdown, errors, and worker lifetime are encoded by tuple position.

### Proposed API

```rust
pub struct RuntimeBuilder {
    renderer: RendererConfig,
    limits: ResourceLimits,
    event_capacity: usize,
    stage_capacity: usize,
}

impl RuntimeBuilder {
    pub fn new() -> Self;
    pub fn renderer_config(mut self, value: RendererConfig) -> Self;
    pub fn resource_limits(mut self, value: ResourceLimits) -> Self;
    pub fn build(self) -> Result<Runtime, ConfigError>;
}

pub struct RuntimeHandle {
    // private channels and joins
}

impl RuntimeHandle {
    pub fn submit(&self, root: Node) -> Result<(), SubmitError>;
    pub fn try_next(&mut self) -> Result<Option<RuntimeEvent>, RuntimeError>;
    pub fn shutdown(self, policy: ShutdownPolicy) -> Result<(), RuntimeError>;
}
```

High-level `run(node)` or `App::run` can own this internally. Advanced consumers get the handle, not raw tuple wiring.

### Gate

Compile tests show the handle cannot be split into orphaned channels/joins. Lifecycle tests from SAF-08 pass through the public handle.

## API-08 — preserve error sources and encapsulate shared state

Priority: P2

### Current issue

- `RasterImageError::{Decode(String), Io(String)}` loses typed sources and often path context.
- `CanvasError` does not consistently expose underlying sources.
- Public `Ref<T> = Arc<Mutex<T>>` exposes poisoning and the synchronization primitive as permanent API policy.

### Proposed errors

```rust
pub enum RasterImageError {
    Io { path: PathBuf, source: std::io::Error },
    Decode { source: image::ImageError },
    LimitExceeded { resource: ImageResource, limit: u64, requested: u64 },
}
```

If public errors must be `Clone`, store source details in `Arc`; do not erase them to achieve cloning. `Error::source` should work.

### Proposed reference wrapper

```rust
pub struct StateRef<T> { inner: Arc<Mutex<T>> }

impl<T> StateRef<T> {
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R, StateError>;
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, StateError>;
    pub fn try_update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<Option<R>, StateError>;
}
```

This preserves freedom to change lock strategy and centralizes poison handling. Do not expose guards across callbacks; that would recreate reentrancy deadlocks.

### Gate

Every subsystem error retains relevant operation/path/limit context and an error source when one exists. Public callbacks cannot hold framework state locks inadvertently.

## API-09 — make theme derivation semantics explicit

Priority: P2

### Current issue

`Theme::new` derives typography, borders, and scrollbar tokens from colors around `src/theme.rs:592-603`, but `Theme` and its constituent fields are publicly mutable. Mutating `theme.colors` later does not recompute derived styles, leaving an apparently palette-driven theme in a mixed state.

### Options

Recommended: treat `Theme` as a complete set of independent resolved tokens and use a builder for derivation:

```rust
let theme = ThemeBuilder::from_palette(colors)
    .input_style(custom)
    .build();
```

`Theme::from_palette` performs derivation once. Later field mutation is either unavailable (private fields plus `with_*`) or clearly replaces a resolved token. This avoids surprising automatic recomputation that could overwrite customized styles.

Alternative: keep a palette plus override tracking and recompute only untouched derived tokens. This is more complex and should be chosen only if live palette editing is a core use case.

### Gate

Adding a palette/token field breaks complete preset construction at compile time. Theme builder tests establish deterministic precedence among palette-derived defaults and explicit overrides.

## API-10 — regularize text and canvas method naming

Priority: P2

### Text

`Text::style(TextStyle)` and `Text::with_style(Style)` use the same word for different domains. Canonical names:

- `text_style(TextStyle)` for shaping/typography spans;
- `layout_style(Style)` for box/layout/paint style.

Keep old delegates for one transition release.

### Canvas

Methods named `foreground`, `background`, and `attributes` act as setters even though those names normally read state. Use:

- mutating `set_foreground`, `set_background`, `set_attributes`;
- consuming fluent `with_foreground`, etc., only where useful;
- query methods retain noun names.

`CanvasDraw = Fn(...) -> ()` conflicts with drawing operations that return `Result`, making errors awkward to propagate. Choose one coherent model:

1. `CanvasDraw = Fn(&mut CanvasContext) -> Result<(), CanvasError>` and component rendering surfaces the error; or
2. validate glyph/style objects before the callback so bounded drawing primitives are infallible and return a `DrawOutcome` for clipping.

Recommended: use a fallible callback initially. `CanvasContext::set` currently clips out-of-bounds as success; return `DrawOutcome::Clipped` or explicitly choose strict coordinates at builder configuration. Do not call silent clipping an error in one primitive and success in another.

### Gate

Compile and behavior tests establish setter/with/query conventions and uniform clipping/error behavior across set, fill, line, and text.

## API-11 — make IDs and programmatic focus capability-safe

Priority: P1

### Current issue

`DomId(pub u64)` is fabricable by any caller. `EventDispatcher` focus changes can accept an existing region without consistently enforcing focusability around `src/runtime/event.rs:339-343`. A raw numeric ID plus public dispatcher surface creates ambiguous authority.

### Proposed API

- Make `ElementId` fields private; construct IDs only through framework publication/ref handles.
- Return a stable `ElementRef`/`FocusHandle` to code that legitimately needs programmatic focus.
- `try_focus(handle) -> Result<FocusOutcome, FocusError>` checks that the target exists in the current generation and is focusable.
- If force focus is required internally, keep it crate-private and name it explicitly.
- Encode stale-generation errors instead of potentially focusing a reused numeric ID.

### Gate

Callers cannot fabricate an element handle. Tests cover stale handles, removed nodes, disabled/nonfocusable nodes, and focus requested during reconciliation.

## API-12 — validate configuration before runtime creation

Priority: P1

### Current issue

`RuntimeConfig` and `RendererConfig` expose public fields, are extensibility hazards, and permit problematic combinations such as zero polling duration or transform sizes that exceed integer/memory limits.

### Proposed approach

- In the breaking release, use private fields plus builders/getters. `#[non_exhaustive]` alone does not validate mutation and is itself breaking for external literals.
- `build()` returns `ConfigError` before threads, terminal modes, or native resources are created.
- Validation combines renderer settings with `ResourceLimits`, so a cell pixel size that can never fit the transform budget is rejected.
- Provide a safe `Default` with bounded queues, nonzero timing, conservative resource limits, and pure-Rust operation if native raster is feature-gated.

### Gate

Property-generated configurations either construct a runtime satisfying all invariants or return a typed error without side effects/threads.

## API-13 — simplify key and props construction

Priority: P3

### Issues

- `Key::new(impl Into<String>)` allocates a `String`, while `From<&str>` can construct shared storage more directly.
- `Props<T>` offers public `user_defined`, extra accessors, deref, and mapping routes with overlapping meaning.
- `ComponentContext` combines generic custom-hook state and deref behavior in a way that makes the core component contract harder to infer.

### Direction

- Accept `impl Into<Key>` at keyed-node call sites and make `Key::from` canonical; avoid a constructor that forces intermediate `String`.
- Choose one `Props<T>` payload access model as described in DUP-07.
- Keep the standard component context concrete. Put advanced/custom hook storage behind a separate internal extension type or an explicitly advanced generic adapter.

These changes are cleanup, not release blockers; land only after behavior-critical APIs settle.

## API-14 — use typed advanced frame construction

Priority: P1 for advanced consumers

### Current issue

Public `Frame`/`Operation` allow invalid sequences: wrong surface operation, duplicate IDs, use after removal, and inconsistent ordering are discovered only at renderer validation. SAF-06 must strengthen validation regardless, but the API can prevent common mistakes.

### Proposed builder

```rust
let cells = frame.create_cells(surface)?;
frame.patch_cells(&cells, patch)?;

let raster = frame.create_raster(image)?;
frame.set_raster_clip(&raster, clip)?;

frame.place(&cells, position, z)?;
let update = frame.finish()?;
```

`CellSurfaceHandle` and `RasterSurfaceHandle` are distinct. The builder tracks creation/removal within the batch. Long-lived scene handles carry a renderer/scene generation so handles cannot cross runtimes accidentally.

Keep raw operation ingestion crate-private or in a clearly unsafe-by-contract serialization adapter that still validates. Never let typed builders replace transactional renderer validation; callers can deserialize or encounter internal bugs.

### Gate

Compile-fail tests demonstrate that raster operations cannot accept cell handles. Runtime property tests over raw/adversarial operations prove validator transactionality.

## Proposed compatibility table

| Current | Canonical | Transition |
|---|---|---|
| root `Image` | `advanced::CellSurface` | type alias for one release |
| `image()` | `widgets::raster_image()` | deprecated function delegate |
| root renderer/runtime types | `advanced::*` | root reexports removed in breaking release |
| root `__ui_*` | `__private::*` | macro expansion updated atomically |
| `Text::style(TextStyle)` | `Text::text_style` | deprecated delegate |
| `Text::with_style(Style)` | `Text::layout_style` | deprecated delegate |
| canvas `foreground(value)` | `set_foreground(value)` | deprecated delegate |
| `keyboard_event` | `on_app_key` or targeted `on_key_*` | requires semantic classification, no blind alias |
| terminal use of `FocusEvent` | `TerminalFocusEvent` | behavior-breaking separation |
| constructor tuples | `RuntimeBuilder`/`RuntimeHandle` | adapter until breaking release |
| public `RasterPlacement` fields | accessors/builders | fields private in breaking release |
| `Ref<T>` alias | `StateRef<T>` newtype | conversions temporarily available |
| attribute operators | explicit methods | macro desugars; traits deprecated then removed |
| generic `provider` component | `ContextKey::provider` | legacy adapter returns error, then removal |

## API change sequencing

### Phase A — behavior and internals, minimal public removal

- Fix focus taxonomy internally, paste/delete behavior, surface-kind validation, resource limits, panic containment, and runtime ownership.
- Add new event/context/outcome and builder types.
- Add new module facades and canonical names.
- Make all internal/example/test code use canonical APIs.

### Phase B — transition release

- Retain source aliases only where semantics are exact.
- Emit deprecations for mechanical renames.
- For non-equivalent event behavior, require explicit new registration rather than hiding a behavior change behind an alias.
- Publish machine-checkable API snapshots and downstream compile fixtures.

### Phase C — breaking `0.x` release

- Remove root advanced reexports, tuple constructors, operator traits, public invariant-bearing fields, generic provider, and ambiguous names.
- Make IDs opaque and configuration private/validated.
- Move raw protocols to `advanced` and make typed builders canonical.
- Run compatibility fixture updates as a single reviewed migration commit.

## Public API acceptance checklist

- Root and prelude contain no renderer implementation detail or macro helper.
- Every public constructor/method handles ordinary invalid input without panic.
- Every public struct with cross-field invariants has private fields and validated mutation.
- Focus ownership, terminal focus, and application-global keyboard paths use distinct types.
- Controls named as interactive pass the shared activation/disabled/focus suite.
- Public errors retain sources, IDs/paths, operation indexes, and configured/observed limits.
- Runtime ownership and shutdown are represented by named handles.
- Cell surfaces and raster images have unambiguous names and typed operations.
- Downstream compile fixtures cover high-level use, advanced use, macro expansion, and feature-off/native-feature builds.
- Deprecated delegates contain no independent business logic and have a removal release.
