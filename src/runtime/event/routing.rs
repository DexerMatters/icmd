// Routing, hit testing, and the pointer/scroll bookkeeping that the
// dispatcher delegates to. They live beside the dispatcher so it stays
// focused on state transitions and public API.
#![allow(unused_imports)]

use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum PointerHandler {
    Down,
    Up,
    Move,
    Cancel,
    Over,
    Out,
    Enter,
    Leave,
    GotCapture,
    LostCapture,
    Click,
}

impl PointerHandler {
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

pub(super) fn wheel_delta(kind: MouseEventKind) -> Option<(i16, i16)> {
    match kind {
        MouseEventKind::ScrollUp => Some((0, -1)),
        MouseEventKind::ScrollDown => Some((0, 1)),
        MouseEventKind::ScrollLeft => Some((-1, 0)),
        MouseEventKind::ScrollRight => Some((1, 0)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ScrollEdge {
    Start,
    End,
}

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

pub(super) fn consume_scroll(offset: i32, delta: i32, max: i32) -> (i32, i32) {
    let next = offset.saturating_add(delta).clamp(0, max.max(0));
    (next, next - offset)
}

pub(super) fn listener<T: Clone>(slot: &Attr<T>) -> Option<T> {
    slot.clone().into()
}

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
    // Indexed lookup: routing walks ancestors and must not rescan every region.
    pub(super) fn region(&self, id: DomId) -> Option<&EventRegion> {
        self.id_probes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.by_id
            .get(&id)
            .and_then(|index| self.regions.get(*index))
    }

    pub(super) fn hit_target(&self, position: ScreenPosition) -> Option<DomId> {
        self.regions
            .iter()
            .filter(|region| region.rect.contains(position.line, position.column))
            .max_by_key(|region| (region.level, region.order))
            .map(|region| region.id)
    }

    pub(super) fn pointer_listener(
        &self,
        target: DomId,
        handler: PointerHandler,
    ) -> Option<EventListener<PointerEvent>> {
        self.region(target)
            .and_then(|region| pointer_slot(&region.handlers, handler))
    }

    pub(super) fn route_pointer(
        &self,
        target: DomId,
        handler: PointerHandler,
    ) -> Vec<EventListener<PointerEvent>> {
        self.route(target, |handlers| pointer_slot(handlers, handler))
    }

    pub(super) fn route_wheel(&self, target: DomId) -> Vec<EventListener<WheelEvent>> {
        self.route(target, |handlers| listener(&handlers.wheel))
    }

    pub(super) fn route_scroll(&self, target: DomId) -> Vec<EventListener<ScrollEvent>> {
        self.route(target, |handlers| listener(&handlers.scroll))
    }

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

    pub(super) fn route<T>(
        &self,
        target: DomId,
        listener: impl Fn(&EventHandlers) -> Option<EventListener<T>>,
    ) -> Vec<EventListener<T>> {
        let mut callbacks = self.route_capture(target, listener);
        callbacks.reverse();
        callbacks
    }

    // Root-to-target order, which is the order capture listeners run in. The
    // bubble pass is the same route reversed, so both share one traversal.
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
        // `chain` is target-to-root; capture runs the other way.
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
