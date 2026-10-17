//! Scroll container that clips its content and owns a scroll offset.

use std::ops::Range;

use crate::{
    Attr, ComponentContext, DomProps, ElementRef, ElementScrollState, EventListener, Node,
    Overflow, Props, ScrollAxes, ScrollEvent, ScrollOffset, ScrollbarStyle, ScrollbarVisibility,
    Size, StateRef, basic::props::ScrollConfig, ui, view,
};

/// Configuration for [`scroll_area`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScrollAreaProps {
    /// Axes that scroll and clip; defaults to [`ScrollAxes::Vertical`].
    pub axes: Attr<ScrollAxes>,
    /// When the scrollbar is shown; defaults to
    /// [`ScrollbarVisibility::Auto`].
    pub scrollbar_visibility: Attr<ScrollbarVisibility>,
    /// Controlled scroll offset in cells; when unset the runtime owns it.
    pub offset: Attr<ScrollOffset>,
    /// Whether mouse dragging scrolls; defaults to `true`.
    pub enable_mouse: Attr<bool>,
    /// Whether the wheel scrolls; defaults to `true`.
    pub enable_wheel: Attr<bool>,
    /// Whether keyboard scrolling is handled; defaults to `true`.
    pub enable_keyboard: Attr<bool>,
    /// Cells moved per wheel notch; defaults to `1` and is floored at `1`.
    pub wheel_step: Attr<u16>,
    /// Scrollbar glyph style; defaults to the theme's scrollbar.
    pub scrollbar_style: Attr<ScrollbarStyle>,
    /// Controller owning the offset. When set, the runtime treats the offset
    /// as controlled and the controller receives reveal helpers and metrics.
    ///
    /// The controller attaches its own element ref to the host. If the caller
    /// also supplies `element_ref` on this control, the caller's ref wins and
    /// the controller cannot read content or viewport metrics; use
    /// `on_element_change` in that case. An explicit `offset` also wins over
    /// the controller.
    pub controller: Attr<ScrollController>,
}

/// Shared, observable scroll state for one [`scroll_area`].
///
/// The controller owns a live offset and learns content and viewport metrics
/// from the host's committed element snapshot. Updates wake the component that
/// called [`ComponentContext::use_scroll_controller`].
#[derive(Clone)]
pub struct ScrollController {
    offset: StateRef<ScrollOffset>,
    element: ElementRef,
}

impl std::fmt::Debug for ScrollController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollController")
            .field("offset", &self.offset())
            .finish_non_exhaustive()
    }
}

impl PartialEq for ScrollController {
    fn eq(&self, other: &Self) -> bool {
        self.offset.same_allocation(&other.offset)
    }
}

impl Eq for ScrollController {}

impl ScrollController {
    /// Current offset in cells.
    pub fn offset(&self) -> ScrollOffset {
        self.offset
            .read(|offset| *offset)
            .unwrap_or_else(|_| ScrollOffset::new(0, 0))
    }

    /// Largest offset the host allows.
    pub fn max_offset(&self) -> ScrollOffset {
        self.state().map_or_else(
            || ScrollOffset::new(0, 0),
            |state| state.max_scroll_offset(),
        )
    }

    /// Visible child area after border, padding, and reserved scrollbar cells.
    pub fn viewport_size(&self) -> Size {
        self.state().map_or_else(
            || Size::new(0, 0),
            |state| {
                Size::new(
                    u16::try_from(state.viewport_width).unwrap_or(u16::MAX),
                    u16::try_from(state.viewport_height).unwrap_or(u16::MAX),
                )
            },
        )
    }

    /// Logical content extent.
    pub fn content_size(&self) -> Size {
        self.state().map_or_else(
            || Size::new(0, 0),
            |state| {
                Size::new(
                    u16::try_from(state.content_width).unwrap_or(u16::MAX),
                    u16::try_from(state.content_height).unwrap_or(u16::MAX),
                )
            },
        )
    }

    /// Columns occupied by the vertical scrollbar, reserved or overlaid.
    pub fn scrollbar_width(&self) -> u32 {
        self.state().map_or(0, |state| state.scrollbar_width)
    }

    /// Rows occupied by the horizontal scrollbar, reserved or overlaid.
    pub fn scrollbar_height(&self) -> u32 {
        self.state().map_or(0, |state| state.scrollbar_height)
    }

    /// Rows visible through the scroll host.
    pub fn visible_rows(&self) -> u32 {
        self.state().map_or(0, |state| state.viewport_height)
    }

    /// Columns visible through the scroll host.
    pub fn visible_columns(&self) -> u32 {
        self.state().map_or(0, |state| state.viewport_width)
    }

