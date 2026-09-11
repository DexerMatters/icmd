# Raw input, input, and textarea redesign plan

**Status:** proposed  
**Date:** 2026-09-11  
**Repository:** `/home/dexer/repos/rust_projects/icmd`

## 1. Outcome

Replace the current monolithic text editor with one extensible component named
`raw_input`, and implement the public `input` and `textarea` components as thin
specializations of that primitive.

The finished design must have these properties:

1. `raw_input` is the only owner of text-entry behavior. A custom component can
   wrap it, forward ordinary `DomProps`, and change its styling without copying
   editing code.
2. `input` selects single-line policy and applies the application's themed input
   appearance. `textarea` selects multiline policy and applies the themed
   textarea appearance. Neither contains an editor state machine.
3. Text measurement, wrapping, cursor placement, vertical navigation, pointer
   hit-testing, selection painting, and scroll extents use one canonical layout
   result. The element layer must not approximate or reproduce the commit
   layer's line breaking.
4. Controlled and uncontrolled values follow an explicit state machine. There
   are no value-comparison heuristics for guessing whether a render is an echo,
   acceptance, rejection, or external replacement.
5. The component has one semantic host for layout, focus, scrolling, and event
   forwarding. Caller events coexist with internal behavior on that host.
6. Unicode correctness is defined in terms of UTF-8 source boundaries,
   extended grapheme clusters, and terminal cells. These coordinate systems
   are never used interchangeably.
7. The normal component conventions remain authoritative: logical components
   return nodes, `Props<T>` carry semantic props plus `DomProps`, theme supplies
   component defaults, and caller style overrides those defaults field by
   field.

This is a clean architectural replacement, not a second editor beside the
current one and not a compatibility project. Remove superseded names, props,
handler types, and runtime paths in the same change that moves the public
components to `raw_input`.

## 2. Why the current shape should be replaced

The current tests are a useful behavior baseline (all 27 tests in
`tests/text_edit.rs` pass as of this plan), but the implementation is too
coupled to be a stable foundation:

- `src/elements/text_edit.rs` combines normalization, editing commands,
  selection, controlled-value reconciliation, Unicode cell geometry, wrapping,
  painting, measurement feedback, scrolling, focus, pointer capture, theme
  chrome, and the two public components in one file.
- `visual_rows` and its helpers reproduce wrapping decisions that also exist in
  `src/runtime/commit/text.rs`. Small differences between those algorithms
  become visible cursor, click, wrapping, and scroll bugs.
- `Text::on_measure_width` sends paint-time width back into component state and
  requests another render. Correctness therefore depends on a feedback render
  settling, and ordinary `Text` carries a private exception for one component.
- `InputProps::width` and `TextAreaProps::{width,height}` define one geometry
  API while `DomProps.style` defines another. `editor_dom` then forcibly
  restores selected internal style fields after merging caller props. This is
  surprising in a system whose normal rule is `Props::host_props` with caller
  overrides.
- The outer view, inner event view, hidden `scroll_area`, and text surface split
  focus and event identity across multiple DOM nodes. Caller events are moved
  to a different host to avoid replacing internal handlers.
- `EditorState` mixes durable document state with focus, drag state, speculative
  controlled edits, viewport offsets, cached layout, and a render-feedback
  flag. This makes unrelated transitions affect one another.
- `TextEditHandler<T>` duplicates the existing generic `EventListener<T>`
  callback abstraction.

These are structural failure modes. Adding more branches or more regression
tests to the same component can hide individual symptoms, but it will not
establish a single source of truth.

## 3. Architectural boundary

The dependency direction should be:

```text
custom field ─┐
input ────────┼──> raw_input ──> edit model ──> editor surface
textarea ─────┘         │                              │
                        └── one host / scroll state    v
                                           canonical text layout
                                          /       |          \
                                   measurement  painting  hit/navigation
```

There are four distinct responsibilities.

### 3.1 Public components

- `raw_input` exposes text-entry semantics and ordinary host composition.
- `input` and `textarea` choose policy and theme defaults, then render
  `raw_input`.
