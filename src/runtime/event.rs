use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};

use crate::{
    DomId, ScreenPosition, Size,
    basic::{
        EventHandlers, EventListener, FocusEvent, KeyboardEvent, PasteEvent, PointerButton,
        PointerEvent, PointerEventKind, ResizeEvent, ScrollEvent, WheelEvent, common::Attr,
        events::mouse_details,
    },
};

use super::commit::ViewportSetter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EventRect {
    pub(crate) line: i32,
    pub(crate) column: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl EventRect {
    pub(crate) const fn new(line: i32, column: i32, width: i32, height: i32) -> Self {
        Self {
            line,
            column,
            width,
            height,
        }
    }

    fn contains(self, line: i32, column: i32) -> bool {
        self.width > 0
            && self.height > 0
            && line >= self.line
            && column >= self.column
            && line < self.line.saturating_add(self.height)
            && column < self.column.saturating_add(self.width)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EventRegion {
    pub(crate) id: DomId,
    pub(crate) parent: Option<DomId>,
    pub(crate) rect: EventRect,
    pub(crate) level: i32,
    pub(crate) order: u64,
    pub(crate) handlers: EventHandlers,
    pub(crate) scroll: Option<ScrollRegion>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollRegion {
    pub(crate) max_x: i32,
    pub(crate) max_y: i32,
    pub(crate) viewport_height: i32,
    pub(crate) horizontal: bool,
    pub(crate) vertical: bool,
    pub(crate) wheel_step: u16,
    pub(crate) wheel: bool,
    pub(crate) enable_mouse: bool,
    pub(crate) vertical_bar: Option<ScrollbarRegion>,
    pub(crate) horizontal_bar: Option<ScrollbarRegion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarRegion {
    pub(crate) line: i32,
    pub(crate) column: i32,
    pub(crate) length: i32,
    pub(crate) thumb_start: i32,
    pub(crate) thumb_len: i32,
    pub(crate) max_offset: i32,
    pub(crate) vertical: bool,
}

impl ScrollbarRegion {
    pub(crate) fn contains_track(self, position: ScreenPosition) -> bool {
        if self.vertical {
            position.column == self.column
                && position.line >= self.line
                && position.line < self.line.saturating_add(self.length)
        } else {
            position.line == self.line
                && position.column >= self.column
                && position.column < self.column.saturating_add(self.length)
        }
    }

    pub(crate) fn contains_thumb(self, position: ScreenPosition) -> bool {
        if !self.contains_track(position) {
            return false;
        }
        let coordinate = if self.vertical {
            position.line
        } else {
            position.column
        };
        let start = if self.vertical {
            self.line
        } else {
            self.column
        };
        let thumb_start = start.saturating_add(self.thumb_start);
        coordinate >= thumb_start && coordinate < thumb_start.saturating_add(self.thumb_len)
    }

    fn coordinate(self, position: ScreenPosition) -> i32 {
        if self.vertical {
            position.line
        } else {
            position.column
        }
    }

    fn start(self) -> i32 {
        if self.vertical {
            self.line
        } else {
            self.column
        }
    }
}

pub(crate) fn scrollbar_region(
    line: i32,
    column: i32,
    length: i32,
    content_len: i32,
    offset: i32,
    vertical: bool,
) -> Option<ScrollbarRegion> {
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
    Some(ScrollbarRegion {
        line,
        column,
        length,
        thumb_start,
        thumb_len,
        max_offset,
        vertical,
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ScrollOffset {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Default)]
struct EventState {
    regions: Vec<EventRegion>,
    focused: Option<DomId>,
    captured: Option<PointerCapture>,
    hovered: Option<DomId>,
    buttons: u16,
    last_position: ScreenPosition,
    drag: Option<ScrollDrag>,
}

#[derive(Debug, Clone, Copy)]
struct PointerCapture {
    target: DomId,
}

#[derive(Debug, Clone, Copy)]
struct ScrollDrag {
    source: DomId,
    bar: ScrollbarRegion,
    grab_offset: i32,
    last_position: ScreenPosition,
    moved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollInput {
    Wheel,
    Keyboard,
}

#[derive(Clone)]
pub struct EventDispatcher {
    state: Arc<RwLock<EventState>>,
    viewport: ViewportSetter,
    scroll_offsets: Arc<Mutex<HashMap<DomId, ScrollOffset>>>,
}

impl EventDispatcher {
    pub(crate) fn new(viewport: ViewportSetter) -> Self {
        Self {
            state: Arc::new(RwLock::new(EventState::default())),
            viewport,
            scroll_offsets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn scroll_offsets(&self) -> Arc<Mutex<HashMap<DomId, ScrollOffset>>> {
        self.scroll_offsets.clone()
    }

    pub(crate) fn publish(
        &self,
        mut regions: Vec<EventRegion>,
        retained_scroll_ids: &HashSet<DomId>,
    ) {
        regions.sort_by_key(|region| region.order);
        let mut capture_deliveries = Vec::new();
        let lost_focus = {
            let mut state = self.state.write().expect("event registry poisoned");
            let lost_focus = if state
                .focused
                .is_some_and(|id| !regions.iter().any(|region| region.id == id))
            {
                let old = state.focused.take();
                old.and_then(|id| {
                    state
                        .regions
                        .iter()
                        .find(|region| region.id == id)
                        .and_then(|region| listener(&region.handlers.focus_event))
                })
            } else {
                None
            };
            if let Some(capture) = state
                .captured
                .filter(|capture| !regions.iter().any(|region| region.id == capture.target))
            {
                let pointer = PointerEvent::new(
                    PointerEventKind::Cancel,
                    state.last_position,
                    PointerButton::None,
                    0,
                    KeyModifiers::empty(),
                );
                queue_pointer(
                    &state,
                    capture.target,
                    PointerHandler::Cancel,
                    pointer,
                    &mut capture_deliveries,
                );
                queue_pointer(
                    &state,
                    capture.target,
                    PointerHandler::LostCapture,
                    PointerEvent {
                        kind: PointerEventKind::LostCapture,
                        ..pointer
                    },
                    &mut capture_deliveries,
                );
                state.captured = None;
                state.buttons = 0;
            }
            if state
                .drag
                .is_some_and(|drag| !regions.iter().any(|region| region.id == drag.source))
            {
                state.drag = None;
            }
            if state
                .hovered
                .is_some_and(|id| !regions.iter().any(|region| region.id == id))
            {
                state.hovered = None;
            }
            state.regions = regions;
            lost_focus
        };
        let state = self.state.read().expect("event registry poisoned");
        let mut offsets = self.scroll_offsets.lock().expect("scroll mutex poisoned");
        offsets.retain(|id, _| {
            retained_scroll_ids.contains(id)
                || state
                    .regions
                    .iter()
                    .any(|region| region.id == *id && region.scroll.is_some())
        });
        for region in state.regions.iter() {
            let Some(scroll) = region.scroll else {
                continue;
            };
            let offset = offsets.entry(region.id).or_default();
            offset.x = offset.x.clamp(0, scroll.max_x.max(0));
            offset.y = offset.y.clamp(0, scroll.max_y.max(0));
        }
        drop(offsets);
        drop(state);
        for (listener, event) in capture_deliveries {
            listener.call(event);
        }
        if let Some(listener) = lost_focus {
            listener.call(FocusEvent::Lost);
        }
    }

    pub fn dispatch(&self, event: Event) -> usize {
        match event {
            Event::Mouse(event) => self.dispatch_mouse(event),
            Event::Key(event) => self.dispatch_key(KeyboardEvent { key: event }),
            Event::Paste(value) => self.dispatch_paste(PasteEvent { text: value }),
            Event::Resize(width, height) => self.dispatch_resize(ResizeEvent {
                size: Size::new(width, height),
            }),
            Event::FocusGained => self.dispatch_terminal_focus(FocusEvent::Gained),
            Event::FocusLost => self.dispatch_terminal_focus(FocusEvent::Lost),
        }
    }

    pub fn focus(&self, id: DomId) -> bool {
        self.change_focus(id).0
    }

    fn change_focus(&self, id: DomId) -> (bool, usize) {
        let callbacks = {
            let mut state = self.state.write().expect("event registry poisoned");
            if !state.regions.iter().any(|region| region.id == id) || state.focused == Some(id) {
                return (false, 0);
            }
            let old = state.focused.replace(id);
            let mut callbacks = Vec::with_capacity(2);
            if let Some(old) = old
                && let Some(region) = state.regions.iter().find(|region| region.id == old)
                && let Some(listener) = listener(&region.handlers.focus_event)
            {
                callbacks.push((listener, FocusEvent::Lost));
            }
            if let Some(region) = state.regions.iter().find(|region| region.id == id)
                && let Some(listener) = listener(&region.handlers.focus_event)
            {
                callbacks.push((listener, FocusEvent::Gained));
            }
            callbacks
        };

        let count = callbacks.len();
        for (listener, event) in callbacks {
            listener.call(event);
        }
        (true, count)
    }

    pub fn blur(&self) -> bool {
        let callback = {
            let mut state = self.state.write().expect("event registry poisoned");
            let Some(old) = state.focused.take() else {
                return false;
            };
            state
                .regions
                .iter()
                .find(|region| region.id == old)
                .and_then(|region| listener(&region.handlers.focus_event))
        };
        if let Some(listener) = callback {
            listener.call(FocusEvent::Lost);
        }
        true
    }

    pub fn focused(&self) -> Option<DomId> {
        self.state.read().expect("event registry poisoned").focused
    }

    pub fn set_pointer_capture(&self, id: DomId) -> bool {
        let mut deliveries = Vec::new();
        let accepted = {
            let mut state = self.state.write().expect("event registry poisoned");
            if !state.regions.iter().any(|region| region.id == id) {
                false
            } else if state.captured.is_some_and(|capture| capture.target == id) {
                true
            } else {
                if let Some(previous) = state.captured {
                    let lost = PointerEvent::new(
                        PointerEventKind::LostCapture,
                        state.last_position,
                        PointerButton::None,
                        state.buttons,
                        KeyModifiers::empty(),
                    );
                    queue_pointer(
                        &state,
                        previous.target,
                        PointerHandler::LostCapture,
                        lost,
                        &mut deliveries,
                    );
                }
                state.captured = Some(PointerCapture { target: id });
                let gained = PointerEvent::new(
                    PointerEventKind::GotCapture,
                    state.last_position,
                    PointerButton::None,
                    state.buttons,
                    KeyModifiers::empty(),
                );
                queue_pointer(
                    &state,
                    id,
                    PointerHandler::GotCapture,
                    gained,
                    &mut deliveries,
                );
                true
            }
        };
        for (listener, event) in deliveries {
            listener.call(event);
        }
        accepted
    }

    pub fn release_pointer_capture(&self, id: DomId) -> bool {
        let mut deliveries = Vec::new();
        let released = {
            let mut state = self.state.write().expect("event registry poisoned");
            if state.captured.is_some_and(|capture| capture.target == id) {
                state.captured = None;
                let lost = PointerEvent::new(
                    PointerEventKind::LostCapture,
                    state.last_position,
                    PointerButton::None,
                    state.buttons,
                    KeyModifiers::empty(),
                );
                queue_pointer(
                    &state,
                    id,
                    PointerHandler::LostCapture,
                    lost,
                    &mut deliveries,
                );
                true
            } else {
                false
            }
        };
        for (listener, event) in deliveries {
            listener.call(event);
        }
        released
    }

    pub fn pointer_capture(&self) -> Option<DomId> {
        self.state
            .read()
            .expect("event registry poisoned")
            .captured
            .map(|capture| capture.target)
    }

    fn dispatch_mouse(&self, event: MouseEvent) -> usize {
        let mut pointer_deliveries = Vec::new();
        let mut wheel_deliveries = Vec::new();
        let mut scroll_deliveries = Vec::new();
        let mut focus_target = None;
        let mut scroll_changed = false;

        {
            let mut state = self.state.write().expect("event registry poisoned");
            let position = ScreenPosition::new(event.row as i32, event.column as i32);
            state.last_position = position;
            let normal_target = state.hit_target(position);

            if let Some((delta_x, delta_y)) = wheel_delta(event.kind) {
                if let Some(target) = normal_target {
                    let wheel = WheelEvent {
                        position,
                        delta_x,
                        delta_y,
                        modifiers: event.modifiers,
                    };
                    for listener in state.route_wheel(target) {
                        wheel_deliveries.push((listener, wheel));
                    }
                    scroll_changed = self.scroll_from_target(
                        &state,
                        target,
                        i32::from(delta_x),
                        i32::from(delta_y),
                        ScrollInput::Wheel,
                        &mut scroll_deliveries,
                    );
                }
            } else {
                let (kind, changed_button) = mouse_details(event);
                match kind {
                    PointerEventKind::Down => {
                        state.buttons |= changed_button.bit();
                    }
                    PointerEventKind::Up => {
                        state.buttons &= !changed_button.bit();
                    }
                    PointerEventKind::Move if changed_button != PointerButton::None => {
                        state.buttons |= changed_button.bit();
                    }
                    _ => {}
                }

                let capture = state.captured;
                let captured_target = capture.map(|capture| capture.target);
                let capture_event = state.captured.is_some();
                let target = if capture_event {
                    captured_target.or(normal_target)
                } else {
                    normal_target
                };

                let pointer = PointerEvent::new(
                    kind,
                    position,
                    changed_button,
                    state.buttons,
                    event.modifiers,
                )
                .with_local_position(
                    target
                        .and_then(|id| state.region(id))
                        .map_or(position, |region| {
                            ScreenPosition::new(
                                position.line.saturating_sub(region.rect.line),
                                position.column.saturating_sub(region.rect.column),
                            )
                        }),
                );

                // Boundary events use normal hit testing. While captured,
                // pointer boundary events stay with the capture target.
                if !capture_event
                    && matches!(event.kind, MouseEventKind::Moved | MouseEventKind::Down(_))
                    && state.hovered != normal_target
                {
                    let boundary = PointerEvent::new(
                        PointerEventKind::Out,
                        position,
                        PointerButton::None,
                        state.buttons,
                        event.modifiers,
                    );
                    if let Some(old) = state.hovered {
                        queue_pointer(
                            &state,
                            old,
                            PointerHandler::Out,
                            boundary,
                            &mut pointer_deliveries,
                        );
                        queue_pointer_direct(
                            &state,
                            old,
                            PointerHandler::Leave,
                            PointerEvent {
                                kind: PointerEventKind::Leave,
                                ..boundary
                            },
                            &mut pointer_deliveries,
                        );
                    }
                    if let Some(new) = normal_target {
                        let over = PointerEvent {
                            kind: PointerEventKind::Over,
                            ..boundary
                        };
                        queue_pointer(
                            &state,
                            new,
                            PointerHandler::Over,
                            over,
                            &mut pointer_deliveries,
                        );
                        queue_pointer_direct(
                            &state,
                            new,
                            PointerHandler::Enter,
                            PointerEvent {
                                kind: PointerEventKind::Enter,
                                ..boundary
                            },
                            &mut pointer_deliveries,
                        );
                    }
                    state.hovered = normal_target;
                }

                if let Some(target) = target {
                    if matches!(kind, PointerEventKind::Down)
                        && changed_button == PointerButton::Primary
                    {
                        state.drag = None;
                        if let Some((source, bar)) = state.scrollbar_at(target, position) {
                            let enable_mouse = state
                                .regions
                                .iter()
                                .find(|region| region.id == source)
                                .and_then(|region| region.scroll)
                                .is_some_and(|scroll| scroll.enable_mouse);
                            if enable_mouse {
                                if bar.contains_thumb(position) {
                                    state.drag = Some(ScrollDrag {
                                        source,
                                        bar,
                                        grab_offset: bar.coordinate(position).saturating_sub(
                                            bar.start().saturating_add(bar.thumb_start),
                                        ),
                                        last_position: position,
                                        moved: false,
                                    });
                                } else {
                                    scroll_changed |= self.scrollbar_click(
                                        source,
                                        bar,
                                        position,
                                        &state,
                                        &mut scroll_deliveries,
                                    );
                                }
                            }
                        }
                    }

                    if matches!(kind, PointerEventKind::Move)
                        && state.buttons & PointerButton::Primary.bit() != 0
                        && matches!(changed_button, PointerButton::None | PointerButton::Primary)
                        && let Some(mut drag) = state.drag
                    {
                        let delta_x = drag.last_position.column.saturating_sub(position.column);
                        let delta_y = drag.last_position.line.saturating_sub(position.line);
                        if delta_x != 0 || delta_y != 0 {
                            let coordinate = if drag.bar.vertical {
                                position.line
                            } else {
                                position.column
                            };
                            let track_start = drag.bar.start();
                            let travel = drag.bar.length.saturating_sub(drag.bar.thumb_len);
                            if travel > 0 && drag.bar.max_offset > 0 {
                                let desired_start = coordinate
                                    .saturating_sub(track_start)
                                    .saturating_sub(drag.grab_offset)
                                    .clamp(0, travel);
                                let next = ((desired_start as i64 * drag.bar.max_offset as i64
                                    + travel as i64 / 2)
                                    / travel as i64)
                                    as i32;
                                if self.set_scroll_offset(
                                    drag.source,
                                    drag.bar.vertical,
                                    next,
                                    &state,
                                    &mut scroll_deliveries,
                                ) {
                                    drag.moved = true;
                                    scroll_changed = true;
                                }
                            }
                        }
                        drag.last_position = position;
                        state.drag = Some(drag);
                    }

                    queue_pointer(
                        &state,
                        target,
                        PointerHandler::for_kind(kind),
                        pointer,
                        &mut pointer_deliveries,
                    );

                    if matches!(kind, PointerEventKind::Down)
                        && state.captured.is_none()
                        && state.is_pointer_interactive(target)
                    {
                        let capture = PointerCapture { target };
                        state.captured = Some(capture);
                        queue_pointer(
                            &state,
                            target,
                            PointerHandler::GotCapture,
                            PointerEvent {
                                kind: PointerEventKind::GotCapture,
                                ..pointer
                            },
                            &mut pointer_deliveries,
                        );
                    }

                    if matches!(kind, PointerEventKind::Up)
                        && let Some(capture) = capture
                        && state.buttons == 0
                    {
                        queue_pointer(
                            &state,
                            capture.target,
                            PointerHandler::LostCapture,
                            PointerEvent {
                                kind: PointerEventKind::LostCapture,
                                ..pointer
                            },
                            &mut pointer_deliveries,
                        );
                        state.captured = None;

                        // Browser ordering is pointerup -> lostpointercapture
                        // -> click. A click is only produced when the primary
                        // button is released over its original target.
                        let dragged = state.drag.take().is_some_and(|drag| drag.moved);
                        if changed_button == PointerButton::Primary
                            && normal_target == Some(capture.target)
                            && !dragged
                        {
                            queue_pointer(
                                &state,
                                capture.target,
                                PointerHandler::Click,
                                PointerEvent {
                                    kind: PointerEventKind::Click,
                                    ..pointer
                                },
                                &mut pointer_deliveries,
                            );
                        }

                        // Once capture is released, restore normal boundary
                        // semantics at the release location.
                        if state.hovered != normal_target {
                            let boundary = PointerEvent {
                                kind: PointerEventKind::Out,
                                ..pointer
                            };
                            if let Some(old) = state.hovered {
                                queue_pointer(
                                    &state,
                                    old,
                                    PointerHandler::Out,
                                    boundary,
                                    &mut pointer_deliveries,
                                );
                                queue_pointer_direct(
                                    &state,
                                    old,
                                    PointerHandler::Leave,
                                    PointerEvent {
                                        kind: PointerEventKind::Leave,
                                        ..boundary
                                    },
                                    &mut pointer_deliveries,
                                );
                            }
                            if let Some(new) = normal_target {
                                queue_pointer(
                                    &state,
                                    new,
                                    PointerHandler::Over,
                                    PointerEvent {
                                        kind: PointerEventKind::Over,
                                        ..boundary
                                    },
                                    &mut pointer_deliveries,
                                );
                                queue_pointer_direct(
                                    &state,
                                    new,
                                    PointerHandler::Enter,
                                    PointerEvent {
                                        kind: PointerEventKind::Enter,
                                        ..boundary
                                    },
                                    &mut pointer_deliveries,
                                );
                            }
                            state.hovered = normal_target;
                        }
                    }
                }

                if matches!(kind, PointerEventKind::Down)
                    && normal_target.is_some_and(|target| state.is_pointer_interactive(target))
                {
                    focus_target = normal_target;
                }
            }
        }

        // DOM ordering puts pointerdown (and its capture lifecycle) before
        // the focus transition caused by the press.
        let mut count = 0;
        for (listener, event) in pointer_deliveries {
            listener.call(event);
            count += 1;
        }
        for (listener, event) in wheel_deliveries {
            listener.call(event);
            count += 1;
        }
        for (listener, event) in scroll_deliveries {
            listener.call(event);
            count += 1;
        }
        if scroll_changed {
            self.viewport.request_redraw();
        }
        if let Some(target) = focus_target {
            count += self.change_focus(target).1;
        }
        count
    }

    fn dispatch_key(&self, event: KeyboardEvent) -> usize {
        let mut scroll_changed = false;
        let mut scroll_deliveries = Vec::new();
        let callbacks = {
            let state = self.state.read().expect("event registry poisoned");
            let target = state.focused.or_else(|| state.root());
            target
                .map(|target| {
                    if matches!(
                        event.key.kind,
                        crossterm::event::KeyEventKind::Press
                            | crossterm::event::KeyEventKind::Repeat
                    ) && event.key.modifiers.is_empty()
                    {
                        let (delta_x, delta_y, edge, page) = key_scroll(event.key.code);
                        if delta_x != 0 || delta_y != 0 {
                            scroll_changed |= self.scroll_from_target(
                                &state,
                                target,
                                delta_x,
                                delta_y,
                                ScrollInput::Keyboard,
                                &mut scroll_deliveries,
                            );
                        }
                        if let Some(edge) = edge {
                            scroll_changed |=
                                self.scroll_to_edge(&state, target, edge, &mut scroll_deliveries);
                        }
                        if let Some(page) = page {
                            scroll_changed |=
                                self.scroll_page(&state, target, page, &mut scroll_deliveries);
                        }
                    }
                    let mut callbacks =
                        state.route(target, |handlers| listener(&handlers.keyboard_event));
                    let key_callbacks = match event.key.kind {
                        crossterm::event::KeyEventKind::Release => {
                            state.route(target, |handlers| listener(&handlers.key_up))
                        }
                        crossterm::event::KeyEventKind::Press
                        | crossterm::event::KeyEventKind::Repeat => {
                            state.route(target, |handlers| listener(&handlers.key_down))
                        }
                    };
                    callbacks.extend(key_callbacks);
                    callbacks
                })
                .unwrap_or_default()
        };
        let count = callbacks.len();
        for listener in callbacks {
            listener.call(event);
        }
        for (listener, event) in scroll_deliveries {
            listener.call(event);
        }
        if scroll_changed {
            self.viewport.request_redraw();
        }
        count
    }

    fn scroll_from_target(
        &self,
        state: &EventState,
        target: DomId,
        delta_x: i32,
        delta_y: i32,
        input: ScrollInput,
        deliveries: &mut Vec<(EventListener<ScrollEvent>, ScrollEvent)>,
    ) -> bool {
        if delta_x == 0 && delta_y == 0 {
            return false;
        }
        let mut remaining_x = delta_x;
        let mut remaining_y = delta_y;
        let mut changed = false;
        let mut current = Some(target);
        let mut visited = HashSet::new();
        let mut offsets = self.scroll_offsets.lock().expect("scroll mutex poisoned");
        while let Some(id) = current {
            if !visited.insert(id) {
                break;
            }
            let Some(region) = state.regions.iter().find(|region| region.id == id) else {
                break;
            };
            if let Some(scroll) = region.scroll {
                if input == ScrollInput::Wheel && (!scroll.enable_mouse || !scroll.wheel) {
                    current = region.parent;
                    continue;
                }
                let offset = offsets.entry(id).or_default();
                let before_x = offset.x;
                let before_y = offset.y;
                let step = if input == ScrollInput::Wheel {
                    i32::from(scroll.wheel_step.max(1))
                } else {
                    1
                };
                let requested_x = remaining_x.saturating_mul(step);
                let requested_y = remaining_y.saturating_mul(step);
                let (x, consumed_x) = consume_scroll(offset.x, requested_x, scroll.max_x);
                let (y, consumed_y) = consume_scroll(offset.y, requested_y, scroll.max_y);
                if scroll.horizontal {
                    if x != offset.x {
                        offset.x = x;
                        changed = true;
                    }
                    if consumed_x != 0 {
                        remaining_x = if consumed_x.abs() < requested_x.abs() {
                            requested_x.signum()
                        } else {
                            0
                        };
                    }
                }
                if scroll.vertical {
                    if y != offset.y {
                        offset.y = y;
                        changed = true;
                    }
                    if consumed_y != 0 {
                        remaining_y = if consumed_y.abs() < requested_y.abs() {
                            requested_y.signum()
                        } else {
                            0
                        };
                    }
                }
                if before_x != offset.x || before_y != offset.y {
                    queue_scroll_event(
                        state,
                        id,
                        ScrollEvent {
                            offset_x: offset.x,
                            offset_y: offset.y,
                            max_x: scroll.max_x.max(0),
                            max_y: scroll.max_y.max(0),
                            delta_x: offset.x.saturating_sub(before_x),
                            delta_y: offset.y.saturating_sub(before_y),
                        },
                        deliveries,
                    );
                }
                if remaining_x == 0 && remaining_y == 0 {
                    break;
                }
            }
            current = region.parent;
        }
        changed
    }

    fn scrollbar_click(
        &self,
        source: DomId,
        bar: ScrollbarRegion,
        position: ScreenPosition,
        state: &EventState,
        deliveries: &mut Vec<(EventListener<ScrollEvent>, ScrollEvent)>,
    ) -> bool {
        let travel = bar.length.saturating_sub(bar.thumb_len);
        if travel <= 0 || bar.max_offset <= 0 {
            return false;
        }
        let desired_start = bar
            .coordinate(position)
            .saturating_sub(bar.start())
            .saturating_sub(bar.thumb_len / 2)
            .clamp(0, travel);
        let next = ((desired_start as i64 * bar.max_offset as i64 + travel as i64 / 2)
            / travel as i64) as i32;
        self.set_scroll_offset(source, bar.vertical, next, state, deliveries)
    }

    fn set_scroll_offset(
        &self,
        source: DomId,
        vertical: bool,
        value: i32,
        state: &EventState,
        deliveries: &mut Vec<(EventListener<ScrollEvent>, ScrollEvent)>,
    ) -> bool {
        let mut offsets = self.scroll_offsets.lock().expect("scroll mutex poisoned");
        let offset = offsets.entry(source).or_default();
        let before_x = offset.x;
        let before_y = offset.y;
        let target = if vertical {
            &mut offset.y
        } else {
            &mut offset.x
        };
        if *target == value.max(0) {
            return false;
        }
        *target = value.max(0);
        let Some(scroll) = state.region(source).and_then(|region| region.scroll) else {
            return true;
        };
        queue_scroll_event(
            state,
            source,
            ScrollEvent {
                offset_x: offset.x,
                offset_y: offset.y,
                max_x: scroll.max_x.max(0),
                max_y: scroll.max_y.max(0),
                delta_x: offset.x.saturating_sub(before_x),
                delta_y: offset.y.saturating_sub(before_y),
            },
            deliveries,
        );
        true
    }

    fn scroll_to_edge(
        &self,
        state: &EventState,
        target: DomId,
        end: ScrollEdge,
        deliveries: &mut Vec<(EventListener<ScrollEvent>, ScrollEvent)>,
    ) -> bool {
        let mut current = Some(target);
        let mut visited = HashSet::new();
        let mut offsets = self.scroll_offsets.lock().expect("scroll mutex poisoned");
        while let Some(id) = current {
            if !visited.insert(id) {
                break;
            }
            let Some(region) = state.regions.iter().find(|region| region.id == id) else {
                break;
            };
            if let Some(scroll) = region.scroll
                && scroll.vertical
            {
                let offset = offsets.entry(id).or_default();
                let next = match end {
                    ScrollEdge::Start => 0,
                    ScrollEdge::End => scroll.max_y.max(0),
                };
                if offset.y != next {
                    let before = offset.y;
                    offset.y = next;
                    queue_scroll_event(
                        state,
                        id,
                        ScrollEvent {
                            offset_x: offset.x,
                            offset_y: offset.y,
                            max_x: scroll.max_x.max(0),
                            max_y: scroll.max_y.max(0),
                            delta_x: 0,
                            delta_y: offset.y.saturating_sub(before),
                        },
                        deliveries,
                    );
                    return true;
                }
            }
            current = region.parent;
        }
        false
    }

    fn scroll_page(
        &self,
        state: &EventState,
        target: DomId,
        direction: i32,
        deliveries: &mut Vec<(EventListener<ScrollEvent>, ScrollEvent)>,
    ) -> bool {
        let mut current = Some(target);
        let mut visited = HashSet::new();
        let mut offsets = self.scroll_offsets.lock().expect("scroll mutex poisoned");
        while let Some(id) = current {
            if !visited.insert(id) {
                break;
            }
            let Some(region) = state.regions.iter().find(|region| region.id == id) else {
                break;
            };
            if let Some(scroll) = region.scroll
                && scroll.vertical
            {
                let offset = offsets.entry(id).or_default();
                let page = scroll.viewport_height.max(1);
                let next = offset
                    .y
                    .saturating_add(direction.saturating_mul(page))
                    .clamp(0, scroll.max_y.max(0));
                if next != offset.y {
                    let before = offset.y;
                    offset.y = next;
                    queue_scroll_event(
                        state,
                        id,
                        ScrollEvent {
                            offset_x: offset.x,
                            offset_y: offset.y,
                            max_x: scroll.max_x.max(0),
                            max_y: scroll.max_y.max(0),
                            delta_x: 0,
                            delta_y: offset.y.saturating_sub(before),
                        },
                        deliveries,
                    );
                    return true;
                }
            }
            current = region.parent;
        }
        false
    }

    fn dispatch_paste(&self, value: PasteEvent) -> usize {
        let callbacks = {
            let state = self.state.read().expect("event registry poisoned");
            let target = state.focused.or_else(|| state.root());
            target
                .map(|target| state.route(target, |handlers| listener(&handlers.paste_event)))
                .unwrap_or_default()
        };
        let count = callbacks.len();
        for listener in callbacks {
            listener.call(value.clone());
        }
        count
    }

    fn dispatch_resize(&self, event: ResizeEvent) -> usize {
        self.viewport.set(event.size);
        let callbacks = {
            let state = self.state.read().expect("event registry poisoned");
            state
                .regions
                .iter()
                .filter_map(|region| listener(&region.handlers.resize_event))
                .collect::<Vec<EventListener<ResizeEvent>>>()
        };
        let count = callbacks.len();
        for listener in callbacks {
            listener.call(event);
        }
        count
    }

    fn dispatch_terminal_focus(&self, event: FocusEvent) -> usize {
        let mut pointer_deliveries = Vec::new();
        if event == FocusEvent::Lost {
            let mut state = self.state.write().expect("event registry poisoned");
            if let Some(capture) = state.captured.take() {
                let pointer = PointerEvent::new(
                    PointerEventKind::Cancel,
                    state.last_position,
                    PointerButton::None,
                    0,
                    KeyModifiers::empty(),
                );
                queue_pointer(
                    &state,
                    capture.target,
                    PointerHandler::Cancel,
                    pointer,
                    &mut pointer_deliveries,
                );
                queue_pointer(
                    &state,
                    capture.target,
                    PointerHandler::LostCapture,
                    PointerEvent {
                        kind: PointerEventKind::LostCapture,
                        ..pointer
                    },
                    &mut pointer_deliveries,
                );
                state.buttons = 0;
            }
        }
        let callbacks = {
            let state = self.state.read().expect("event registry poisoned");
            state
                .regions
                .iter()
                .filter_map(|region| listener(&region.handlers.focus_event))
                .collect::<Vec<EventListener<FocusEvent>>>()
        };
        let count = pointer_deliveries.len() + callbacks.len();
        for (listener, pointer) in pointer_deliveries {
            listener.call(pointer);
        }
        for listener in callbacks {
            listener.call(event);
        }
        count
    }
}

#[derive(Debug, Clone, Copy)]
enum PointerHandler {
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
    fn for_kind(kind: PointerEventKind) -> Self {
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

fn queue_pointer(
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

fn queue_pointer_direct(
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

fn wheel_delta(kind: MouseEventKind) -> Option<(i16, i16)> {
    match kind {
        MouseEventKind::ScrollUp => Some((0, -1)),
        MouseEventKind::ScrollDown => Some((0, 1)),
        MouseEventKind::ScrollLeft => Some((-1, 0)),
        MouseEventKind::ScrollRight => Some((1, 0)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum ScrollEdge {
    Start,
    End,
}

fn key_scroll(code: crossterm::event::KeyCode) -> (i32, i32, Option<ScrollEdge>, Option<i32>) {
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

fn consume_scroll(offset: i32, delta: i32, max: i32) -> (i32, i32) {
    let next = offset.saturating_add(delta).clamp(0, max.max(0));
    (next, next - offset)
}

fn listener<T: Clone>(slot: &Attr<T>) -> Option<T> {
    slot.clone().into()
}

fn queue_scroll_event(
    state: &EventState,
    target: DomId,
    event: ScrollEvent,
    deliveries: &mut Vec<(EventListener<ScrollEvent>, ScrollEvent)>,
) {
    for listener in state.route_scroll(target) {
        deliveries.push((listener, event));
    }
}

fn pointer_slot(
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
    fn region(&self, id: DomId) -> Option<&EventRegion> {
        self.regions.iter().find(|region| region.id == id)
    }

    fn hit_target(&self, position: ScreenPosition) -> Option<DomId> {
        self.regions
            .iter()
            .filter(|region| region.rect.contains(position.line, position.column))
            .max_by_key(|region| (region.level, region.order))
            .map(|region| region.id)
    }

    fn pointer_listener(
        &self,
        target: DomId,
        handler: PointerHandler,
    ) -> Option<EventListener<PointerEvent>> {
        self.regions
            .iter()
            .find(|region| region.id == target)
            .and_then(|region| pointer_slot(&region.handlers, handler))
    }

    fn route_pointer(
        &self,
        target: DomId,
        handler: PointerHandler,
    ) -> Vec<EventListener<PointerEvent>> {
        self.route(target, |handlers| pointer_slot(handlers, handler))
    }

    fn route_wheel(&self, target: DomId) -> Vec<EventListener<WheelEvent>> {
        self.route(target, |handlers| listener(&handlers.wheel))
    }

    fn route_scroll(&self, target: DomId) -> Vec<EventListener<ScrollEvent>> {
        self.route(target, |handlers| listener(&handlers.scroll))
    }

    fn is_pointer_interactive(&self, target: DomId) -> bool {
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

    fn has_scroll_ancestor(&self, target: DomId) -> bool {
        let mut current = Some(target);
        let mut visited = HashSet::new();
        while let Some(id) = current {
            if !visited.insert(id) {
                return false;
            }
            let Some(region) = self.regions.iter().find(|region| region.id == id) else {
                return false;
            };
            if region.scroll.is_some() {
                return true;
            }
            current = region.parent;
        }
        false
    }

    fn scrollbar_at(
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
            let Some(region) = self.regions.iter().find(|region| region.id == id) else {
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

    fn root(&self) -> Option<DomId> {
        self.regions
            .iter()
            .find(|region| region.parent.is_none())
            .map(|region| region.id)
    }

    fn route<T>(
        &self,
        target: DomId,
        listener: impl Fn(&EventHandlers) -> Option<EventListener<T>>,
    ) -> Vec<EventListener<T>> {
        let mut callbacks = Vec::new();
        let mut current = Some(target);
        let mut visited = HashSet::new();
        while let Some(id) = current {
            if !visited.insert(id) {
                break;
            }
            let Some(region) = self.regions.iter().find(|region| region.id == id) else {
                break;
            };
            if let Some(callback) = listener(&region.handlers) {
                callbacks.push(callback);
            }
            current = region.parent;
        }
        callbacks
    }
}