    /// Moves to `offset`, clamped to the host's maximum.
    pub fn scroll_to(&self, offset: ScrollOffset) {
        let max = self.max_offset();
        let next = ScrollOffset::new(offset.x.min(max.x), offset.y.min(max.y));
        let _ = self.offset.set(next);
    }

    /// Moves by a cell delta, clamped to the host's range.
    pub fn scroll_by(&self, dx: i32, dy: i32) {
        let current = self.offset();
        self.scroll_to(ScrollOffset::new(
            offset_component(current.x, dx),
            offset_component(current.y, dy),
        ));
    }

    /// Places `row` at the top of the viewport.
    pub fn scroll_to_row(&self, row: u32) {
        self.scroll_to(ScrollOffset::new(self.offset().x, row));
    }

    /// Scrolls the minimum amount needed to show `rows` (vertical, in rows).
    pub fn reveal(&self, rows: Range<u32>) {
        let visible = self.visible_rows().max(1);
        let current = self.offset().y;
        let target = if rows.start < current {
            rows.start
        } else if rows.end > current.saturating_add(visible) {
            rows.end.saturating_sub(visible)
        } else {
            current
        };
        if target != current {
            self.scroll_to(ScrollOffset::new(self.offset().x, target));
        }
    }

    pub(crate) fn sync(&self, offset: ScrollOffset) {
        if self.offset() != offset {
            let _ = self.offset.update(|current| *current = offset);
        }
    }

    fn state(&self) -> Option<ElementScrollState> {
        self.element
            .current()
            .and_then(|snapshot| snapshot.scroll())
    }
}

fn offset_component(value: u32, delta: i32) -> u32 {
    if delta >= 0 {
        value.saturating_add(delta as u32)
    } else {
        value.saturating_sub(delta.unsigned_abs())
    }
}

impl ComponentContext {
    /// Creates a controller for a [`scroll_area`] in this component.
    ///
    /// The controller owns a live offset; `scroll_to`, `scroll_by`,
    /// `scroll_to_row`, and `reveal` wake this component, and scrolling by
    /// wheel or keys is mirrored back into it.
    pub fn use_scroll_controller(&mut self) -> ScrollController {
        let offset = self.use_live_state(|| ScrollOffset::new(0, 0));
        let element = self.use_element_ref();
        ScrollController { offset, element }
    }
}

/// Clipping scroll container; see [`ScrollAreaProps`] for its configuration.
///
/// Caller style is applied exactly once, by the canonical host-prop merge.
/// Scrolling owns clipping on its enabled axes; that required invariant is
/// applied after the caller merge so it cannot be overridden away.
pub fn scroll_area(cx: &mut ComponentContext, props: &Props<ScrollAreaProps>) -> Node {
    let theme = cx.use_theme();
    let axes = props.axes | ScrollAxes::Vertical;
    let scrollbar_visibility = props.scrollbar_visibility | ScrollbarVisibility::Auto;
    let horizontal = matches!(axes, ScrollAxes::Horizontal | ScrollAxes::Both);
    let vertical = matches!(axes, ScrollAxes::Vertical | ScrollAxes::Both);

    let mut dom = props.host_props(DomProps::default());
    if horizontal {
        dom.style.overflow_x = Attr::Set(Overflow::Clip);
    }
    if vertical {
        dom.style.overflow_y = Attr::Set(Overflow::Clip);
    }
    let controller = props.controller.as_ref().cloned();
    if let Some(controller) = controller.as_ref()
        && props.offset.as_ref().is_none()
    {
        if !dom.element_ref.is_set() {
            dom.element_ref = Attr::Set(controller.element.clone());
        }
        let caller = dom.events.scroll.as_ref().cloned();
        let for_listener = controller.clone();
        dom.events.scroll = Attr::Set(EventListener::compose(
            move |event: ScrollEvent| for_listener.sync(event.offset),
            caller,
        ));
    }
    let requested_offset = controller
        .as_ref()
        .filter(|_| props.offset.as_ref().is_none())
        .map(ScrollController::offset)
        .or(props.offset.as_ref().copied());
    dom.scroll = Some(Box::new(ScrollConfig {
        horizontal,
        vertical,
        scrollbar_visibility,
        offset: requested_offset,
        enable_mouse: props.enable_mouse | true,
        enable_wheel: props.enable_wheel | true,
        enable_keyboard: props.enable_keyboard | true,
        wheel_step: (props.wheel_step | 1).max(1),
        scrollbar: props.data().scrollbar_style.clone() | theme.scrollbar.clone(),
    }));

    ui! { <view dom={dom}>{props.children_node()}</view> }
}
