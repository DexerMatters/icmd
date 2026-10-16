# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While the crate is pre-1.0, a minor release may add fields to public structs and
so is a breaking change for exhaustive struct literals.

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

[0.3.0]: https://github.com/DexerMatters/icmd/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/DexerMatters/icmd/releases/tag/v0.2.0