- User-defined fields extend `raw_input` through an ordinary function
  component. They do not subclass it or copy a private helper.

### 3.2 Pure edit model

A model/reducer owns the normalized value and selection. It accepts typed edit
actions and returns an outcome; it knows nothing about `Node`, `Theme`,
`EventDispatcher`, scrolling, or callbacks.

### 3.3 Editor surface

A crate-private declarative surface describes the value, placeholder,
selection, caret, wrap policy, and visual decoration for the current render. It
contains no mutable editor behavior. It is allowed to be a dedicated internal
node/DOM variant if that is the cleanest way to keep editor-only annotations
out of ordinary `Text`.

### 3.4 Canonical text layout

One pure layout module converts styled source graphemes into visual rows at a
given cell width. Both the commit pipeline and editor interactions consume its
result. No wrapping or byte-to-cell implementation remains in
`elements/input`.

## 4. Public API

Use Rust's existing naming style for types and the requested component names:

```rust
pub enum RawInputMode {
    SingleLine,
    Multiline,
}

pub struct RawInputProps {
    pub mode: Attr<RawInputMode>,
    pub value: Attr<String>,
    pub default_value: Attr<String>,
    pub placeholder: Attr<String>,
    pub wrap: Attr<TextWrap>,
    pub max_length: Attr<usize>,
    pub disabled: Attr<bool>,
    pub read_only: Attr<bool>,
    pub appearance: Attr<RawInputAppearance>,
    pub on_change: Attr<EventListener<TextValueEvent>>,
    pub on_submit: Attr<EventListener<TextValueEvent>>,
    pub on_clipboard: Attr<EventListener<TextClipboardEvent>>,
}

pub fn raw_input(cx: &mut ComponentContext, props: &Props<RawInputProps>) -> Node;
pub fn input(cx: &mut ComponentContext, props: &Props<InputProps>) -> Node;
pub fn textarea(cx: &mut ComponentContext, props: &Props<TextareaProps>) -> Node;
```

The exact fields of `RawInputAppearance` should be limited to editor-specific
decoration that ordinary host `Style` cannot express: placeholder, active and
inactive selection, caret, and optional focused-border styling. Its defaults
should be terminal-native (inherit text colors, dim placeholder, reverse caret
and selection), so `raw_input` does not require branded theme chrome. `input`
and `textarea` translate the current `Theme` into an appearance and host style.

API rules:

- `RawInputProps` contains behavior, not width or height. Layout has one public
  owner: `props.dom.style`.
- `InputProps` exposes common value/editing fields plus `on_submit`; it does not
  expose multiline or wrap switches.
- `TextareaProps` exposes common fields plus `wrap`; Enter inserts a newline and
  there is no ambiguous default submit gesture.
- `value` means controlled. `default_value` is read exactly once when
  uncontrolled. Supplying both is permitted but `value` wins, matching the
  current API.
- `read_only` remains focusable and selectable and permits copy, but does not
  mutate. `disabled` is not focusable and does not participate in editing or
  selection.
- `max_length` counts extended grapheme clusters after input normalization and
  after replacing the current selection.
- Single-line normalization maps CR, LF, and tab to spaces and discards other
  control characters. Multiline normalization canonicalizes CRLF/CR to LF,
  preserves newline and tab, and discards other control characters.
- The standard DOM `on_focus_event` is the canonical focus observer. Do not add
  another permanent focus callback specific to inputs.
- Arbitrary children are not an extension mechanism for `raw_input`: they
  cannot be mapped safely to source positions. Document that children are
  ignored (or reject them in debug builds). Extension means wrapping the
  primitive and forwarding props, style, and events.

A custom component should be possible without private APIs:

```rust
fn command_field(cx: &mut ComponentContext, props: &Props<CommandFieldProps>) -> Node {
    let theme = cx.use_theme();
    raw_input
        .props(RawInputProps {
            mode: Attr::Set(RawInputMode::SingleLine),
            value: props.value.clone(),
            on_change: props.on_change.clone(),
            ..RawInputProps::default()
        })
        .style(move |style| {
            style.width /= Dimension::Max;
            style.border.foreground /= theme.colors.accent;
        })
        .node()
}
```

