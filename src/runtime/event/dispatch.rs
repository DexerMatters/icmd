//! Event dispatch routing for pointer, keyboard, paste, resize, and terminal
//! activation, plus the focus-transition API that owns DOM focus.
#![allow(unused_imports)]

use super::*;

impl EventDispatcher {
    /// Routes one terminal event to its listeners and built-in actions; counts
    /// every listener call in the returned outcome.
    pub fn dispatch(&self, event: Event) -> DispatchOutcome {
        crate::runtime::metrics::note_event_dispatched();
        match event {
            Event::Mouse(event) => self.dispatch_mouse(event),
            Event::Key(event) => self.dispatch_key(KeyboardEvent { key: event }),
            Event::Paste(value) => {
                DispatchOutcome::from_delivered(self.dispatch_paste(PasteEvent {
                    text: std::sync::Arc::from(value),
                }))
            }
            Event::Resize(width, height) => {
                DispatchOutcome::from_delivered(self.dispatch_resize(ResizeEvent {
                    size: Size::new(width, height),
                }))
            }
            Event::FocusGained => {
                DispatchOutcome::from_delivered(self.dispatch_terminal_focus(FocusEvent::Gained))
            }
            Event::FocusLost => {
                DispatchOutcome::from_delivered(self.dispatch_terminal_focus(FocusEvent::Lost))
            }
        }
    }

    /// Moves focus to the region with this DOM id, reporting whether the
    /// transition occurred; unknown or already-focused ids leave focus unchanged.
    pub fn focus(&self, id: DomId) -> bool {
        self.change_focus(id).0
    }

    /// Checked programmatic focus that rejects a fabricated or stale target:
    /// the region must exist in the current generation and be focusable.
    pub fn try_focus(&self, id: DomId) -> Result<FocusOutcome, FocusError> {
        {
            let state = self.state.read().expect("event registry poisoned");
            let Some(region) = state.region(id) else {
                return Err(FocusError::UnknownTarget(id));
            };
            if !region.focusable {
                return Err(FocusError::NotFocusable(id));
            }
            if state.focused == Some(id) {
                return Err(FocusError::AlreadyFocused(id));
            }
        }
        let (changed, delivered) = self.change_focus(id);
        if !changed {
            return Err(FocusError::UnknownTarget(id));
        }
        Ok(FocusOutcome {
            delivered,
            focused: id,
        })
    }

    /// Requests focus for a target that may not be published yet; the request
    /// is granted at the next publication containing the target, the only
    /// moment a focus transition can be observed by the target itself.
    pub(crate) fn request_focus(&self, id: DomId) {
        let mut state = self.state.write().expect("event registry poisoned");
        state.pending_focus = Some(id);
    }

