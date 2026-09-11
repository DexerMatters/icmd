use std::collections::HashSet;

use crate::{
    Align, Dimension, DomId, DomNode, Edges, Justify, Layout, Overflow, PercentBasis, Style,
    Visibility,
};

use super::Commit;
use super::geometry::{
    RectI, definite_dimension, justify_offset, resolve_dimension, resolve_position,
};
use super::style::{content_insets, slot};
use super::text::text_measure;
use super::types::{ComputedStyle, ComputedText, ScrollSpec};

pub(super) struct ScrollLayout<'a> {
    pub(super) content: RectI,
    pub(super) bar_vertical: bool,
    pub(super) bar_horizontal: bool,
    pub(super) children: Vec<(&'a DomNode, RectI)>,
    pub(super) extent: (i32, i32),
}

impl<'a> ScrollLayout<'a> {
    pub(super) fn vertical_metrics(
        &self,
        offset: super::super::event::RuntimeScrollOffset,
    ) -> Option<ScrollbarMetrics> {
        self.bar_vertical
            .then(|| ScrollbarMetrics::new(self.content.height, self.extent.1, offset.y))?
    }

    pub(super) fn horizontal_metrics(
        &self,
        offset: super::super::event::RuntimeScrollOffset,
    ) -> Option<ScrollbarMetrics> {
        self.bar_horizontal
            .then(|| ScrollbarMetrics::new(self.content.width, self.extent.0, offset.x))?
    }
}

/// Canonical geometry for a scrollbar thumb.  Paint and hit-testing consume
/// the same metrics produced from the layout extent and retained offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarMetrics {
    pub(crate) length: i32,
    pub(crate) thumb_start: i32,
    pub(crate) thumb_len: i32,
    pub(crate) max_offset: i32,
}

impl ScrollbarMetrics {
    pub(crate) fn new(length: i32, content_len: i32, offset: i32) -> Option<Self> {
        if length <= 0 {
            return None;
        }
        let content_len = content_len.max(1);
        let thumb_len = if content_len <= length {
            length
        } else {
            ((length as i64 * length as i64 + content_len as i64 - 1) / content_len as i64)
                .clamp(1, length as i64) as i32
        };
        let max_offset = content_len.saturating_sub(length).max(0);
        let max_start = length.saturating_sub(thumb_len);
        let thumb_start = if max_offset == 0 {
            0
        } else {
            ((offset.clamp(0, max_offset) as i64 * max_start as i64 + max_offset as i64 / 2)
                / max_offset as i64) as i32
        };
        Some(Self {
            length,
            thumb_start,
            thumb_len,
            max_offset,
        })
    }
}

impl Commit {
    pub(super) fn root_rect(&self, root: &DomNode) -> RectI {
        let style = match root {
            DomNode::Element { props, .. } => {
                ComputedStyle::resolve(&props.style, super::style::terminal_text())
            }
            _ => {
                return RectI::new(
                    0,
                    0,
                    self.viewport.width as i32,
                    self.viewport.height as i32,
                );
            }
        };
        let width = match style.width {
            Dimension::Cells(v) => v as i32,
            Dimension::Percent(p) => p.resolve(self.viewport.width as i32).max(0),
            _ => self.viewport.width as i32,
        };
        let height = match style.height {
            Dimension::Cells(v) => v as i32,
            Dimension::Percent(p) => p.resolve(self.viewport.height as i32).max(0),
            _ => self.viewport.height as i32,
        };
        RectI::new(0, 0, width, height)
    }

    pub(super) fn style_of(node: &DomNode) -> Option<&Style> {
        match node {
            DomNode::Element { props, .. } => Some(&props.style),
            DomNode::Text { text, .. } => Some(&text.layout_style),
            _ => None,
        }
    }

    pub(super) fn visibility_of(node: &DomNode) -> Visibility {
        Self::style_of(node)
            .and_then(|style| slot(&style.visibility))
            .unwrap_or_default()
    }