The final documentation example should also demonstrate that a caller-supplied
pointer, focus, or keyboard observer still runs alongside the raw editor's
internal handler.

## 5. The canonical layout result

Create a crate-private layout module below `basic` rather than below `elements`
or `runtime`, because both component behavior and commit need it without an
upward dependency.

Its immutable result should retain, for every displayed grapheme:

- the UTF-8 source byte range;
- the terminal-cell start and width;
- the visual row;
- the effective text style; and
- whether source bytes following it are an explicit newline or a separator
  omitted at a soft-wrap boundary.

It should expose operations rather than its storage layout:

- intrinsic width and visual row count;
- row widths and scroll extent;
- source boundary to caret rectangle;
- visual cell to source boundary with an explicit leading/trailing hit bias;
- source boundary to visual row and cell;
- vertical movement using a preferred terminal-cell column; and
- iteration for rasterization, including selection and caret decoration.

The implementation must establish the following invariants:

1. Every returned source position is a valid extended-grapheme boundary.
2. Every normalized source byte belongs to exactly one painted grapheme,
   explicit line break, or deliberately hidden wrap separator.
3. A pointer hit and a caret position use the same row table and the same tab
   stops.
4. Wide graphemes use terminal cells, including a documented half-cell rule for
   pointer hits; a width-two grapheme in a one-cell viewport is handled without
   an invalid continuation cell.
5. Soft and hard wrapping, trailing whitespace, empty lines, final newlines,
   tabs, combining sequences, and ZWJ emoji have one interpretation in measure,
   paint, hit-test, and navigation.
6. Zero-sized offered geometry is clamped safely at the boundary and cannot
   panic or create an unbounded loop.

Refactor `src/runtime/commit/text.rs` to construct and consume this result for
both measurement and rasterization. Delete the separate editor row builder
once parity tests pass.

The editor surface should publish its last committed layout into a passive,
crate-private layout probe shared with `raw_input`. Pointer handlers then read
the exact layout that produced the visible frame. Painting must already be
correct at the parent's final content width; the probe must not request a
second render merely to make wrapping correct. A guarded wake is acceptable
only when a changed viewport requires a focused caret's scroll offset to be
reconciled, and must be covered by a test proving that it settles after one
correction.

This replaces `Text::on_measure_width`; ordinary text should no longer contain
an editor-specific paint-to-render feedback callback.

## 6. Edit model and controlled-value contract

Split mutable state into explicit domains:

```text
EditBuffer
  normalized value
  anchor and cursor source boundaries
  preferred terminal-cell column

ValueOwnership
  uncontrolled, or controlled with render revision
  optional draft produced since that render
  last emitted draft and its selection snapshot

InteractionState
  focused / dragging
  viewport offset
  reveal-caret request
```

Use an `EditAction` enum for insert, paste, backward/forward delete, movement,
line/document edges, select-all, pointer placement, and drag extension. The
reducer returns an `EditOutcome` containing `handled`, an optional changed
value, optional clipboard/submit output, and whether the caret must be
revealed. Event adapters invoke callbacks only after releasing the model lock.

Controlled behavior must be revision based:

1. At component render, normalize the current `value` and record a monotonically
   increasing render revision.
2. All input events dispatched before the next render reduce sequentially
   against one draft. This preserves rapid/repeated keystrokes without waiting
   for the owner.
3. At the next render, the supplied `value` is authoritative. If it equals the
   emitted draft, restore the draft's selection snapshot. Otherwise treat it as
   rejection or external replacement and clamp selection to that value.
4. Never infer causality by comparing a value with multiple historical strings.
   The render revision says whether a draft was produced from this render.
5. Switching from uncontrolled to controlled adopts `value`. Switching back
   retains the last authoritative rendered value and then resumes local
   ownership. Cover both transitions explicitly.