    fn change_focus(&self, id: DomId) -> (bool, usize) {
        let callbacks = {
            let mut state = self.state.write().expect("event registry poisoned");
            if state.region(id).is_none() || state.focused == Some(id) {
                return (false, 0);
            }
            let old = state.focused.replace(id);
            let mut callbacks = Vec::with_capacity(2);
            if let Some(old) = old
                && let Some(region) = state.region(old)
                && let Some(listener) = listener(&region.handlers.focus_event)
            {
                callbacks.push((listener, FocusEvent::Lost));
            }
            if let Some(region) = state.region(id)
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

    /// Removes focus from the current region, reporting whether a region held
    /// focus; notifies that region's focus listener with `Lost`.
    pub fn blur(&self) -> bool {
        let callback = {
            let mut state = self.state.write().expect("event registry poisoned");
            let Some(old) = state.focused.take() else {
                return false;
            };
            state
                .region(old)
                .and_then(|region| listener(&region.handlers.focus_event))
        };
        if let Some(listener) = callback {
            listener.call(FocusEvent::Lost);
        }
        true
    }

    /// Returns the DOM id of the region that currently owns focus, if any.
    pub fn focused(&self) -> Option<DomId> {
        self.state.read().expect("event registry poisoned").focused
    }

    /// Returns and clears the count of indexed region lookups since the last
    /// call, used by tests to prove a deep route is O(route depth).
    #[doc(hidden)]
    pub fn take_id_probe_count(&self) -> u64 {
        self.state
            .read()
            .expect("event registry poisoned")
            .id_probes
            .swap(0, std::sync::atomic::Ordering::Relaxed)
    }

    /// Returns and clears the first callback fault recorded during dispatch,
    /// turning a panic or rejected reentrant delivery into a typed message.
    #[doc(hidden)]
    pub fn take_callback_fault(&self) -> Option<&'static str> {
        crate::basic::events::take_callback_fault().map(|fault| fault.message())
    }

    /// Sets pointer capture on the region with this DOM id, notifying the previous
    /// capture target with lost-capture and the new one with got-capture;
    /// returns whether the region exists and now holds capture.
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

    /// Releases pointer capture held by the region with this DOM id, notifying it
    /// with lost-capture; returns whether that region held capture.
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

    /// Returns the DOM id of the region that currently holds pointer capture, if any.
    pub fn pointer_capture(&self) -> Option<DomId> {
        self.state
            .read()
            .expect("event registry poisoned")
            .captured
            .map(|capture| capture.target)
    }

    /// Dispatches a mouse event: capture listeners run root-to-target before the
    /// target/bubble phase and built-in actions, and stopping them suppresses both.
    pub(super) fn dispatch_mouse(&self, event: MouseEvent) -> DispatchOutcome {
        let capture_outcome = self.dispatch_pointer_capture(event);
        if capture_outcome.propagation_stopped {
            return capture_outcome;
        }
        let mut pointer_deliveries = Vec::new();
        let mut wheel_deliveries = Vec::new();
        let mut scroll_deliveries = Vec::new();
        let mut focus_target = None;
        let mut scroll_changed = false;
        let wheel_offsets_before = wheel_delta(event.kind).map(|_| {
            self.scroll_offsets
                .lock()
                .expect("scroll mutex poisoned")
                .clone()
        });

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
                                position.line.saturating_sub(region.origin.line),
                                position.column.saturating_sub(region.origin.column),
                            )
                        }),
                );

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
                                .region(source)
                                .and_then(|region| region.scroll)
                                .is_some_and(|scroll| scroll.enable_mouse);
                            if enable_mouse {
                                if bar.contains_thumb(position) {
                                    state.drag = Some(ScrollDrag {
                                        source,
                                        bar,
                                        grab_offset: bar.coordinate(position).saturating_sub(
                                            bar.start().saturating_add(bar.thumb_start()),
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
                            let travel = drag.bar.length().saturating_sub(drag.bar.thumb_len());
                            if travel > 0 && drag.bar.max_offset() > 0 {
                                let desired_start = coordinate
                                    .saturating_sub(track_start)
                                    .saturating_sub(drag.grab_offset)
                                    .clamp(0, travel);
                                let next = ((desired_start as i64 * drag.bar.max_offset() as i64
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

                if matches!(kind, PointerEventKind::Down) {
                    focus_target = normal_target.and_then(|target| state.focus_target_for(target));
                }
            }
        }

        let dispatch = crate::basic::events::begin_dispatch();
        let mut count = 0;
        for (listener, event) in pointer_deliveries {
            listener.call(event);
            count += 1;
            if crate::basic::events::propagation_stopped() {
                break;
            }
        }
        if !crate::basic::events::propagation_stopped() {
            for (listener, event) in wheel_deliveries {
                listener.call(event);
                count += 1;
                if crate::basic::events::propagation_stopped() {
                    break;
                }
            }
        }
        if !crate::basic::events::propagation_stopped() {
            for (listener, event) in scroll_deliveries {
                listener.call(event);
                count += 1;
                if crate::basic::events::propagation_stopped() {
                    break;
                }
            }
        }
        let stopped = crate::basic::events::propagation_stopped();
        let prevented = crate::basic::events::default_prevented();
        drop(dispatch);

        if prevented && let Some(before) = wheel_offsets_before {
            *self.scroll_offsets.lock().expect("scroll mutex poisoned") = before;
            scroll_changed = false;
        }
        if scroll_changed && !prevented {
            self.viewport.request_redraw();
        }
        if let Some(target) = focus_target
            && !prevented
        {
            count += self.change_focus(target).1;
        }
        DispatchOutcome {
            delivered: count + capture_outcome.delivered,
            propagation_stopped: stopped,
            default_prevented: prevented || capture_outcome.default_prevented,
            redraw_requested: scroll_changed && !prevented,
        }
    }

    /// Runs capture listeners for the event's family, in root-to-target order.
    pub(super) fn dispatch_pointer_capture(&self, event: MouseEvent) -> DispatchOutcome {
        let position = ScreenPosition::new(event.row as i32, event.column as i32);
        let (pointer_events, wheel_events) = {
            let state = self.state.read().expect("event registry poisoned");
            let Some(target) = state.hit_target(position) else {
                return DispatchOutcome::default();
            };
            let pointer =
                match mouse_details(event).0 {
                    PointerEventKind::Down => state
                        .route_capture(target, |handlers| listener(&handlers.pointer_down_capture)),
                    PointerEventKind::Up => state
                        .route_capture(target, |handlers| listener(&handlers.pointer_up_capture)),
                    PointerEventKind::Click => {
                        state.route_capture(target, |handlers| listener(&handlers.click_capture))
                    }
                    _ => Vec::new(),
                };
            let wheel = if wheel_delta(event.kind).is_some() {
                state.route_capture(target, |handlers| listener(&handlers.wheel_capture))
            } else {
                Vec::new()
            };
            (pointer, wheel)
        };
        if pointer_events.is_empty() && wheel_events.is_empty() {
            return DispatchOutcome::default();
        }

        let dispatch = crate::basic::events::begin_dispatch();
        let mut delivered = 0;
        if !pointer_events.is_empty() {
            let pointer = PointerEvent::new(
                mouse_details(event).0,
                position,
                mouse_details(event).1,
                0,
                event.modifiers,
            );
            for listener in pointer_events {
                listener.call(pointer);
                delivered += 1;
                if crate::basic::events::propagation_stopped() {
                    break;
                }
            }
        }
        if !crate::basic::events::propagation_stopped()
            && let Some((delta_x, delta_y)) = wheel_delta(event.kind)
        {
            let wheel = WheelEvent {
                position,
                delta_x,
                delta_y,
                modifiers: event.modifiers,
            };
            for listener in wheel_events {
                listener.call(wheel);
                delivered += 1;
                if crate::basic::events::propagation_stopped() {
                    break;
                }
            }
        }
        let stopped = crate::basic::events::propagation_stopped();
        let prevented = crate::basic::events::default_prevented();
        drop(dispatch);
        DispatchOutcome {
            delivered,
            propagation_stopped: stopped,
            default_prevented: prevented,
            redraw_requested: false,
        }
    }

    /// Dispatches a key event: capture listeners run first, then target-specific
    /// and application-global shortcuts, then built-in key scrolling.
    pub(super) fn dispatch_key(&self, event: KeyboardEvent) -> DispatchOutcome {
        let capture_outcome = self.dispatch_key_capture(event);
        if capture_outcome.propagation_stopped {
            return capture_outcome;
        }
        let mut scroll_changed = false;
        let mut scroll_deliveries = Vec::new();
        let (target, keyboard_callbacks, key_callbacks, app_callbacks) = {
            let state = self.state.read().expect("event registry poisoned");
            let app_callbacks = state
                .regions
                .iter()
                .filter_map(|region| listener(&region.handlers.app_key))
                .collect::<Vec<_>>();
            let (target, keyboard_callbacks, key_callbacks) = match state.focused {
                None => (None, Vec::new(), Vec::new()),
                Some(target) => {
                    let keyboard_callbacks =
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
                    (Some(target), keyboard_callbacks, key_callbacks)
                }
            };
            (target, keyboard_callbacks, key_callbacks, app_callbacks)
        };

        let dispatch = crate::basic::events::begin_dispatch();
        let mut count = 0;
        for listener in key_callbacks {
            listener.call(event);
            count += 1;
            if crate::basic::events::propagation_stopped() {
                break;
            }
        }
        if !crate::basic::events::propagation_stopped() {
            for listener in keyboard_callbacks {
                listener.call(event);
                count += 1;
                if crate::basic::events::propagation_stopped() {
                    break;
                }
            }
        }
        let shortcut_kind = matches!(
            event.key.kind,
            crossterm::event::KeyEventKind::Press | crossterm::event::KeyEventKind::Repeat
        );
        if shortcut_kind && !crate::basic::events::propagation_stopped() {
            for listener in app_callbacks {
                listener.call(event);
                count += 1;
                if crate::basic::events::propagation_stopped() {
                    break;
                }
            }
        }
        let consumed = crate::basic::events::propagation_stopped();
        let prevented = crate::basic::events::default_prevented();
        drop(dispatch);

        if !consumed
            && !prevented
            && let Some(target) = target
            && matches!(
                event.key.kind,
                crossterm::event::KeyEventKind::Press | crossterm::event::KeyEventKind::Repeat
            )
            && event.key.modifiers.is_empty()
        {
            let state = self.state.read().expect("event registry poisoned");
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
                scroll_changed |= self.scroll_to_edge(&state, target, edge, &mut scroll_deliveries);
            }
            if let Some(page) = page {
                scroll_changed |= self.scroll_page(&state, target, page, &mut scroll_deliveries);
            }
        }
        for (listener, event) in scroll_deliveries {
            listener.call(event);
        }
        if scroll_changed {
            self.viewport.request_redraw();
        }
        DispatchOutcome {
            delivered: count + capture_outcome.delivered,
            propagation_stopped: consumed,
            default_prevented: prevented || capture_outcome.default_prevented,
            redraw_requested: scroll_changed && !prevented,
        }
    }

    /// Runs capture listeners for a key routed at the focused target.
    pub(super) fn dispatch_key_capture(&self, event: KeyboardEvent) -> DispatchOutcome {
        let callbacks = {
            let state = self.state.read().expect("event registry poisoned");
            let Some(target) = state.focused else {
                return DispatchOutcome::default();
            };
            if event.key.kind == crossterm::event::KeyEventKind::Release {
                state.route_capture(target, |handlers| listener(&handlers.key_up_capture))
            } else {
                state.route_capture(target, |handlers| listener(&handlers.key_down_capture))
            }
        };
        if callbacks.is_empty() {
            return DispatchOutcome::default();
        }
        let dispatch = crate::basic::events::begin_dispatch();
        let mut delivered = 0;
        for listener in callbacks {
            listener.call(event);
            delivered += 1;
            if crate::basic::events::propagation_stopped() {
                break;
            }
        }
        let stopped = crate::basic::events::propagation_stopped();
        let prevented = crate::basic::events::default_prevented();
        drop(dispatch);
        DispatchOutcome {
            delivered,
            propagation_stopped: stopped,
            default_prevented: prevented,
            redraw_requested: false,
        }
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
            let Some(region) = state.region(id) else {
                break;
            };
            if let Some(scroll) = region.scroll {
                if (input == ScrollInput::Wheel && !scroll.wheel)
                    || (input == ScrollInput::Keyboard && !scroll.enable_keyboard)
                {
                    current = region.parent;
                    continue;
                }
                let offset = offsets.entry(id).or_default();
                let before = *offset;
                let mut next = before;
                let step = if input == ScrollInput::Wheel {
                    i32::from(scroll.wheel_step.max(1))
                } else {
                    1
                };
                let requested_x = remaining_x.saturating_mul(step);
                let requested_y = remaining_y.saturating_mul(step);
                let (x, consumed_x) = consume_scroll(next.x, requested_x, scroll.max_x);
                let (y, consumed_y) = consume_scroll(next.y, requested_y, scroll.max_y);
                if scroll.horizontal {
                    if x != next.x {
                        next.x = x;
                        changed |= !scroll.controlled;
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
                    if y != next.y {
                        next.y = y;
                        changed |= !scroll.controlled;
                    }
                    if consumed_y != 0 {
                        remaining_y = if consumed_y.abs() < requested_y.abs() {
                            requested_y.signum()
                        } else {
                            0
                        };
                    }
                }
                if before.x != next.x || before.y != next.y {
                    if !scroll.controlled {
                        *offset = next;
                    }
                    queue_scroll_event(
                        state,
                        id,
                        make_scroll_event(next, scroll.max_x, scroll.max_y, before),
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
        let travel = bar.length().saturating_sub(bar.thumb_len());
        if travel <= 0 || bar.max_offset() <= 0 {
            return false;
        }
        let desired_start = bar
            .coordinate(position)
            .saturating_sub(bar.start())
            .saturating_sub(bar.thumb_len() / 2)
            .clamp(0, travel);
        let next = ((desired_start as i64 * bar.max_offset() as i64 + travel as i64 / 2)
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
        let before = *offset;
        let mut next = before;
        let target = if vertical { &mut next.y } else { &mut next.x };
        *target = value.max(0);
        if *target == if vertical { before.y } else { before.x } {
            return false;
        }
        let Some(scroll) = state.region(source).and_then(|region| region.scroll) else {
            return true;
        };
        if !scroll.controlled {
            *offset = next;
        }
        queue_scroll_event(
            state,
            source,
            make_scroll_event(next, scroll.max_x, scroll.max_y, before),
            deliveries,
        );
        !scroll.controlled
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
            let Some(region) = state.region(id) else {
                break;
            };
            if let Some(scroll) = region.scroll
                && scroll.vertical
                && scroll.enable_keyboard
            {
                let offset = offsets.entry(id).or_default();
                let next = match end {
                    ScrollEdge::Start => 0,
                    ScrollEdge::End => scroll.max_y.max(0),
                };
                if offset.y != next {
                    let before = offset.y;
                    let mut proposed = *offset;
                    proposed.y = next;
                    if !scroll.controlled {
                        offset.y = next;
                    }
                    queue_scroll_event(
                        state,
                        id,
                        make_scroll_event(
                            proposed,
                            scroll.max_x,
                            scroll.max_y,
                            RuntimeScrollOffset {
                                x: offset.x,
                                y: before,
                            },
                        ),
                        deliveries,
                    );
                    return !scroll.controlled;
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
            let Some(region) = state.region(id) else {
                break;
            };
            if let Some(scroll) = region.scroll
                && scroll.vertical
                && scroll.enable_keyboard
            {
                let offset = offsets.entry(id).or_default();
                let page = scroll.viewport_height.max(1);
                let next = offset
                    .y
                    .saturating_add(direction.saturating_mul(page))
                    .clamp(0, scroll.max_y.max(0));
                if next != offset.y {
                    let before = offset.y;
                    let mut proposed = *offset;
                    proposed.y = next;
                    if !scroll.controlled {
                        offset.y = next;
                    }
                    queue_scroll_event(
                        state,
                        id,
                        make_scroll_event(
                            proposed,
                            scroll.max_x,
                            scroll.max_y,
                            RuntimeScrollOffset {
                                x: offset.x,
                                y: before,
                            },
                        ),
                        deliveries,
                    );
                    return !scroll.controlled;
                }
            }
            current = region.parent;
        }
        false
    }

    /// Routes a paste event only to the focused region's paste listeners; an
    /// unfocused input never accepts it and there is no global paste route.
    pub(super) fn dispatch_paste(&self, value: PasteEvent) -> usize {
        let callbacks = {
            let state = self.state.read().expect("event registry poisoned");
            state
                .focused
                .map(|target| state.route(target, |handlers| listener(&handlers.paste_event)))
                .unwrap_or_default()
        };
        let count = callbacks.len();
        for listener in callbacks {
            listener.call(value.clone());
        }
        count
    }

    pub(super) fn dispatch_resize(&self, event: ResizeEvent) -> usize {
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

    pub(super) fn dispatch_terminal_focus(&self, event: FocusEvent) -> usize {
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
                .filter_map(|region| listener(&region.handlers.terminal_focus))
                .collect::<Vec<EventListener<TerminalFocusEvent>>>()
        };
        let event = match event {
            FocusEvent::Gained => TerminalFocusEvent::Gained,
            FocusEvent::Lost => TerminalFocusEvent::Lost,
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
