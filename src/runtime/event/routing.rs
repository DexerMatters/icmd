//! Routing, hit testing, and the pointer/scroll bookkeeping the dispatcher
//! delegates to. These live beside the dispatcher so it stays focused on state
//! transitions and public API.
#![allow(unused_imports)]

use super::*;

/// Pointer handler slot a routing lookup resolves, one per pointer event kind.
#[derive(Debug, Clone, Copy)]
pub(super) enum PointerHandler {
    /// Pointer pressed.
    Down,
    /// Pointer released.
    Up,
    /// Pointer moved.
    Move,
    /// Pointer interaction cancelled.
    Cancel,
    /// Pointer entered the node's box without yet being over it.
    Over,
    /// Pointer left the node's box.
    Out,
    /// Pointer entered the node and its subtree.
    Enter,
    /// Pointer left the node and its subtree.
    Leave,
    /// Node gained pointer capture.
    GotCapture,
    /// Node lost pointer capture.
    LostCapture,
    /// Primary-button click completed.
    Click,
}

impl PointerHandler {
    /// Maps a pointer event kind to its handler slot.
    pub(super) fn for_kind(kind: PointerEventKind) -> Self {
        match kind {
            PointerEventKind::Down => Self::Down,
            PointerEventKind::Up => Self::Up,
            PointerEventKind::Move => Self::Move,
            PointerEventKind::Cancel => Self::Cancel,
            PointerEventKind::Over => Self::Over,
            PointerEventKind::Out => Self::Out,
            PointerEventKind::Enter => Self::Enter,
            PointerEventKind::Leave => Self::Leave,
            PointerEventKind::GotCapture => Self::GotCapture,
            PointerEventKind::LostCapture => Self::LostCapture,
            PointerEventKind::Click => Self::Click,
        }
    }
}

/// Appends every bubble-phase pointer listener from `target` to the root.
pub(super) fn queue_pointer(
    state: &EventState,
    target: DomId,
    handler: PointerHandler,
    event: PointerEvent,
    deliveries: &mut Vec<(EventListener<PointerEvent>, PointerEvent)>,
) {
    for listener in state.route_pointer(target, handler) {
        deliveries.push((listener, event));
    }
}

/// Appends only the listener on `target` itself, skipping ancestors.
pub(super) fn queue_pointer_direct(
    state: &EventState,
    target: DomId,
    handler: PointerHandler,
    event: PointerEvent,
    deliveries: &mut Vec<(EventListener<PointerEvent>, PointerEvent)>,
) {
    if let Some(listener) = state.pointer_listener(target, handler) {
        deliveries.push((listener, event));
    }
}

/// Maps a mouse wheel kind to a `(columns, rows)` delta, or `None` for
/// non-scroll kinds.
pub(super) fn wheel_delta(kind: MouseEventKind) -> Option<(i16, i16)> {
    match kind {
        MouseEventKind::ScrollUp => Some((0, -1)),
        MouseEventKind::ScrollDown => Some((0, 1)),
        MouseEventKind::ScrollLeft => Some((-1, 0)),
        MouseEventKind::ScrollRight => Some((1, 0)),
        _ => None,
    }
}

/// Which end of a scroll range a key press jumps to.
#[derive(Debug, Clone, Copy)]
pub(super) enum ScrollEdge {
    /// Jump to the start of the range.
    Start,
    /// Jump to the end of the range.
    End,
}

/// Maps a key to `(x, y, edge, pages)` scroll intent; at most one field is set.
pub(super) fn key_scroll(
    code: crossterm::event::KeyCode,
) -> (i32, i32, Option<ScrollEdge>, Option<i32>) {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::Up => (0, -1, None, None),
        KeyCode::Down => (0, 1, None, None),
        KeyCode::Left => (-1, 0, None, None),
        KeyCode::Right => (1, 0, None, None),
        KeyCode::Home => (0, 0, Some(ScrollEdge::Start), None),
        KeyCode::End => (0, 0, Some(ScrollEdge::End), None),
        KeyCode::PageUp => (0, 0, None, Some(-1)),
        KeyCode::PageDown => (0, 0, None, Some(1)),
        _ => (0, 0, None, None),
    }
}