This contract intentionally gives the owner final authority at every render;
optimistic drafts only bridge multiple events within one render interval. State
this in the public documentation so rejection is predictable.

## 7. One host, focus, events, and scrolling

`raw_input` should render one scroll-capable host containing the editor surface.
That host receives caller `DomProps` and is the focus target. Do not recreate
the current outer-event-host/inner-editor-host arrangement.

Required supporting changes:

- Add an event-listener composition helper so internal handlers and caller
  handlers occupy one event slot. Internal state reduction runs first and the
  caller observer runs afterward, outside the model lock. `stop_propagation`
  prevents ancestor delivery, not the caller's observer on the same host.
- Stop propagation only for keys the editor handles. Unrecognized shortcuts and
  navigation that does not belong to the current mode continue to ancestors.
- Give `DomProps`/event regions an explicit `focusable` property. Its default is
  false; controls opt in, and `raw_input` sets it true when enabled and false
  when disabled. Update other built-in controls that need keyboard focus in the
  same cutover. Do not infer focus behavior from which event callbacks happen
  to be installed.
- Keep `scroll_area` as the scrolling mechanism; do not build a second scroll
  engine in the editor. `raw_input` may control its offset to reveal the caret
  and receives `on_scroll` updates, while the runtime remains the authority for
  clipping, extents, wheel behavior, and pointer coordinates.
- Single-line raw input enables horizontal scrolling. Multiline input enables
  vertical scrolling and horizontal scrolling only when `NoWrap` or a terminal
  width edge case creates real horizontal extent. Scrollbars remain hidden by
  default but the raw primitive's appearance can opt into them later without
  changing the edit model.
- Pointer coordinates are viewport/content-box coordinates supplied by the
  event system. Add the current scroll offset once, then resolve through the
  committed layout probe. Padding and borders must never be manually subtracted
  by the editor.
- Runtime focus events are the source of truth for focused presentation. The
  component caches focus only to render a caret; it does not invent a second
  independent focus lifecycle.

## 8. Styled wrappers

`input` and `textarea` should have no hooks and no handlers of their own. Each
wrapper should:

1. Read `Theme`.
2. Build its default host style and `RawInputAppearance`.
3. Merge caller `DomProps` with `Props::host_props`, with caller fields winning.
4. Translate its semantic props into `RawInputProps`.
5. Return `raw_input.apply(...)` while preserving the caller DOM events.

Use border-box dimensions in `Style` consistently. The default visual sizes may
match today's controls, but the meaning must no longer switch between a
`width` prop as content width and `style.width` as host width. Raw input may
enforce semantic facts such as clipping its scrolling axes, but it must not
silently restore caller-overridden layout, padding, alignment, or dimensions.

Theme responsibilities remain where the rest of the element library puts them:

- `input`: input background/text colors, horizontal padding, and the existing
  underline treatment;
- `textarea`: input background/text colors, padding, and full themed border;
- focused border/selection, placeholder, caret, disabled, and read-only visual
  states derive from the current theme through `RawInputAppearance`.

## 9. Implementation sequence

### Phase 0: Characterize behavior and expose missing failures

- Preserve all existing worktree changes; do not reset or rewrite unrelated
  files.
- Classify the 27 current text-edit tests as semantic invariants or artifacts of
  the discarded API/DOM shape. Preserve the semantic cases as black-box
  characterization tests and delete the accidental structural assertions.
- Add focused failing tests for the reported bugs before changing behavior.
- Add a small test-only interaction driver so tests can render, dispatch an
  event, and drain rerenders without repeating timing code.
- Record intentional behavior for key propagation, disabled/read-only focus,
  controlled rejection, mode transitions, style sizing, and event ordering.

Gate: the new tests reproduce the known failures and the existing suite remains
a trustworthy baseline.

### Phase 1: Establish one layout engine

- Introduce the canonical text layout result and unit tests.
- Convert commit measurement and painting to it without touching public input
  APIs.
- Add the declarative editor surface and layout probe.
- Prove ordinary `Text` output remains unchanged with rendering snapshots and
  existing hard/soft-wrap tests.

