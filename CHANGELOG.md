# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While the crate is pre-1.0, a minor release may add fields to public structs and
so is a breaking change for exhaustive struct literals.

## [0.4.0] - 2026-10-16

### Added

- `use_live_state` and live `use_state_ref`: shared state whose writes are
  visible immediately and wake the owning component. `StateRef` is now
  cloneable, exposes `set`, `update_quiet`/`try_update_quiet`, and reports
  `same_allocation`.
- `ElementRef::observe` and `ElementObserver`: a committed-snapshot
  subscription used by anchored overlays and available to applications.
- `TextSelectionState` and `TextSelectionChange`, plus `selection` and
  `on_selection_change` on every text control.
- Application-global pointer hooks: `app_pointer`, `app_pointer_capture`, and
  `app_wheel`; `pointer_cancel_capture`; macro attributes for every capture
  slot; chainable `DomProps` setters for every event slot.
- `AppHandle::{request_frame, viewport, focus, try_focus, focused, blur}` and
  `ComponentContext::{request_frame, use_viewport}`.
- `PortalProps`/`portal`, `DialogProps`/`dialog`, `PopoverProps`/`popover`,
  `PopoverPlacement`, `MenuProps`/`menu`, and `MenuItem`.
- `PointerEvents` and `Style::pointer_events`, so a painted host can be
  transparent to pointer hit testing.
- `Dimension::Grow(u16)`: weighted main-axis distribution with largest-remainder
  rounding; `Max` behaves as `Grow(1)`.
- `FrameProps`/`frame`, `PaneProps`/`pane`, `SplitProps`/`split`, and
  `SplitResizeEvent`: weighted panes with draggable and keyboard-adjustable
  splitters.
- `ThemeRoles` and `Theme::roles`: selection, row-alt, header, header-focus,
  chip-on, and overlay role colors derived from the palette.
- `ScrollController` and `ComponentContext::use_scroll_controller`, with
  `scroll_to`, `scroll_by`, `scroll_to_row`, `reveal`, visible/content metrics,
  and scrollbar size. `ScrollbarVisibility::Overlay` paints the bar without
  reserving layout space.
- `ElementScrollState` viewport and scrollbar metrics.
- `display_width`, `display_width_with`, `fit`, and `fit_right`.
- `ComponentContext::use_frame_clock`, `FrameClock`, and frame-clock scheduling
  with pause/resume, intervals, and panic reporting.
- `icmd::testing`: a headless `Harness` with mount/update, painted `Screen`,
  key/mouse/wheel/drag gestures, resize, frame counting, focus, and frame-clock
  ticking. `CellSlot` is exported through `advanced`.
- Braille dot drawing on `CanvasContext`: `set_dot`, `clear_dot`, `set_dots`,
  `set_run`, `dot_width`, `dot_height`, and `cell`.
- `Style::new` and chainable setters for every style field.
- Documented contracts for state snapshots, controlled-input reconciliation,
  `Reset` vs `transparent()`, border/padding order, z-index and hit ordering,
  `prevent_default`, overlay pointer ownership, and keyed identity.

### Changed

- `use_state_ref`-backed state now wakes the runtime; use
  `StateRef::update_quiet` for the old quiet behavior.
- Controlled text inputs no longer revert an optimistic draft when the owner
  re-renders an old value. A rejection is a distinct new publisher value.
- `Theme::from_parts` gained a `roles` argument; `ThemeBuilder` gained
  `roles`.
- New fields on `Style`, `DomProps`, `ScrollAreaProps`, `EventHandlers`, and the
  text-control props; downstream exhaustive struct literals need `..Default`.
- Portals are hoisted to an absolute root so an overlay and the application
  root both occupy the full viewport.

### Fixed

- Text controls insert the character the user typed on terminals that speak the
  kitty keyboard protocol. Shift and Caps Lock now produce uppercase letters
  instead of the unshifted code point the terminal reports, they cancel each
  other as usual, and Shift produces the keyboard layout's symbol. Legacy
  terminals, which bake the shifted glyph into the character, keep working.
- Fast typing against a controlled input no longer loses characters or stalls
  the caret when a stale owner render arrives.
- `pointer_cancel_capture` now observes terminal focus loss and can cancel the
  rest of the pointer delivery.

### Migration

- Replace a parallel `Ref` plus redraw tick with `use_live_state`.
- Use a distinct controlled value to reject a draft, or `use_live_state` for a
  synchronous owner.
- Replace hand-built overlay wrappers with `portal`/`dialog`/`popover`.
- Replace pane cell arithmetic with `Grow`/`split`.
- Import test helpers from `icmd::testing` instead of `__private`.

## [0.3.0] - 2026-10-16

### Added

- `transparent()`: the terminal's own default color, assignable to any style
  color slot. Unlike leaving a slot unset, it is an explicit value that stops
  backdrop inheritance, so it works inside a card that already paints a
  background.
- `BackgroundBlend` and `Style::background_blend`: a transparent background can
  now `Cover` the layers below with the terminal default (the default) or
  `Blend` into them so the lower layer's own style shows through. The resolved
  value is exposed on `ResolvedElementStyle::background_blend`.
- `ThemePreset::Glass`: a ninth preset whose page and overlays are transparent
  over frosted, opaque panes, in both light and dark modes.
- `Glyph`: a validated one-column terminal glyph, shared by scrollbars and
  progress bars. `ScrollbarGlyph` is now an alias of it.
- `GlyphRamp`: an ordered sequence of one-column glyphs describing a progress
  bar's fractional cell from least to most filled.
- `ProgressBarStyle`, `Theme::progress`, and `ThemeBuilder::progress`: progress
  glyphs and cell styles are now theme tokens derived from the palette.
- `ProgressBarProps::filled`, `empty`, and `partial` for per-bar glyph
  overrides.
- `ProgressBarProps::draggable`, `on_drag_start`, `on_drag`, and `on_drag_end`
  plus `ProgressBarDragEvent`: an opt-in pointer drag that reports the proposed
  value to the caller and never changes the rendered value itself.

### Changed

- Progress bars paint full cells, at most one fractional cell chosen from the
  partial ramp, and empty cells, so the painted width is always exactly the
  requested width instead of a rounded fill count.
- `Style` and `ResolvedElementStyle` gained a `background_blend` field.
- `Theme` and `ThemeBuilder` gained a `progress` token, and `Theme::from_parts`
  takes it.
- The interactive field guide (`cargo icmd docs`) documents transparent
  surfaces and blending in a new chapter 08 section 8.7, and now ships nine
  presets.

## [0.2.0] - 2026-09-16

### Added

- Initial public release.
- A retained, terminal-native component tree with `ui!` syntax, typed props,
  context, hooks, and lifecycle phases.
- Cell-accurate rendering: Unicode grapheme and width handling, soft and hard
  wrapping, clipping, focus, key routing, and incremental frame diffing.
- Widgets for layout, text and documents, controls, feedback, scrolling,
  selection, clipboard, canvas drawing, and raster images.
- Element refs that expose committed geometry, resolved style, clipping, and
  scroll state.
- A semantic theme system with named color roles, presets, derived typography,
  spacing, border, scrollbar, and progress tokens, and contrast helpers.
- A pure-Rust default build, with opt-in `native-raster` (Chafa-backed Kitty,
  Sixel, and iTerm2 output) and `markdown` (CommonMark as selectable
  components) features.

[Unreleased]: https://github.com/DexerMatters/icmd/compare/v0.3.0...HEAD
[0.4.0]: https://github.com/DexerMatters/icmd/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/DexerMatters/icmd/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/DexerMatters/icmd/releases/tag/v0.2.0