/// Clamps `offset + delta` into `0..=max` and returns the new offset together
/// with the delta actually consumed.
pub(super) fn consume_scroll(offset: i32, delta: i32, max: i32) -> (i32, i32) {
    let next = offset.saturating_add(delta).clamp(0, max.max(0));
    (next, next - offset)
}

/// Extracts a listener from an attribute slot, yielding `None` when unset.
pub(super) fn listener<T: Clone>(slot: &Attr<T>) -> Option<T> {
    slot.clone().into()
}

/// Appends every bubble-phase scroll listener from `target` to the root.
///
/// The bubble route is deliberate: a scroll host can wrap the region that
/// actually scrolls and still observe it, which is how the editor controls keep
/// their painted offset in step with the scroll area they own. A caller that
/// controls an offset must therefore check that an event describes its own
/// region before adopting it, because a nested region's event reaches it too.
pub(super) fn queue_scroll_event(
    state: &EventState,
    target: DomId,
    event: ScrollEvent,
    deliveries: &mut Vec<(EventListener<ScrollEvent>, ScrollEvent)>,
) {
    for listener in state.route_scroll(target) {
        deliveries.push((listener, event));
    }
}

/// Builds a scroll event from the committed offset, the maxima, and the offset
/// before the change.
pub(super) fn make_scroll_event(
    offset: RuntimeScrollOffset,
    max_x: i32,
    max_y: i32,
    before: RuntimeScrollOffset,
) -> ScrollEvent {
    ScrollEvent {
        offset: ScrollOffset::new(
            u32::try_from(offset.x.max(0)).unwrap_or(u32::MAX),
            u32::try_from(offset.y.max(0)).unwrap_or(u32::MAX),
        ),
        max_offset: ScrollOffset::new(
            u32::try_from(max_x.max(0)).unwrap_or(u32::MAX),
            u32::try_from(max_y.max(0)).unwrap_or(u32::MAX),
        ),
        delta: ScrollDelta::new(
            offset.x.saturating_sub(before.x),
            offset.y.saturating_sub(before.y),
        ),
    }
}

/// Returns the listener registered for one pointer handler in `handlers`.
pub(super) fn pointer_slot(
    handlers: &EventHandlers,
    handler: PointerHandler,
) -> Option<EventListener<PointerEvent>> {
    let slot = match handler {
        PointerHandler::Down => &handlers.pointer_down,
        PointerHandler::Up => &handlers.pointer_up,
        PointerHandler::Move => &handlers.pointer_move,
        PointerHandler::Cancel => &handlers.pointer_cancel,
        PointerHandler::Over => &handlers.pointer_over,
        PointerHandler::Out => &handlers.pointer_out,
        PointerHandler::Enter => &handlers.pointer_enter,
        PointerHandler::Leave => &handlers.pointer_leave,
        PointerHandler::GotCapture => &handlers.got_pointer_capture,
        PointerHandler::LostCapture => &handlers.lost_pointer_capture,
        PointerHandler::Click => &handlers.click,
    };
    listener(slot)
}

impl EventState {
    /// Indexed lookup of a region by DOM id, counting the probe; routing walks
    /// ancestors and must not rescan every region.
    pub(super) fn region(&self, id: DomId) -> Option<&EventRegion> {
        self.id_probes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.by_id
            .get(&id)
            .and_then(|index| self.regions.get(*index))
    }

    /// Returns the topmost region containing `position`, ranked by level then
    /// order.
    pub(super) fn hit_target(&self, position: ScreenPosition) -> Option<DomId> {
        self.regions
            .iter()
            .filter(|region| region.rect.contains(position.line, position.column))
            .max_by_key(|region| (region.level, region.order))
            .map(|region| region.id)
    }

    /// Returns the pointer listener on `target` for one handler.
    pub(super) fn pointer_listener(
        &self,
        target: DomId,
        handler: PointerHandler,
    ) -> Option<EventListener<PointerEvent>> {
        self.region(target)
            .and_then(|region| pointer_slot(&region.handlers, handler))
    }

    /// Returns the bubble-phase pointer listeners from `target` to the root.
    pub(super) fn route_pointer(
        &self,
        target: DomId,
        handler: PointerHandler,
    ) -> Vec<EventListener<PointerEvent>> {
        self.route(target, |handlers| pointer_slot(handlers, handler))
    }