    pub(super) fn intrinsic(
        &self,
        node: &DomNode,
        offered_width: Option<i32>,
        offered_height: Option<i32>,
        inherited: ComputedText,
    ) -> (i32, i32) {
        match node {
            DomNode::Image { image, .. } => (image.width() as i32, image.height() as i32),
            DomNode::Raster { raster, .. } => (raster.width as i32, raster.height as i32),
            DomNode::Text { text, .. } => {
                let style = ComputedStyle::resolve(&text.layout_style, inherited);
                if style.visibility == Visibility::Hidden {
                    return (0, 0);
                }
                let border = style.border.insets();
                let insets = content_insets(border, style.padding);
                let explicit_width = slot(&text.layout_style.width).and_then(|value| {
                    definite_dimension(value, offered_width, self.viewport.width as i32)
                });
                let offered_content_width = explicit_width
                    .or(offered_width)
                    .map(|value| value.saturating_sub(insets.left as i32 + insets.right as i32));
                let text_width = explicit_width
                    .map(|value| value.saturating_sub(insets.left as i32 + insets.right as i32))
                    .or(offered_content_width);
                let (intrinsic_width, intrinsic_height) =
                    text_measure(text, text_width, style.text);
                let intrinsic_outer_width = intrinsic_width
                    .saturating_add(insets.left as i32)
                    .saturating_add(insets.right as i32);
                let intrinsic_outer_height = intrinsic_height
                    .saturating_add(insets.top as i32)
                    .saturating_add(insets.bottom as i32);
                let width = explicit_width.unwrap_or(intrinsic_outer_width);
                let height = slot(&text.layout_style.height)
                    .map(|value| {
                        resolve_dimension(
                            value,
                            offered_height,
                            self.viewport.height as i32,
                            intrinsic_outer_height,
                        )
                    })
                    .unwrap_or(intrinsic_outer_height);
                (
                    width.min(offered_width.unwrap_or(width)).max(0),
                    height.min(offered_height.unwrap_or(height)).max(0),
                )
            }
            DomNode::Element {
                props, children, ..
            } => {
                let style = ComputedStyle::resolve(&props.style, inherited);
                if style.visibility == Visibility::Hidden {
                    return (0, 0);
                }
                let explicit_width = slot(&props.style.width).and_then(|value| {
                    definite_dimension(value, offered_width, self.viewport.width as i32)
                });
                let explicit_height = slot(&props.style.height).and_then(|value| {
                    definite_dimension(value, offered_height, self.viewport.height as i32)
                });
                let border = style.border.insets();
                let inner_w = explicit_width.map(|value| {
                    (value
                        - border.left as i32
                        - border.right as i32
                        - style.padding.left as i32
                        - style.padding.right as i32)
                        .max(0)
                });
                let inner_h = explicit_height.map(|value| {
                    (value
                        - border.top as i32
                        - border.bottom as i32
                        - style.padding.top as i32
                        - style.padding.bottom as i32)
                        .max(0)
                });
                let sizes: Vec<_> = children
                    .iter()
                    .filter_map(|child| {
                        if Self::visibility_of(child) == Visibility::Hidden {
                            return None;
                        }
                        let (width, height) = self.intrinsic(child, inner_w, inner_h, style.text);
                        let margin = Self::style_of(child)
                            .and_then(|style| slot(&style.margin))
                            .unwrap_or_default();
                        Some((
                            width
                                .saturating_add(margin.left as i32)
                                .saturating_add(margin.right as i32),
                            height
                                .saturating_add(margin.top as i32)
                                .saturating_add(margin.bottom as i32),
                        ))
                    })
                    .collect();
                let (mut width, mut height) = match style.layout {
                    Layout::Vertical => (
                        sizes.iter().map(|(w, _)| *w).max().unwrap_or(0),
                        sizes
                            .iter()
                            .fold(0i32, |sum, (_, h)| sum.saturating_add(*h))
                            .saturating_add(
                                (style.gap as i32)
                                    .saturating_mul(sizes.len().saturating_sub(1) as i32),
                            ),
                    ),
                    Layout::Horizontal => (
                        sizes
                            .iter()
                            .fold(0i32, |sum, (w, _)| sum.saturating_add(*w))
                            .saturating_add(
                                (style.gap as i32)
                                    .saturating_mul(sizes.len().saturating_sub(1) as i32),
                            ),
                        sizes.iter().map(|(_, h)| *h).max().unwrap_or(0),
                    ),
                    Layout::Absolute => (
                        sizes.iter().map(|(w, _)| *w).max().unwrap_or(0),
                        sizes.iter().map(|(_, h)| *h).max().unwrap_or(0),
                    ),
                };
                width = width
                    .saturating_add(border.left as i32)
                    .saturating_add(border.right as i32)
                    .saturating_add(style.padding.left as i32)
                    .saturating_add(style.padding.right as i32);
                height = height
                    .saturating_add(border.top as i32)
                    .saturating_add(border.bottom as i32)
                    .saturating_add(style.padding.top as i32)
                    .saturating_add(style.padding.bottom as i32);
                if let Some(value) = explicit_width {
                    width = value;
                }
                if let Some(value) = explicit_height {
                    height = value;
                }
                if let Some(limit) = offered_width {
                    width = width.min(limit.max(0));
                }
                if let Some(limit) = offered_height {
                    height = height.min(limit.max(0));
                }
                (width.max(0), height.max(0))
            }
        }
    }

