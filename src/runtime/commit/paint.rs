//! Paint pass that turns a laid-out DOM tree into ordered paint fragments for a frame.
//! Owns traversal, clipping, backgrounds, borders, scrollbars, rasters, and selection tiling.

use std::collections::HashMap;

use crossterm::style::Color;

use crate::basic::element_ref::{
    ElementRect, ElementScrollState, ElementSnapshot, ResolvedBorderStyle, ResolvedElementStyle,
    ResolvedTextStyle,
};
use crate::{
    BorderKind, Cell, DomId, DomNode, Fill, Image, Overflow, Rect, ScreenPosition,
    basic::{ScrollbarGlyph, ScrollbarStyle},
};

use super::super::event::{
    EventRect, EventRegion, RuntimeScrollOffset, ScrollRegion, scrollbar_region,
};
use super::geometry::RectI;
use super::layout::ScrollbarMetrics;
use super::style::{content_insets, scroll_spec};
use super::text::merge_text;
use super::types::{
    BorderPainter, Clip, ComputedBorder, ComputedText, PaintContent, PaintFragment, PaintKey,
    PaintRole,
};
use super::{Commit, ElementBinding};

pub(super) struct SelectionPaint<'a> {
    config: &'a crate::basic::selection::SelectionConfig,
    builder: crate::basic::selection::DocumentBuilder,
    origin: ScreenPosition,
    width: usize,
    height: usize,
}

impl SelectionPaint<'_> {
    fn publish(self) {
        let Self {
            config,
            builder,
            origin,
            width,
            height,
        } = self;
        config
            .probe
            .publish(crate::basic::selection::CommittedSelection {
                origin,
                width,
                height,
                document: builder.finish(),
            });
    }
}