    /// Returns the bubble-phase wheel listeners from `target` to the root.
    pub(super) fn route_wheel(&self, target: DomId) -> Vec<EventListener<WheelEvent>> {
        self.route(target, |handlers| listener(&handlers.wheel))
    }

    /// Returns the bubble-phase scroll listeners from `target` to the root.
    pub(super) fn route_scroll(&self, target: DomId) -> Vec<EventListener<ScrollEvent>> {
        self.route(target, |handlers| listener(&handlers.scroll))
    }

    /// Walks from `target` to the root and returns the nearest focusable region,
    /// or the nearest scrollable one when none is focusable.
    pub(super) fn focus_target_for(&self, target: DomId) -> Option<DomId> {
        let mut current = Some(target);
        let mut visited = HashSet::new();
        let mut first_focusable = None;
        while let Some(id) = current {
            if !visited.insert(id) {
                break;
            }
            let Some(region) = self.region(id) else {
                break;
            };
            if region.focusable {
                return Some(id);
            }
            if first_focusable.is_none() && region.scroll.is_some() {
                first_focusable = Some(id);
            }
            current = region.parent;
        }
        first_focusable
    }

    /// Reports whether `target` has any pointer listener or a scrollable
    /// ancestor.
    pub(super) fn is_pointer_interactive(&self, target: DomId) -> bool {
        [
            PointerHandler::Down,
            PointerHandler::Up,
            PointerHandler::Move,
            PointerHandler::Cancel,
            PointerHandler::Click,
        ]
        .into_iter()
        .any(|handler| !self.route_pointer(target, handler).is_empty())
            || self.has_scroll_ancestor(target)
    }

    /// Reports whether `target` or one of its ancestors is scrollable.
    pub(super) fn has_scroll_ancestor(&self, target: DomId) -> bool {
        let mut current = Some(target);
        let mut visited = HashSet::new();
        while let Some(id) = current {
            if !visited.insert(id) {
                return false;
            }
            let Some(region) = self.region(id) else {
                return false;
            };
            if region.scroll.is_some() {
                return true;
            }
            current = region.parent;
        }
        false
    }

    /// Walks from `target` to the root and returns the first scrollbar whose
    /// track contains `position`.
    pub(super) fn scrollbar_at(
        &self,
        target: DomId,
        position: ScreenPosition,
    ) -> Option<(DomId, ScrollbarRegion)> {
        let mut current = Some(target);
        let mut visited = HashSet::new();
        while let Some(id) = current {
            if !visited.insert(id) {
                break;
            }
            let Some(region) = self.region(id) else {
                break;
            };
            if let Some(scroll) = region.scroll {
                if scroll
                    .vertical_bar
                    .is_some_and(|bar| bar.contains_track(position))
                {
                    return scroll.vertical_bar.map(|bar| (id, bar));
                }
                if scroll
                    .horizontal_bar
                    .is_some_and(|bar| bar.contains_track(position))
                {
                    return scroll.horizontal_bar.map(|bar| (id, bar));
                }
            }
            current = region.parent;
        }
        None
    }

    /// Returns listeners in bubble order, from `target` to the root.
    pub(super) fn route<T>(
        &self,
        target: DomId,
        listener: impl Fn(&EventHandlers) -> Option<EventListener<T>>,
    ) -> Vec<EventListener<T>> {
        let mut callbacks = self.route_capture(target, listener);
        callbacks.reverse();
        callbacks
    }

    /// Returns listeners in capture order, from the root to `target`. The bubble
    /// pass is the same route reversed, so both share one traversal.
    pub(super) fn route_capture<T>(
        &self,
        target: DomId,
        listener: impl Fn(&EventHandlers) -> Option<EventListener<T>>,
    ) -> Vec<EventListener<T>> {
        let mut callbacks = Vec::new();
        let mut chain = Vec::new();
        let mut current = Some(target);
        let mut visited = HashSet::new();
        while let Some(id) = current {
            if !visited.insert(id) {
                break;
            }
            let Some(region) = self.region(id) else {
                break;
            };
            chain.push(id);
            current = region.parent;
        }
        for id in chain.into_iter().rev() {
            let Some(region) = self.region(id) else {
                continue;
            };
            if let Some(callback) = listener(&region.handlers) {
                callbacks.push(callback);
            }
        }
        callbacks
    }
}