    pub(super) fn layout_scroll_content<'a>(
        &mut self,
        children: &'a [DomNode],
        base: RectI,
        style: &ComputedStyle,
        spec: Option<&ScrollSpec<'_>>,
    ) -> ScrollLayout<'a> {
        let Some(spec) = spec else {
            let (children, width, height) =
                self.child_layouts(children, base, style, (false, false));
            return ScrollLayout {
                content: base,
                bar_vertical: false,
                bar_horizontal: false,
                children,
                extent: (width, height),
            };
        };

        let vertical_axis = spec.vertical;
        let horizontal_axis = spec.horizontal;
        let bars_allowed = base.width > 0 && base.height > 0;
        let mut bar_vertical =
            bars_allowed && spec.always_vertical && spec.draw_scrollbar && vertical_axis;
        let mut bar_horizontal =
            bars_allowed && spec.always_horizontal && spec.draw_scrollbar && horizontal_axis;

        let mut seen = [false; 4];
        for _ in 0..4 {
            let state = usize::from(bar_vertical) * 2 + usize::from(bar_horizontal);
            if seen[state] {
                break;
            }
            seen[state] = true;
            let content = RectI::new(
                base.line,
                base.column,
                base.width
                    .saturating_sub(i32::from(bar_vertical && base.width > 0)),
                base.height
                    .saturating_sub(i32::from(bar_horizontal && base.height > 0)),
            );
            let (next_children, width, height) =
                self.child_layouts(children, content, style, (horizontal_axis, vertical_axis));
            let next_vertical = spec.draw_scrollbar
                && vertical_axis
                && base.width > 0
                && base.height > 0
                && (spec.always_vertical || height > content.height);
            let next_horizontal = spec.draw_scrollbar
                && horizontal_axis
                && base.width > 0
                && base.height > 0
                && (spec.always_horizontal || width > content.width);
            if next_vertical == bar_vertical && next_horizontal == bar_horizontal {
                return ScrollLayout {
                    content,
                    bar_vertical,
                    bar_horizontal,
                    children: next_children,
                    extent: (width, height),
                };
            }
            let next_state = usize::from(next_vertical) * 2 + usize::from(next_horizontal);
            if seen[next_state] {
                bar_vertical |= next_vertical;
                bar_horizontal |= next_horizontal;
                break;
            }
            bar_vertical = next_vertical;
            bar_horizontal = next_horizontal;
        }

        let content = RectI::new(
            base.line,
            base.column,
            base.width
                .saturating_sub(i32::from(bar_vertical && base.width > 0)),
            base.height
                .saturating_sub(i32::from(bar_horizontal && base.height > 0)),
        );
        let (children_layouts, width, height) =
            self.child_layouts(children, content, style, (horizontal_axis, vertical_axis));
        ScrollLayout {
            content,
            bar_vertical,
            bar_horizontal,
            children: children_layouts,
            extent: (width, height),
        }
    }

    pub(super) fn child_layouts<'a>(
        &self,
        children: &'a [DomNode],
        content: RectI,
        style: &ComputedStyle,
        scroll_axes: (bool, bool),
    ) -> (Vec<(&'a DomNode, RectI)>, i32, i32) {
        if children.is_empty() {
            return (Vec::new(), 0, 0);
        }
        if style.layout == Layout::Absolute {
            let out: Vec<_> = children
                .iter()
                .filter_map(|child| {
                    if Self::visibility_of(child) == Visibility::Hidden {
                        return None;
                    }
                    let (offered_width, offered_height) = intrinsic_offers(content, scroll_axes);
                    let (iw, ih) = self.intrinsic(child, offered_width, offered_height, style.text);
                    let (w, h) = self.child_size(child, iw, ih, content, scroll_axes);
                    let child_style = Self::style_of(child);
                    let margin = child_style
                        .and_then(|style| slot(&style.margin))
                        .unwrap_or_default();
                    let x = resolve_position(
                        child_style
                            .and_then(|style| slot(&style.column))
                            .unwrap_or_default(),
                        content.column,
                        content
                            .width
                            .saturating_sub(margin.left as i32)
                            .saturating_sub(margin.right as i32),
                        self.viewport.width as i32,
                        w,
                    );
                    let y = resolve_position(
                        child_style
                            .and_then(|style| slot(&style.line))
                            .unwrap_or_default(),
                        content.line,
                        content
                            .height
                            .saturating_sub(margin.top as i32)
                            .saturating_sub(margin.bottom as i32),
                        self.viewport.height as i32,
                        h,
                    );
                    Some((
                        child,
                        RectI::new(
                            y.saturating_add(margin.top as i32),
                            x.saturating_add(margin.left as i32),
                            w,
                            h,
                        ),
                    ))
                })
                .collect();
            let extent = out.iter().fold((0, 0), |(width, height), (child, rect)| {
                let margin = Self::style_of(child)
                    .and_then(|style| slot(&style.margin))
                    .unwrap_or_default();
                (
                    width.max(
                        rect.right()
                            .saturating_add(margin.right as i32)
                            .saturating_sub(content.column)
                            .max(0),
                    ),
                    height.max(
                        rect.bottom()
                            .saturating_add(margin.bottom as i32)
                            .saturating_sub(content.line)
                            .max(0),
                    ),
                )
            });
            return (out, extent.0, extent.1);
        }

        let horizontal = style.layout == Layout::Horizontal;
        let available_main = if horizontal {
            content.width
        } else {
            content.height
        };
        let available_cross = if horizontal {
            content.height
        } else {
            content.width
        };
        let viewport_main = if horizontal {
            self.viewport.width as i32
        } else {
            self.viewport.height as i32
        };
        let viewport_cross = if horizontal {
            self.viewport.height as i32
        } else {
            self.viewport.width as i32
        };
        let mut entries = Vec::with_capacity(children.len());
        let mut max_count = 0usize;
        let mut percent_total = 0i64;
        let mut percent_allocated = 0i64;
        for child in children {
            if Self::visibility_of(child) == Visibility::Hidden {
                continue;
            }
            let (offered_width, offered_height) = intrinsic_offers(content, scroll_axes);
            let (iw, ih) = self.intrinsic(child, offered_width, offered_height, style.text);
            let child_style = Self::style_of(child);
            let margin = child_style
                .and_then(|s| slot(&s.margin))
                .unwrap_or_default();
            let main_dimension = child_style
                .and_then(|s| {
                    if horizontal {
                        slot(&s.width)
                    } else {
                        slot(&s.height)
                    }
                })
                .unwrap_or(Dimension::Auto);
            let cross_dimension = child_style
                .and_then(|s| {
                    if horizontal {
                        slot(&s.height)
                    } else {
                        slot(&s.width)
                    }
                })
                .unwrap_or(Dimension::Auto);
            let intrinsic_main = if horizontal { iw } else { ih };
            let intrinsic_cross = if horizontal { ih } else { iw };
            let main_scrolls = if horizontal {
                scroll_axes.0
            } else {
                scroll_axes.1
            };
            let cross_scrolls = if horizontal {
                scroll_axes.1
            } else {
                scroll_axes.0
            };
            let main_available = if main_scrolls {
                None
            } else {
                Some(available_main)
            };
            let cross_available = if cross_scrolls {
                None
            } else {
                Some(available_cross)
            };
            let main = if main_dimension == Dimension::Max && !main_scrolls {
                max_count = max_count.saturating_add(1);
                0
            } else {
                resolve_dimension(
                    main_dimension,
                    main_available,
                    viewport_main,
                    intrinsic_main,
                )
            };
            if let Dimension::Percent(percent) = main_dimension
                && percent.basis() == PercentBasis::Available
                && percent.basis_points() > 0
            {
                percent_total = percent_total.saturating_add(percent.basis_points() as i64);
                percent_allocated = percent_allocated.saturating_add(main as i64);
            }
            let cross_margin = if horizontal {
                margin.top as i32 + margin.bottom as i32
            } else {
                margin.left as i32 + margin.right as i32
            };
            let cross_basis_available =
                cross_available.map(|value| value.saturating_sub(cross_margin).max(0));
            let cross = resolve_dimension(
                cross_dimension,
                cross_basis_available,
                viewport_cross,
                intrinsic_cross,
            );
            entries.push((child, margin, main_dimension, cross_dimension, main, cross));
        }
        let target_percent = ((available_main.max(0) as i64).saturating_mul(percent_total) / 10_000)
            .min(i32::MAX as i64) as i32;
        let mut percent_remainder = (target_percent as i64 - percent_allocated).max(0);
        for entry in &mut entries {
            if percent_remainder == 0 {
                break;
            }
            if matches!(entry.2, Dimension::Percent(percent)
                if percent.basis() == PercentBasis::Available && percent.basis_points() > 0)
            {
                entry.4 = entry.4.saturating_add(1);
                percent_remainder -= 1;
            }
        }
        let mut used = entries
            .iter()
            .map(|(_, margin, _, _, main, _)| {
                let main_margin = if horizontal {
                    margin.left as i32 + margin.right as i32
                } else {
                    margin.top as i32 + margin.bottom as i32
                };
                main.saturating_add(main_margin)
            })
            .fold(0i32, i32::saturating_add);
        used = used.saturating_add(
            (style.gap as i32).saturating_mul(entries.len().saturating_sub(1) as i32),
        );
        let remaining = (available_main - used).max(0);
        let max_share = if max_count > 0 {
            remaining / max_count as i32
        } else {
            0
        };
        let mut remainder = if max_count > 0 {
            remaining % max_count as i32
        } else {
            0
        };
        let mut cursor = if max_count == 0 {
            justify_offset(style.justify, remaining)
        } else {
            0
        };
        let space_between =
            max_count == 0 && style.justify == Justify::SpaceBetween && entries.len() > 1;
        let extra_gap = if space_between {
            remaining / (entries.len() - 1) as i32
        } else {
            0
        };
        let extra_remainder = if space_between {
            remaining % (entries.len() - 1) as i32
        } else {
            0
        };
        let mut out = Vec::with_capacity(entries.len());
        let entry_count = entries.len();
        for (index, (child, margin, main_dimension, cross_dimension, mut main, mut cross)) in
            entries.into_iter().enumerate()
        {
            let main_scrolls = if horizontal {
                scroll_axes.0
            } else {
                scroll_axes.1
            };
            let cross_scrolls = if horizontal {
                scroll_axes.1
            } else {
                scroll_axes.0
            };
            if main_dimension == Dimension::Max && !main_scrolls {
                main = max_share + (remainder > 0) as i32;
                remainder -= (remainder > 0) as i32;
            }
            let cross_is_auto = cross_dimension == Dimension::Auto;
            if cross_is_auto && style.align == Align::Stretch && !cross_scrolls {
                cross = cross_available_for_child(margin, horizontal, available_cross);
            }
            let clips_cross = if horizontal {
                style.overflow_y == Overflow::Clip
            } else {
                style.overflow_x == Overflow::Clip
            };
            if clips_cross && !cross_scrolls {
                cross = cross.min(cross_available_for_child(
                    margin,
                    horizontal,
                    available_cross,
                ));
            }
            let cross_margin_start = if horizontal {
                margin.top as i32
            } else {
                margin.left as i32
            };
            let cross_basis = cross_available_for_child(margin, horizontal, available_cross);
            let cross_offset = match style.align {
                Align::Center => (cross_basis - cross).max(0) / 2,
                Align::End => (cross_basis - cross).max(0),
                _ => 0,
            };
            if horizontal {
                let line = content
                    .line
                    .saturating_add(cross_margin_start)
                    .saturating_add(cross_offset);
                let column = content
                    .column
                    .saturating_add(cursor)
                    .saturating_add(margin.left as i32);
                out.push((child, RectI::new(line, column, main, cross)));
                if index + 1 < entry_count {
                    let distributed = extra_gap
                        + if (index as i32) < extra_remainder {
                            1
                        } else {
                            0
                        };
                    cursor = cursor
                        .saturating_add(main)
                        .saturating_add(margin.left as i32)
                        .saturating_add(margin.right as i32)
                        .saturating_add(style.gap as i32)
                        .saturating_add(distributed);
                }
            } else {
                let line = content
                    .line
                    .saturating_add(cursor)
                    .saturating_add(margin.top as i32);
                let column = content
                    .column
                    .saturating_add(cross_margin_start)
                    .saturating_add(cross_offset);
                out.push((child, RectI::new(line, column, cross, main)));
                if index + 1 < entry_count {
                    let distributed = extra_gap
                        + if (index as i32) < extra_remainder {
                            1
                        } else {
                            0
                        };
                    cursor = cursor
                        .saturating_add(main)
                        .saturating_add(margin.top as i32)
                        .saturating_add(margin.bottom as i32)
                        .saturating_add(style.gap as i32)
                        .saturating_add(distributed);
                }
            }
        }
        let extent = out.iter().fold((0, 0), |(width, height), (child, rect)| {
            let margin = Self::style_of(child)
                .and_then(|style| slot(&style.margin))
                .unwrap_or_default();
            (
                width.max(
                    rect.right()
                        .saturating_add(margin.right as i32)
                        .saturating_sub(content.column)
                        .max(0),
                ),
                height.max(
                    rect.bottom()
                        .saturating_add(margin.bottom as i32)
                        .saturating_sub(content.line)
                        .max(0),
                ),
            )
        });
        (out, extent.0, extent.1)
    }

    pub(super) fn child_size(
        &self,
        child: &DomNode,
        iw: i32,
        ih: i32,
        available: RectI,
        scroll_axes: (bool, bool),
    ) -> (i32, i32) {
        let Some(style) = Self::style_of(child) else {
            return (iw.max(0), ih.max(0));
        };
        let width = resolve_dimension(
            slot(&style.width).unwrap_or(Dimension::Auto),
            if scroll_axes.0 {
                None
            } else {
                Some(available.width)
            },
            self.viewport.width as i32,
            iw,
        );
        let height = resolve_dimension(
            slot(&style.height).unwrap_or(Dimension::Auto),
            if scroll_axes.1 {
                None
            } else {
                Some(available.height)
            },
            self.viewport.height as i32,
            ih,
        );
        (width.max(0), height.max(0))
    }
}

pub(super) fn collect_scroll_ids(node: &DomNode, ids: &mut HashSet<DomId>) {
    if let DomNode::Element {
        id,
        props,
        children,
    } = node
    {
        if props.scroll.is_some() {
            ids.insert(*id);
        }
        for child in children {
            collect_scroll_ids(child, ids);
        }
    }
}

pub(super) fn cross_available_for_child(
    margin: Edges<u16>,
    horizontal: bool,
    available: i32,
) -> i32 {
    let margins = if horizontal {
        margin.top as i32 + margin.bottom as i32
    } else {
        margin.left as i32 + margin.right as i32
    };
    available.saturating_sub(margins).max(0)
}

pub(super) fn intrinsic_offers(
    content: RectI,
    scroll_axes: (bool, bool),
) -> (Option<i32>, Option<i32>) {
    (
        (!scroll_axes.0).then_some(content.width),
        (!scroll_axes.1).then_some(content.height),
    )
}