Gate: measure and paint use the same row objects; ordinary text and all
non-input tests pass.

### Phase 2: Replace the mutable editor core

- Implement the pure edit reducer and the revision-based value ownership state
  machine.
- Port key, paste, pointer, selection, clipboard, and max-length behavior as
  model tests before wiring UI events.
- Keep byte offsets private to the model and expose only validated boundaries.

Gate: reducer tests cover every current editing behavior without constructing a
runtime or renderer.

### Phase 3: Build `raw_input`

- Add the single-host component, event composition, explicit focusability, and
  scroll integration.
- Render only the declarative editor surface.
- Verify that final committed layout, hit-testing, navigation, painting, and
  cursor reveal all consume the canonical layout.
- Add a custom wrapper test demonstrating style and event extension.

Gate: `raw_input` independently passes the interaction matrix and does not use
`Theme` for branded chrome.

### Phase 4: Rebuild `input` and `textarea`

- Make both wrappers translate props and theme into `raw_input`.
- Port examples and tests to `textarea` and style-based sizing.
- Confirm the wrappers contain no edit state, geometry helpers, or internal
  event handling.

Gate: all public input behavior flows through `raw_input`; visual defaults and
theme variants are covered.

### Phase 5: Public cutover, cleanup, and documentation

- Export `RawInputMode`, `RawInputAppearance`, `RawInputProps`, `raw_input`,
  `InputProps`, `input`, `TextareaProps`, and `textarea` from `elements`, crate
  root, and the prelude.
- Remove `text_area`; `textarea` is the only multiline component name.
- Remove component-specific `width`/`height` props. Convert all examples and
  tests to `style.width`/`style.height`, with border-box sizing as the only
  documented model. Do not add dimensions to `RawInputProps`.
- Remove `TextEditHandler<T>` and use `EventListener<T>` directly.
- Remove `on_focus_change`; use `DomProps.events.focus_event` and the `ui!`
  `on_focus_event` attribute everywhere.
- Delete the old editor implementation, duplicate wrap helpers, measurement
  feedback, and obsolete tests that only inspect the discarded DOM shape.
- Document controlled ownership, normalization, Unicode/cell semantics,
  extension, event propagation, styling precedence, and unsupported features.

Gate: no old API name, compatibility field, or legacy runtime path remains.

## 10. Verification matrix

### Pure model

- insert, replace selection, backspace, delete, Home/End, document edges,
  preferred-column vertical movement, select-all, copy, cut, paste, and submit;
- ASCII, CJK, combining marks, emoji modifiers, ZWJ emoji, tabs, CRLF, empty
  strings, explicit empty lines, and final newlines;
- max length across graphemes that merge at either insertion boundary;
- controlled acceptance, rejection, external replacement, rapid events before
  render, unrelated rerender, and both ownership-mode transitions;
- callback reentrancy and poison-free lock boundaries.

### Layout properties

- all source positions are grapheme boundaries;
- `hit_test(caret_point(boundary))` round-trips for every boundary where the
  visual position is unambiguous;
- all displayed cells are reachable by pointer hit-testing;
- measure and paint have identical extents for widths 0, 1, narrow, exact, and
  wider than content;
- `NoWrap`, `Soft`, and `Hard` agree across ordinary text and editor surfaces;
- dropped wrap separators and source newlines retain navigable source ranges;
- selection and caret never style only half of a wide glyph.

Use table tests plus bounded property-style generation for random Unicode
grapheme sequences and viewport widths. Do not rely only on ANSI string
snapshots for geometry assertions.

### Integrated behavior

- pointer placement with borders, padding, horizontal/vertical offsets, parent
  clipping, and a parent-constrained width;
- drag selection with capture, release, cancellation, and edge scrolling;
- resize while focused and unfocused, including proof that any cursor-reveal
  correction settles rather than causing an endless render loop;
- wheel scrolling is not immediately snapped back unless an edit/navigation
  action explicitly requests caret reveal;
- disabled never receives focus; read-only can focus/select/copy; focus is lost
  exactly once when an enabled focused input becomes disabled;