impl Commit {
    /// Paints one lowered node into the scene.
    ///
    /// A selectable ancestor reserves the plain-text path for its own document,
    /// so a text node carrying an editor surface is painted as a raster even
    /// while a selection host is active. An editor owns its own selection, and
    /// skipping it along with the document text would leave an input inside a
    /// selection area painting its box and nothing else.
    ///
    /// Children of a scroll region are painted with that region's own offset
    /// rather than the sum of every ancestor's. The offset reaches an editor as
    /// the shift applied to its text, and its pointer mapping adds it to a
    /// position that is already relative to the box the scroller sits in;
    /// accumulating ancestors would add a page offset the editor's coordinates
    /// never contained, sending a click past the end of the value.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_node<'a>(
        &mut self,
        node: &'a DomNode,
        rect: RectI,
        inherited: ComputedText,
        backdrop: Option<Color>,
        level: i32,
        clip: Clip,
        order: &mut u64,
        scene: &mut HashMap<PaintKey, PaintFragment>,
        event_regions: &mut Vec<EventRegion>,
        element_bindings: &mut Vec<(DomId, ElementBinding)>,
        measurement_paths: &std::collections::HashSet<DomId>,
        parent: Option<DomId>,
        event_order: &mut u64,
        scroll: (i32, i32),
        mut selection: Option<&mut SelectionPaint<'a>>,
    ) {
        self.instrument.placement();
        let measuring = measurement_paths.contains(&node.id());
        if clip.is_empty() && !measuring {
            if let DomNode::Raster { id, raster } = node
                && raster.loading == crate::ImageLoading::Eager
            {
                Self::insert_raster(
                    scene,
                    PaintKey {
                        node: *id,
                        role: PaintRole::Content,
                    },
                    raster.clone(),
                    rect,
                    level,
                    *order,
                    clip,
                );
                *order = (*order).saturating_add(1);
            }
            return;
        }
        match node {
            DomNode::Image { id, image } => {
                Self::insert(
                    scene,
                    PaintKey {
                        node: *id,
                        role: PaintRole::Content,
                    },
                    image.clone(),
                    rect,
                    level,
                    *order,
                    clip,
                );
                *order = (*order).saturating_add(1);
            }
            DomNode::Raster { id, raster } => {
                Self::insert_raster(
                    scene,
                    PaintKey {
                        node: *id,
                        role: PaintRole::Content,
                    },
                    raster.clone(),
                    rect,
                    level,
                    *order,
                    clip,
                );
                *order = (*order).saturating_add(1);
            }
            DomNode::Text { id, text } => {
                let style = super::types::ComputedStyle::resolve(&text.layout_style, inherited);
                if style.visibility != crate::Visibility::Visible {
                    return;
                }
                let current_backdrop = style.background.or(backdrop);
                let paint_level = level.saturating_add(style.z_index);
                if style.visibility == crate::Visibility::Visible {
                    Self::paint_background(
                        scene,
                        *id,
                        style.background,
                        style.fill.clone(),
                        style.text,
                        backdrop,
                        rect,
                        paint_level,
                        clip,
                        order,
                    );
                    Self::paint_border(
                        scene,
                        *id,
                        style.border,
                        style.background.or(backdrop),
                        rect,
                        paint_level,
                        clip,
                        order,
                    );
                }
                let content = rect.inset(content_insets(style.border.insets(), style.padding));
                let content_visible = clip.intersection(content);
                let selection = selection.filter(|_| text.editor.is_none());
                if let Some(selection) = selection {
                    let geometry = self.text_geometry_cached(*id, text, style.text, content);
                    let origin = ScreenPosition::new(content.line, content.column);
                    let doc_start = selection.builder.push(
                        crate::basic::selection::Run::new(
                            geometry.value.clone(),
                            geometry.layout.clone(),
                            origin,
                            content.width.max(0) as usize,
                            content.height.max(1) as usize,
                        )
                        .with_align(text.align)
                        .with_visible(content_visible.is_some()),
                    );
                    let overlay = selection
                        .config
                        .selection
                        .local_range(doc_start, geometry.value.len())
                        .map(|range| crate::basic::selection::SelectionOverlay {
                            range,
                            focused: selection.config.focused,
                            styles: selection.config.styles.clone(),
                        });
                    if let Some(content_visible) = content_visible
                        && let Some(image) = self.selectable_text_cached(
                            *id,
                            text,
                            &geometry,
                            content,
                            content_visible,
                            style.text,
                            current_backdrop.unwrap_or(Color::Reset),
                            scroll,
                            overlay,
                        )
                    {
                        Self::insert(
                            scene,
                            PaintKey {
                                node: *id,
                                role: PaintRole::Content,
                            },
                            image,
                            content_visible,
                            paint_level,
                            *order,
                            clip,
                        );
                        *order = (*order).saturating_add(1);
                    }
                } else if let Some(content_visible) = content_visible
                    && let Some(image) = self.raster_text_cached(
                        *id,
                        text,
                        content,
                        content_visible,
                        style.text,
                        current_backdrop.unwrap_or(Color::Reset),
                        scroll,
                    )
                {
                    Self::insert(
                        scene,
                        PaintKey {
                            node: *id,
                            role: PaintRole::Content,
                        },
                        image,
                        content_visible,
                        paint_level,
                        *order,
                        clip,
                    );
                    *order = (*order).saturating_add(1);
                }
            }
            DomNode::Element {
                id,
                props,
                children,
            } => {
                let style = super::types::ComputedStyle::resolve(&props.style, inherited);
                if style.visibility == crate::Visibility::Hidden {
                    return;
                }
                let current_backdrop = style.background.or(backdrop);
                let paint_level = level.saturating_add(style.z_index);

                if style.visibility == crate::Visibility::Visible {
                    Self::paint_background(
                        scene,
                        *id,
                        style.background,
                        style.fill.clone(),
                        style.text,
                        backdrop,
                        rect,
                        paint_level,
                        clip,
                        order,
                    );
                    Self::paint_border(
                        scene,
                        *id,
                        style.border,
                        style.background.or(backdrop),
                        rect,
                        paint_level,
                        clip,
                        order,
                    );
                }
                let inner = rect.inset(style.border.insets());
                let base_content = inner.inset(style.padding);
                let spec = scroll_spec(props.scroll.as_deref());
                let scroll_layout =
                    self.layout_scroll_content(children, base_content, &style, spec.as_ref());
                let content = scroll_layout.content;
                let max_x =
                    if spec.as_ref().is_some_and(|spec| spec.horizontal) && content.width > 0 {
                        scroll_layout.extent.0.saturating_sub(content.width).max(0)
                    } else {
                        0
                    };
                let max_y = if spec.as_ref().is_some_and(|spec| spec.vertical) && content.height > 0
                {
                    scroll_layout.extent.1.saturating_sub(content.height).max(0)
                } else {
                    0
                };
                let offset = if spec.is_some() {
                    let mut offsets = self.scroll_offsets.lock().expect("scroll mutex poisoned");
                    let offset = offsets.entry(*id).or_default();
                    if let Some(requested) = spec.as_ref().and_then(|spec| spec.requested_offset) {
                        offset.x = i32::try_from(requested.x).unwrap_or(i32::MAX);
                        offset.y = i32::try_from(requested.y).unwrap_or(i32::MAX);
                    }
                    offset.x = offset.x.clamp(0, max_x);
                    offset.y = offset.y.clamp(0, max_y);
                    *offset
                } else {
                    RuntimeScrollOffset::default()
                };

                if measurement_paths.contains(id)
                    && (props.element_ref.is_set() || props.element_change.is_set())
                {
                    let visible_rect = (style.visibility == crate::Visibility::Visible)
                        .then(|| clip.intersection(rect))
                        .flatten()
                        .map(ElementRect::from_rect);
                    let resolved_style = ResolvedElementStyle {
                        layout: style.layout,
                        margin: style.margin,
                        padding: style.padding,
                        gap: style.gap,
                        justify: style.justify,
                        align: style.align,
                        overflow: style.overflow,
                        overflow_x: style.overflow_x,
                        overflow_y: style.overflow_y,
                        visibility: style.visibility,
                        z_index: style.z_index,
                        background: style.background,
                        fill: style.fill.clone(),
                        border: ResolvedBorderStyle {
                            kind: style.border.kind,
                            edges: style.border.edges,
                            foreground: style.border.foreground,
                            background: style.border.background,
                            attributes: style.border.attributes,
                        },
                        text: ResolvedTextStyle {
                            foreground: style.text.foreground,
                            background: style.text.background,
                            attributes: style.text.attributes,
                        },
                    };
                    let axes = match (
                        spec.as_ref().is_some_and(|value| value.horizontal),
                        spec.as_ref().is_some_and(|value| value.vertical),
                    ) {
                        (true, true) => crate::ScrollAxes::Both,
                        (true, false) => crate::ScrollAxes::Horizontal,
                        _ => crate::ScrollAxes::Vertical,
                    };
                    let scroll = spec.as_ref().map(|_| ElementScrollState {
                        axes,
                        offset: crate::ScrollOffset::new(
                            u32::try_from(offset.x.max(0)).unwrap_or(u32::MAX),
                            u32::try_from(offset.y.max(0)).unwrap_or(u32::MAX),
                        ),
                        max_offset: crate::ScrollOffset::new(
                            u32::try_from(max_x.max(0)).unwrap_or(u32::MAX),
                            u32::try_from(max_y.max(0)).unwrap_or(u32::MAX),
                        ),
                        content_width: u32::try_from(scroll_layout.extent.0.max(0))
                            .unwrap_or(u32::MAX),
                        content_height: u32::try_from(scroll_layout.extent.1.max(0))
                            .unwrap_or(u32::MAX),
                    });
                    let snapshot = ElementSnapshot::new(
                        *id,
                        ElementRect::from_rect(rect),
                        ElementRect::from_rect(content),
                        visible_rect,
                        resolved_style,
                        scroll,
                    );
                    element_bindings.push((
                        *id,
                        ElementBinding {
                            element_ref: props.element_ref.as_ref().cloned(),
                            listener: props.element_change.as_ref().cloned(),
                            snapshot,
                        },
                    ));
                }
                let vertical_metrics = scroll_layout.vertical_metrics(offset);
                let horizontal_metrics = scroll_layout.horizontal_metrics(offset);
                let scroll_region = spec.as_ref().map(|spec| ScrollRegion {
                    max_x,
                    max_y,
                    viewport_height: content.height,
                    horizontal: spec.horizontal,
                    vertical: spec.vertical,
                    wheel_step: spec.wheel_step,
                    wheel: spec.wheel,
                    enable_mouse: spec.enable_mouse,
                    enable_keyboard: spec.enable_keyboard,
                    controlled: spec.controlled,
                    vertical_bar: vertical_metrics.map(|metrics| {
                        scrollbar_region(content.line, content.right(), metrics, true)
                    }),
                    horizontal_bar: horizontal_metrics.map(|metrics| {
                        scrollbar_region(content.bottom(), content.column, metrics, false)
                    }),
                });

                if style.visibility == crate::Visibility::Visible
                    && *id != DomId::root()
                    && let Some(hit_rect) = clip.intersection(rect)
                    && hit_rect.width > 0
                    && hit_rect.height > 0
                {
                    let (origin_line, origin_column) = (base_content.line, base_content.column);

                    event_regions.push(EventRegion {
                        id: *id,
                        parent,
                        focusable: props.focusable | false,
                        autofocus: props.autofocus,
                        rect: EventRect::new(
                            hit_rect.line,
                            hit_rect.column,
                            hit_rect.width,
                            hit_rect.height,
                        ),
                        origin: ScreenPosition::new(origin_line, origin_column),
                        level: paint_level,
                        order: *event_order,
                        handlers: props.events.clone(),
                        scroll: scroll_region,
                    });
                    *event_order = (*event_order).saturating_add(1);
                }

                let child_clip = if style.visibility != crate::Visibility::Visible {
                    Clip::Empty
                } else {
                    match spec {
                        Some(ref spec) => clip.restrict_axes(
                            content,
                            spec.horizontal || style.overflow_x == Overflow::Clip,
                            spec.vertical || style.overflow_y == Overflow::Clip,
                        ),
                        None => clip.restrict_axes(
                            inner,
                            style.overflow_x == Overflow::Clip,
                            style.overflow_y == Overflow::Clip,
                        ),
                    }
                };
                let mut host = props.selection.as_ref().map(|config| SelectionPaint {
                    config,
                    builder: crate::basic::selection::DocumentBuilder::new(self.emoji_merging)
                        .with_viewport_height(content.height.max(0) as usize),
                    origin: ScreenPosition::new(base_content.line, base_content.column),
                    width: content.width.max(0) as usize,
                    height: content.height.max(0) as usize,
                });
                for (child, mut child_rect) in scroll_layout.children {
                    let child_scroll = if spec.is_some() {
                        child_rect.line = child_rect.line.saturating_sub(offset.y);
                        child_rect.column = child_rect.column.saturating_sub(offset.x);
                        (offset.y, offset.x)
                    } else {
                        scroll
                    };
                    let active = match host.as_mut() {
                        Some(host) => Some(host),
                        None => selection.as_deref_mut(),
                    };
                    self.paint_node(
                        child,
                        child_rect,
                        style.text,
                        current_backdrop,
                        paint_level,
                        child_clip,
                        order,
                        scene,
                        event_regions,
                        element_bindings,
                        measurement_paths,
                        Some(*id),
                        event_order,
                        child_scroll,
                        active,
                    );
                }
                if let Some(host) = host {
                    host.publish();
                }
                if let Some(spec) = spec {
                    self.paint_scrollbars(
                        scene,
                        *id,
                        content,
                        vertical_metrics,
                        horizontal_metrics,
                        spec.scrollbar,
                        style.text,
                        current_backdrop,
                        paint_level,
                        clip.restrict(base_content),
                        order,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_border(
        scene: &mut HashMap<PaintKey, PaintFragment>,
        node: DomId,
        border: ComputedBorder,
        backdrop: Option<Color>,
        rect: RectI,
        level: i32,
        clip: Clip,
        order: &mut u64,
    ) {
        let Some(kind) = border.kind else {
            return;
        };
        if rect.width <= 0 || rect.height <= 0 || !border.visible() {
            return;
        }
        let glyphs = border_glyphs(kind);
        let background = border.background.or(backdrop).unwrap_or(Color::Reset);
        let cell = |symbol: &'static str| {
            Cell::styled(border.foreground, background, border.attributes, symbol)
                .expect("border glyph must be a valid terminal cell")
        };
        let top = border.edges.top;
        let right = border.edges.right;
        let bottom = border.edges.bottom;
        let left = border.edges.left;
        let mut painter = BorderPainter {
            scene,
            node,
            level,
            clip,
            order,
        };

        if top {
            let edge = RectI::new(rect.line, rect.column, rect.width, 1);
            if let Some(visible) = clip.intersection(edge) {
                insert_border(
                    &mut painter,
                    PaintRole::BorderTop,
                    border_row_segment(
                        glyphs,
                        &cell,
                        rect.width,
                        true,
                        left,
                        right,
                        visible.column.saturating_sub(rect.column),
                        visible.width,
                    ),
                    visible,
                );
            }
        } else if rect.height == 1 && bottom {
            let edge = RectI::new(rect.line, rect.column, rect.width, 1);
            if let Some(visible) = clip.intersection(edge) {
                insert_border(
                    &mut painter,
                    PaintRole::BorderBottom,
                    border_row_segment(
                        glyphs,
                        &cell,
                        rect.width,
                        false,
                        left,
                        right,
                        visible.column.saturating_sub(rect.column),
                        visible.width,
                    ),
                    visible,
                );
            }
        }

        if bottom && rect.height > 1 {
            let edge = RectI::new(rect.bottom().saturating_sub(1), rect.column, rect.width, 1);
            if let Some(visible) = clip.intersection(edge) {
                insert_border(
                    &mut painter,
                    PaintRole::BorderBottom,
                    border_row_segment(
                        glyphs,
                        &cell,
                        rect.width,
                        false,
                        left,
                        right,
                        visible.column.saturating_sub(rect.column),
                        visible.width,
                    ),
                    visible,
                );
            }
        }

        if left {
            let start = if top { 1 } else { 0 };
            let end = rect.height - if bottom { 1 } else { 0 };
            if end > start {
                let edge = RectI::new(
                    rect.line.saturating_add(start),
                    rect.column,
                    1,
                    end.saturating_sub(start),
                );
                if let Some(visible) = clip.intersection(edge) {
                    insert_border(
                        &mut painter,
                        PaintRole::BorderLeft,
                        border_column_segment(&cell, glyphs.vertical, visible.height),
                        visible,
                    );
                }
            }
        }

        if right && (rect.width > 1 || !left) {
            let start = if top { 1 } else { 0 };
            let end = rect.height - if bottom { 1 } else { 0 };
            if end > start {
                let edge = RectI::new(
                    rect.line.saturating_add(start),
                    rect.right().saturating_sub(1),
                    1,
                    end.saturating_sub(start),
                );
                if let Some(visible) = clip.intersection(edge) {
                    insert_border(
                        &mut painter,
                        PaintRole::BorderRight,
                        border_column_segment(&cell, glyphs.vertical, visible.height),
                        visible,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments, clippy::collapsible_if)]
    pub(super) fn paint_scrollbars(
        &self,
        scene: &mut HashMap<PaintKey, PaintFragment>,
        node: DomId,
        content: RectI,
        vertical_metrics: Option<ScrollbarMetrics>,
        horizontal_metrics: Option<ScrollbarMetrics>,
        scrollbar: &ScrollbarStyle,
        inherited: ComputedText,
        backdrop: Option<Color>,
        level: i32,
        clip: Clip,
        order: &mut u64,
    ) {
        let background = backdrop.unwrap_or(Color::Reset);
        if let Some(metrics) = vertical_metrics {
            if let Some(image) = scrollbar_image(
                metrics,
                &scrollbar.vertical_track,
                &scrollbar.vertical_thumb,
                merge_text(inherited, &scrollbar.track),
                merge_text(inherited, &scrollbar.thumb),
                background,
                true,
            ) {
                Self::insert(
                    scene,
                    PaintKey {
                        node,
                        role: PaintRole::ScrollbarVertical,
                    },
                    image,
                    RectI::new(content.line, content.right(), 1, content.height),
                    level.saturating_add(1),
                    *order,
                    clip,
                );
                *order = (*order).saturating_add(1);
            }
        }
        if let Some(metrics) = horizontal_metrics {
            if let Some(image) = scrollbar_image(
                metrics,
                &scrollbar.horizontal_track,
                &scrollbar.horizontal_thumb,
                merge_text(inherited, &scrollbar.track),
                merge_text(inherited, &scrollbar.thumb),
                background,
                false,
            ) {
                Self::insert(
                    scene,
                    PaintKey {
                        node,
                        role: PaintRole::ScrollbarHorizontal,
                    },
                    image,
                    RectI::new(content.bottom(), content.column, content.width, 1),
                    level.saturating_add(1),
                    *order,
                    clip,
                );
                *order = (*order).saturating_add(1);
            }
        }
        if vertical_metrics.is_some() && horizontal_metrics.is_some() {
            let cell = Cell::styled(inherited.foreground, background, inherited.attributes, " ")
                .expect("scrollbar corner is a valid cell");
            if let Ok(image) = Image::from_rows(vec![vec![cell]]) {
                Self::insert(
                    scene,
                    PaintKey {
                        node,
                        role: PaintRole::ScrollbarCorner,
                    },
                    image,
                    RectI::new(content.bottom(), content.right(), 1, 1),
                    level.saturating_add(1),
                    *order,
                    clip,
                );
                *order = (*order).saturating_add(1);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_background(
        scene: &mut HashMap<PaintKey, PaintFragment>,
        node: DomId,
        color: Option<Color>,
        fill: Option<Fill>,
        text: ComputedText,
        backdrop: Option<Color>,
        rect: RectI,
        level: i32,
        clip: Clip,
        order: &mut u64,
    ) {
        if color.is_none() && fill.is_none() {
            return;
        }
        let Some(visible) = clip.intersection(rect) else {
            return;
        };
        let Some(image) = background_image(
            color.unwrap_or_else(|| backdrop.unwrap_or(Color::Reset)),
            fill.as_ref(),
            text,
            rect,
            visible,
        ) else {
            return;
        };
        Self::insert(
            scene,
            PaintKey {
                node,
                role: PaintRole::Background,
            },
            image,
            visible,
            level,
            *order,
            clip,
        );
        *order = (*order).saturating_add(1);
    }

    pub(super) fn insert(
        scene: &mut HashMap<PaintKey, PaintFragment>,
        key: PaintKey,
        mut image: Image,
        rect: RectI,
        level: i32,
        order: u64,
        clip: Clip,
    ) {
        if rect.width <= 0 || rect.height <= 0 {
            return;
        }
        let source_rect = rect;
        let Some(rect) = clip.intersection(rect) else {
            return;
        };
        let image_rect = RectI::new(
            source_rect.line,
            source_rect.column,
            image.width() as i32,
            image.height() as i32,
        );
        let Some(rect) = rect.intersection(image_rect) else {
            return;
        };
        if rect.width != image.width() as i32 || rect.height != image.height() as i32 {
            let crop = Rect::new(
                (rect.line - source_rect.line).max(0) as usize,
                (rect.column - source_rect.column).max(0) as usize,
                rect.width as usize,
                rect.height as usize,
            );
            let Ok(value) = image.crop(crop) else { return };
            image = value;
        }
        scene.insert(
            key,
            PaintFragment {
                content: PaintContent::Cells(image),
                position: ScreenPosition::new(rect.line, rect.column),
                raster_clip: None,
                level,
                order,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_raster(
        scene: &mut HashMap<PaintKey, PaintFragment>,
        key: PaintKey,
        raster: crate::RasterPlacement,
        rect: RectI,
        level: i32,
        order: u64,
        clip: Clip,
    ) {
        let visible = clip.intersection(rect);
        let load_clip = if raster.loading == crate::ImageLoading::Eager {
            Some(rect)
        } else if raster.source.loaded_image().is_none() {
            clip.prefetch().intersection(rect)
        } else {
            visible
        };
        let Some(load_clip) = load_clip else {
            return;
        };
        if load_clip.width <= 0 || load_clip.height <= 0 {
            return;
        }
        let raster_clip = visible.map_or_else(
            || crate::Rect::new(0, 0, 0, 0),
            |visible| {
                crate::Rect::new(
                    visible.line.saturating_sub(rect.line).max(0) as usize,
                    visible.column.saturating_sub(rect.column).max(0) as usize,
                    visible.width as usize,
                    visible.height as usize,
                )
            },
        );
        scene.insert(
            key,
            PaintFragment {
                content: PaintContent::Raster(raster),
                position: ScreenPosition::new(rect.line, rect.column),
                raster_clip: Some(raster_clip),
                level,
                order,
            },
        );
    }
}

fn insert_border(painter: &mut BorderPainter<'_>, role: PaintRole, image: Image, rect: RectI) {
    Commit::insert(
        painter.scene,
        PaintKey {
            node: painter.node,
            role,
        },
        image,
        rect,
        painter.level,
        *painter.order,
        painter.clip,
    );
    *painter.order = (*painter.order).saturating_add(1);
}

#[allow(clippy::too_many_arguments)]
fn border_row_segment(
    glyphs: BorderGlyphs,
    cell: &impl Fn(&'static str) -> Cell,
    width: i32,
    top: bool,
    left: bool,
    right: bool,
    start: i32,
    visible_width: i32,
) -> Image {
    let row = (start..start.saturating_add(visible_width))
        .map(|column| {
            let symbol = if column == 0 && left {
                corner_glyph(glyphs, top, true, left)
            } else if column == width - 1 && right {
                corner_glyph(glyphs, top, false, right)
            } else {
                glyphs.horizontal
            };
            cell(symbol)
        })
        .collect();
    Image::from_rows(vec![row]).expect("border row must be valid")
}

fn border_column_segment(
    cell: &impl Fn(&'static str) -> Cell,
    symbol: &'static str,
    height: i32,
) -> Image {
    let rows = (0..height).map(|_| vec![cell(symbol)]).collect();
    Image::from_rows(rows).expect("border column must be valid")
}

fn background_image(
    color: Color,
    fill: Option<&Fill>,
    text: ComputedText,
    source: RectI,
    visible: RectI,
) -> Option<Image> {
    if visible.width <= 0 || visible.height <= 0 {
        return None;
    }
    let fill_symbol = fill.map_or(" ", Fill::symbol);
    let fill_width = fill.map_or(1, Fill::width);
    let fill_cell = Cell::styled(text.foreground, color, text.attributes, fill_symbol).ok()?;
    if fill_width == 1 {
        return Image::new(visible.width as usize, visible.height as usize, fill_cell).ok();
    }

    let blank = Cell::styled(text.foreground, color, text.attributes, " ").ok()?;
    let mut rows = Vec::with_capacity(visible.height as usize);
    let start = (visible.column - source.column).max(0) as usize;
    for _ in 0..visible.height {
        let mut row = Vec::with_capacity(visible.width as usize);
        let mut column = 0usize;
        while column < visible.width as usize {
            let source_column = start + column;
            if source_column.is_multiple_of(fill_width)
                && column + fill_width <= visible.width as usize
            {
                row.push(fill_cell.clone());
                column += fill_width;
            } else {
                row.push(blank.clone());
                column += 1;
            }
        }
        rows.push(row);
    }
    Image::from_rows(rows).ok()
}

#[derive(Clone, Copy)]
struct BorderGlyphs {
    horizontal: &'static str,
    vertical: &'static str,
    top_left: &'static str,
    top_right: &'static str,
    bottom_left: &'static str,
    bottom_right: &'static str,
}

fn border_glyphs(kind: BorderKind) -> BorderGlyphs {
    match kind {
        BorderKind::Single => BorderGlyphs {
            horizontal: "─",
            vertical: "│",
            top_left: "┌",
            top_right: "┐",
            bottom_left: "└",
            bottom_right: "┘",
        },
        BorderKind::Rounded => BorderGlyphs {
            horizontal: "─",
            vertical: "│",
            top_left: "╭",
            top_right: "╮",
            bottom_left: "╰",
            bottom_right: "╯",
        },
        BorderKind::Double => BorderGlyphs {
            horizontal: "═",
            vertical: "║",
            top_left: "╔",
            top_right: "╗",
            bottom_left: "╚",
            bottom_right: "╝",
        },
        BorderKind::Heavy => BorderGlyphs {
            horizontal: "━",
            vertical: "┃",
            top_left: "┏",
            top_right: "┓",
            bottom_left: "┗",
            bottom_right: "┛",
        },
    }
}

fn corner_glyph(
    glyphs: BorderGlyphs,
    top: bool,
    left_corner: bool,
    vertical: bool,
) -> &'static str {
    if !vertical {
        return glyphs.horizontal;
    }
    if !top {
        return if left_corner {
            glyphs.bottom_left
        } else {
            glyphs.bottom_right
        };
    }
    if left_corner {
        glyphs.top_left
    } else {
        glyphs.top_right
    }
}

#[allow(clippy::too_many_arguments)]
fn scrollbar_image(
    metrics: ScrollbarMetrics,
    track: &ScrollbarGlyph,
    thumb: &ScrollbarGlyph,
    track_style: ComputedText,
    thumb_style: ComputedText,
    background: Color,
    vertical: bool,
) -> Option<Image> {
    let length = metrics.length as usize;
    let thumb_len = metrics.thumb_len as usize;
    let thumb_start = metrics.thumb_start as usize;
    let make_cell = |symbol: &str, style: ComputedText| {
        Cell::styled(
            style.foreground,
            style.background.unwrap_or(background),
            style.attributes,
            symbol,
        )
    };
    let track_symbol = track.symbol();
    let thumb_symbol = thumb.symbol();
    if vertical {
        let rows = (0..length)
            .map(|index| {
                let in_thumb = (thumb_start..thumb_start + thumb_len).contains(&index);
                make_cell(
                    if in_thumb { thumb_symbol } else { track_symbol },
                    if in_thumb { thumb_style } else { track_style },
                )
                .ok()
                .map(|cell| vec![cell])
            })
            .collect::<Option<Vec<_>>>()?;
        Image::from_rows(rows).ok()
    } else {
        let row = (0..length)
            .map(|index| {
                let in_thumb = (thumb_start..thumb_start + thumb_len).contains(&index);
                make_cell(
                    if in_thumb { thumb_symbol } else { track_symbol },
                    if in_thumb { thumb_style } else { track_style },
                )
                .ok()
            })
            .collect::<Option<Vec<_>>>()?;
        Image::from_rows(vec![row]).ok()
    }
}
