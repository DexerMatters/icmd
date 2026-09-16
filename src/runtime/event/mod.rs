//! Event dispatch state and public result types for the runtime event system.
//! Owns the region registry, focus and pointer-capture state, and the scroll
//! offsets shared across pointer, keyboard, paste, and resize routing.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};

use crate::{
    DomId, ScreenPosition, Size,
    basic::{
        EventHandlers, EventListener, FocusEvent, KeyboardEvent, PasteEvent, PointerButton,
        PointerEvent, PointerEventKind, ResizeEvent, ScrollDelta, ScrollEvent, ScrollOffset,
        TerminalFocusEvent, WheelEvent, common::Attr, events::mouse_details,
    },
};

use super::commit::ViewportSetter;
use super::commit::layout::ScrollbarMetrics;

/// Result of an accepted focus request: the number of focus listeners that
/// observed the transition and the region that now owns focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusOutcome {
    /// Count of focus listeners that observed the transition.
    pub delivered: usize,
    /// DOM id of the region that now owns focus.
    pub focused: DomId,
}

/// Reason a programmatic focus request was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusError {
    /// No published region carries the requested DOM id.
    UnknownTarget(DomId),
    /// The region with this DOM id exists but is not focusable.
    NotFocusable(DomId),
    /// The region with this DOM id already owns focus.
    AlreadyFocused(DomId),
}

impl std::fmt::Display for FocusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTarget(id) => write!(f, "no region with id {id}"),
            Self::NotFocusable(id) => write!(f, "region {id} is not focusable"),
            Self::AlreadyFocused(id) => write!(f, "region {id} already owns focus"),
        }
    }
}

impl std::error::Error for FocusError {}

/// Observable result of one dispatch, replacing a bare delivery count with
/// explicit propagation, default-action, and redraw signals a caller can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DispatchOutcome {
    /// Count of listeners that received the event.
    pub delivered: usize,
    /// Whether a listener stopped propagation.
    pub propagation_stopped: bool,
    /// Whether a listener prevented the built-in default action.
    pub default_prevented: bool,
    /// Whether a scroll changed and a redraw was requested.
    pub redraw_requested: bool,
}

impl DispatchOutcome {
    pub(crate) fn from_delivered(delivered: usize) -> Self {
        Self {
            delivered,
            ..Self::default()
        }
    }
}

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
    pub(crate) focusable: bool,
    pub(crate) autofocus: bool,
    pub(crate) rect: EventRect,
    pub(crate) origin: ScreenPosition,
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
    pub(crate) enable_keyboard: bool,
    pub(crate) controlled: bool,
    pub(crate) vertical_bar: Option<ScrollbarRegion>,
    pub(crate) horizontal_bar: Option<ScrollbarRegion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarRegion {
    pub(crate) line: i32,
    pub(crate) column: i32,
    pub(crate) metrics: ScrollbarMetrics,
    pub(crate) vertical: bool,
}

impl ScrollbarRegion {
    pub(crate) fn contains_track(self, position: ScreenPosition) -> bool {
        if self.vertical {
            position.column == self.column
                && position.line >= self.line
                && position.line < self.line.saturating_add(self.metrics.length)
        } else {
            position.line == self.line
                && position.column >= self.column
                && position.column < self.column.saturating_add(self.metrics.length)
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
        let thumb_start = start.saturating_add(self.metrics.thumb_start);
        coordinate >= thumb_start && coordinate < thumb_start.saturating_add(self.metrics.thumb_len)
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

    fn length(self) -> i32 {
        self.metrics.length
    }

    fn thumb_start(self) -> i32 {
        self.metrics.thumb_start
    }

    fn thumb_len(self) -> i32 {
        self.metrics.thumb_len
    }

    fn max_offset(self) -> i32 {
        self.metrics.max_offset
    }
}

pub(crate) fn scrollbar_region(
    line: i32,
    column: i32,
    metrics: ScrollbarMetrics,
    vertical: bool,
) -> ScrollbarRegion {
    ScrollbarRegion {
        line,
        column,
        metrics,
        vertical,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeScrollOffset {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Default)]
struct EventState {
    regions: Vec<EventRegion>,
    by_id: HashMap<DomId, usize>,
    id_probes: std::sync::atomic::AtomicU64,
    focused: Option<DomId>,
    captured: Option<PointerCapture>,
    hovered: Option<DomId>,
    buttons: u16,
    last_position: ScreenPosition,
    drag: Option<ScrollDrag>,
    pending_focus: Option<DomId>,
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

/// Shared handle to the event registry, viewport, and per-region scroll
/// offsets; routes events and owns DOM focus and pointer capture.
#[derive(Clone)]
pub struct EventDispatcher {
    state: Arc<RwLock<EventState>>,
    viewport: ViewportSetter,
    scroll_offsets: Arc<Mutex<HashMap<DomId, RuntimeScrollOffset>>>,
}

impl EventDispatcher {
    pub(crate) fn new(viewport: ViewportSetter) -> Self {
        Self {
            state: Arc::new(RwLock::new(EventState::default())),
            viewport,
            scroll_offsets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn scroll_offsets(&self) -> Arc<Mutex<HashMap<DomId, RuntimeScrollOffset>>> {
        self.scroll_offsets.clone()
    }

    pub(crate) fn publish(
        &self,
        mut regions: Vec<EventRegion>,
        retained_scroll_ids: &HashSet<DomId>,
    ) {
        regions.sort_by_key(|region| region.order);
        let mut capture_deliveries = Vec::new();
        let (lost_focus, gained_focus) = {
            let mut state = self.state.write().expect("event registry poisoned");
            let lost_focus = if state
                .focused
                .is_some_and(|id| !regions.iter().any(|region| region.id == id))
            {
                let old = state.focused.take();
                old.and_then(|id| {
                    state
                        .region(id)
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
            state.by_id = regions
                .iter()
                .enumerate()
                .map(|(index, region)| (region.id, index))
                .collect();
            state.regions = regions;
            let gained_focus = match state.pending_focus.take() {
                Some(pending)
                    if state.focused.is_none()
                        && state
                            .regions
                            .iter()
                            .any(|region| region.id == pending && region.focusable) =>
                {
                    state.focused = Some(pending);
                    state
                        .region(pending)
                        .and_then(|region| listener(&region.handlers.focus_event))
                }
                _ => None,
            };
            (lost_focus, gained_focus)
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
        if let Some(listener) = gained_focus {
            listener.call(FocusEvent::Gained);
        }
    }
}

mod dispatch;
mod routing;
use routing::*;