- handled keys stop at the input, unhandled keys bubble, and caller observers on
  the raw host still run;
- nested scroll areas route remaining wheel delta normally;
- controlled input does not drop held-key repeats;
- input remains one logical line; textarea wraps to its actual granted content
  width and every painted row maps back to source.

### Public composition

- `ui!` accepts `raw_input`, `input`, and `textarea` attributes with inferred
  closure event types;
- `.style`, `.events`, `.props`, and `.extra` remain composable;
- style width/height are the only sizing inputs;
- a user component can wrap `raw_input` with a custom theme and events without
  accessing crate-private state;
- crate-root and prelude exports compile, while removed names fail compile-time
  UI tests with ordinary unresolved-item/unknown-field diagnostics.

### Repository gates

Run at minimum:

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
git diff --check
```

For timing-sensitive runtime tests, use bounded `recv_timeout` calls and drain
known convergence renders. Do not make correctness depend on sleeps.

## 11. File-level target shape

The exact module names may be adjusted to match the implementation, but the
responsibilities should end near this shape:

```text
src/basic/text_layout.rs       canonical glyph/row/source layout
src/basic/editor_surface.rs    crate-private declarative editor paint surface
src/elements/input/mod.rs      public props, events, raw/styled components
src/elements/input/model.rs    pure edit reducer and value ownership
src/elements/input/view.rs     event adaptation, one host, scroll/focus glue
src/runtime/commit/text.rs     measure/rasterize canonical layouts
src/basic/events.rs            listener composition and focusability support
tests/input_model.rs           reducer and ownership matrix
tests/input_layout.rs          layout invariants/properties
tests/input.rs                 end-to-end raw/input/textarea behavior
tests/ui/*                     compile-time public API coverage
examples/demo.rs               styled controls plus a custom raw-input wrapper
```

Avoid excessive fragmentation: if `model.rs` or `view.rs` remains small, keep it
in `input/mod.rs`. The important boundary is that the model and layout engine
are independently testable and the themed wrappers stay declarative.

## 12. Risks and explicit non-goals

- Adding a crate-private editor surface touches `NodeKind`, `DomNode`, lowering,
  layout, paint, and caches. Land it as a visual-only vertical slice before
  connecting mutable input behavior, and exhaustively match the new variant.
- Event listener composition changes ordering semantics. Limit the initial
  helper to component-internal plus caller handlers, document the ordering, and
  do not silently change `DomProps::with_overrides` for every existing
  component.
- Explicit focusability changes a framework-wide inference. Audit every
  built-in interactive component and add direct tests for controls, scroll
  areas, and non-focusable views in the same change.
- Width/height migration is observably different because style dimensions are
  border-box dimensions. Choose coherent theme defaults and update snapshots;
  do not distort the new sizing model to reproduce contradictory old inputs.
- A display transform such as password masking is not safe unless it provides a
  reversible source-to-display grapheme map. Do not accept an arbitrary render
  callback in the first version. It can be added later as a constrained mapping
  over the canonical layout.
- Undo/redo history, validation, labels, forms, IME composition, terminal
  clipboard ownership, and platform-specific shortcut remapping are out of
  scope. The model should leave room for them without claiming support.
- Programmatic focus, selection refs, and imperative scroll controllers are out
  of scope unless an existing application use case requires them during the
  migration.

## 13. Definition of done

The redesign is complete only when:

1. A public custom component demonstrably extends `raw_input` through normal
   composition.
2. `input` and `textarea` are visibly thin `raw_input` specializations.
3. There is one editor state machine, one callback abstraction, one focus host,
   one scroll mechanism, and one text layout algorithm.
4. Parent constraints and terminal resizes cannot put painted rows, the caret,
   pointer hits, and scroll extents into different coordinate spaces.
5. Controlled acceptance and rejection are deterministic and documented.
6. Caller styles and events follow the same precedence/composition rules as the
   rest of the application.
7. The full verification matrix passes, the legacy editor code is gone, and no
   settling loop is required for correct text wrapping.
