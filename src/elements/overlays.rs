//! Overlay primitives: portals, dialogs, anchored popovers, and menus.
//!
//! Every overlay paints through a portal hoisted to the frame root, so an
//! ancestor's clipping and stacking cannot hide it. A portal host is pointer
//! transparent; the visible card and any modal scrim opt back into hit testing.

use crossterm::event::KeyCode;

use crate::{
    Attr, AxisPosition, BackgroundBlend, ComponentContext, Dimension, DomId, DomProps, Edges,
    ElementObserver, ElementRef, EventListener, Key, KeyboardEvent, Layout, Node, PointerEvent,
    PointerEvents, Props, Text, TextStyle,
};

/// Configuration for [`portal`].
#[derive(Clone, Debug, Default)]
pub struct PortalProps {
    /// Paint order above ordinary content; defaults to `1000`.
    pub z_index: Attr<i32>,
}

/// Paints its children at the frame root, outside every ancestor's clip.
///
/// The portal host itself is pointer transparent, so only the children that
/// opt into pointer events can be hit. Portals paint in declaration order.
pub fn portal(cx: &mut ComponentContext, props: &Props<PortalProps>) -> Node {
    let _ = cx;
    let mut dom = props.host_props(DomProps::default());
    dom.portal = true;
    dom.style.layout.set_default(Layout::Absolute);
    dom.style.width.set_default(Dimension::Max);
    dom.style.height.set_default(Dimension::Max);
    dom.style.line.set_default(AxisPosition::Start);
    dom.style.column.set_default(AxisPosition::Start);
    dom.style.pointer_events.set_default(PointerEvents::None);
    dom.style.z_index.set_default(props.z_index | 1000);
    Node::element(dom, props.children.clone())
}

/// Configuration for [`dialog`].
#[derive(Clone, Default)]
pub struct DialogProps {
    /// Whether the dialog is painted; defaults to `false`.
    pub open: Attr<bool>,
    /// Title shown in the dialog header.
    pub title: Attr<String>,
    /// Whether outside presses are blocked by a scrim; defaults to `true`.
    pub modal: Attr<bool>,
    /// Whether an outside press closes the dialog; defaults to `true`.
    pub dismiss_on_outside: Attr<bool>,
    /// Listener invoked by Escape, the scrim, or an outside press.
    pub on_close: Attr<EventListener<()>>,
    /// Element focused after the next commit; otherwise the card takes focus.
    pub initial_focus: Attr<ElementRef>,
}

impl std::fmt::Debug for DialogProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogProps")
            .field("open", &self.open)
            .field("title", &self.title)
            .field("modal", &self.modal)
            .field("dismiss_on_outside", &self.dismiss_on_outside)
            .finish_non_exhaustive()
    }
}

/// A centered, focus-owning card over the current content.
pub fn dialog(cx: &mut ComponentContext, props: &Props<DialogProps>) -> Node {
    let theme = cx.use_theme();
    let open = props.open | false;
    let handle = cx.use_handle();
    let saved = cx.use_ref(|| None::<DomId>);
    let card_ref = cx.use_element_ref();
    let initial_observer = cx.use_ref(|| None::<(ElementRef, ElementObserver)>);
    let refresh = cx.use_live_state(|| 0_u64);
    let title = props.title.clone() | String::new();
    let modal = props.modal | true;
    let dismiss = props.dismiss_on_outside | true;
    let on_close = props.on_close.as_ref().cloned();

    if open && saved.lock().expect("dialog focus poisoned").is_none() {
        *saved.lock().expect("dialog focus poisoned") = handle.focused();
    }

    /*
     * The cleanup runs when `open` changes and at unmount; restoring there
     * means a closed dialog never leaves focus on an unmounted region.
     */
    {
        let handle = handle.clone();
        let saved = saved.clone();
        cx.use_effect(open, move || {
            move || {
                if let Some(id) = saved.lock().expect("dialog focus poisoned").take() {
                    let _ = handle.focus(id);
                }
            }
        });
    }

    let initial_ref = props.initial_focus.as_ref().cloned();
    if open {
        if let Some(initial_ref) = initial_ref.as_ref() {
            let mut slot = initial_observer
                .lock()
                .expect("dialog focus observer poisoned");
            let stale = slot
                .as_ref()
                .is_none_or(|(registered, _)| registered != initial_ref);
            if stale {
                let refresh = refresh.clone();
                let observer = initial_ref.observe(move |_snapshot| {
                    let _ = refresh.update(|value| *value = value.wrapping_add(1));
                });
                *slot = Some((initial_ref.clone(), observer));
            }
        }
    } else {
        *initial_observer
            .lock()
            .expect("dialog focus observer poisoned") = None;
    }
    let initial = initial_ref
        .as_ref()
        .and_then(ElementRef::current)
        .map(|snapshot| snapshot.id());
    let focus_handle = handle.clone();
    cx.use_effect((open, initial), move || {
        if open && let Some(id) = initial {
            let _ = focus_handle.focus(id);
        }
    });

    if !open {
        return crate::empty();
    }

    let close_listener = on_close.clone();

    let mut scrim = DomProps::default();
    scrim.style.layout.set_default(Layout::Absolute);
    scrim.style.line.set_default(AxisPosition::Start);
    scrim.style.column.set_default(AxisPosition::Start);
    scrim.style.width.set_default(Dimension::Max);
    scrim.style.height.set_default(Dimension::Max);
    scrim.style.background.set_default(theme.roles.overlay);
    scrim
        .style
        .background_blend
        .set_default(BackgroundBlend::Cover);
    scrim.style.z_index.set_default(0);
    if dismiss && let Some(listener) = close_listener.clone() {
        let handle = handle.clone();
        scrim.events.pointer_down = Attr::Set(EventListener::new(move |event: PointerEvent| {
            event.stop_propagation();
            listener.call(());
            handle.request_frame();
        }));
    } else if modal {
        /* A modal scrim always blocks, even when outside presses do not close. */
        scrim.events.pointer_down = Attr::Set(EventListener::new(|event: PointerEvent| {
            event.stop_propagation();
        }));
    } else {
        scrim.style.pointer_events = Attr::Set(PointerEvents::None);
    }

    let width = (cx.use_viewport().width.saturating_sub(4)).clamp(12, 72);
    let mut card = DomProps::default();
    card.style.layout.set_default(Layout::Vertical);
    card.style.width.set_default(Dimension::Cells(width));
    card.style.height.set_default(Dimension::Auto);
    card.style.line.set_default(AxisPosition::Center);
    card.style.column.set_default(AxisPosition::Center);
    card.style.padding.set_default(Edges::symmetric(1, 2));
    card.style.gap.set_default(1);
    card.style.background.set_default(theme.colors.popover);
    card.style
        .text
        .foreground
        .set_default(theme.colors.popover_foreground);
    card.style.border.kind.set_default(theme.borders.kind);
    card.style.border.edges.set_default(theme.borders.edges);
    card.style
        .border
        .foreground
        .set_default(theme.colors.border);
    card.style
        .border
        .background
        .set_default(theme.colors.popover);
    card.style.z_index.set_default(1);
    card.focusable = Attr::Set(true);
    card.autofocus = true;
    card.element_ref = Attr::Set(card_ref.clone());
    if let Some(listener) = close_listener.clone() {
        let handle = handle.clone();
        card.events.app_key = Attr::Set(EventListener::new(move |event: KeyboardEvent| {
            if event.key.code == KeyCode::Esc {
                event.stop_propagation();
                listener.call(());
                handle.request_frame();
            }
        }));
    }

    if !modal {
        let handle = handle.clone();
        let close = close_listener.clone();
        let card_ref = card_ref.clone();
        card.events.app_pointer_capture =
            Attr::Set(EventListener::new(move |event: PointerEvent| {
                if !dismiss {
                    return;
                }
                let Some(snapshot) = card_ref.current() else {
                    return;
                };
                let rect = snapshot.bounding_rect();
                let inside = event.position.line >= rect.line
                    && event.position.line < rect.bottom()
                    && event.position.column >= rect.column
                    && event.position.column < rect.right();
                if !inside && let Some(listener) = &close {
                    listener.call(());
                    handle.request_frame();
                }
            }));
    }

    let mut portal_children = Vec::new();
    if modal || dismiss {
        portal_children.push(Node::element(scrim, []));
    }
    let mut body = Vec::new();
    if !title.is_empty() {
        let mut heading = Text::new(title);
        heading = heading.text_style(theme.typography.heading.clone());
        body.push(heading.into());
    }
    body.push(props.children_node());
    portal_children.push(Node::element(card, body));
    portal(
        cx,
        &Props::new(PortalProps {
            z_index: Attr::Set(1100),
        })
        .children(portal_children),
    )
}

/// Preferred placement of a popover relative to its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum PopoverPlacement {
    /// Below the anchor, left edges aligned.
    #[default]
    BelowStart,
    /// Below the anchor, right edges aligned.
    BelowEnd,
    /// Above the anchor, left edges aligned.
    AboveStart,
    /// Above the anchor, right edges aligned.
    AboveEnd,
    /// To the right of the anchor, top edges aligned.
    Right,
    /// To the left of the anchor, top edges aligned.
    Left,
}

/// Configuration for [`popover`].
#[derive(Clone, Default)]
pub struct PopoverProps {
    /// Whether the popover is painted; defaults to `false`.
    pub open: Attr<bool>,
    /// The element the popover is anchored to.
    pub anchor: Attr<ElementRef>,
    /// Preferred placement; defaults to [`PopoverPlacement::BelowStart`].
    pub placement: Attr<PopoverPlacement>,
    /// Gap in cells between anchor and popover; defaults to `1`.
    pub offset: Attr<u16>,
    /// Whether an outside press closes the popover; defaults to `true`.
    pub dismiss_on_outside: Attr<bool>,
    /// Whether the popover takes focus on open; defaults to `false`.
    pub take_focus: Attr<bool>,
    /// Listener invoked by an outside press or Escape.
    pub on_close: Attr<EventListener<()>>,
}

impl std::fmt::Debug for PopoverProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopoverProps")
            .field("open", &self.open)
            .field("placement", &self.placement)
            .field("offset", &self.offset)
            .field("take_focus", &self.take_focus)
            .finish_non_exhaustive()
    }
}

/// A pointer-anchored card painted through a portal.
pub fn popover(cx: &mut ComponentContext, props: &Props<PopoverProps>) -> Node {
    let theme = cx.use_theme();
    let open = props.open | false;
    let handle = cx.use_handle();
    let saved = cx.use_ref(|| None::<DomId>);
    let card_ref = cx.use_element_ref();
    let observer_slot = cx.use_ref(|| None::<(ElementRef, ElementObserver)>);
    let refresh = cx.use_live_state(|| 0_u64);
    let viewport = cx.use_viewport();
    if open && saved.lock().expect("popover focus poisoned").is_none() {
        *saved.lock().expect("popover focus poisoned") = handle.focused();
    }
    {
        let handle = handle.clone();
        let saved = saved.clone();
        cx.use_effect(open, move || {
            move || {
                if let Some(id) = saved.lock().expect("popover focus poisoned").take() {
                    let _ = handle.focus(id);
                }
            }
        });
    }
    if !open {
        *observer_slot.lock().expect("popover anchor state poisoned") = None;
        return crate::empty();
    }
    if let Some(anchor_ref) = props.anchor.as_ref() {
        let mut slot = observer_slot.lock().expect("popover anchor state poisoned");
        let stale = slot
            .as_ref()
            .is_none_or(|(registered, _)| registered != anchor_ref);
        if stale {
            let refresh = refresh.clone();
            let observer = anchor_ref.observe(move |_snapshot| {
                let _ = refresh.update(|value| *value = value.wrapping_add(1));
            });
            *slot = Some((anchor_ref.clone(), observer));
        }
    }
    let Some(anchor) = props.anchor.as_ref().and_then(ElementRef::current) else {
        return crate::empty();
    };
    let anchor = anchor.bounding_rect();
    let placement = props.placement | PopoverPlacement::BelowStart;
    let offset = props.offset | 1;
    let dismiss = props.dismiss_on_outside | true;
    let take_focus = props.take_focus | false;
    let on_close = props.on_close.as_ref().cloned();

    let measured = card_ref.current().map(|snapshot| snapshot.bounding_rect());
    let width = measured
        .map(|rect| rect.width as i32)
        .filter(|width| *width > 0)
        .unwrap_or(24);
    let height = measured
        .map(|rect| rect.height as i32)
        .filter(|height| *height > 0)
        .unwrap_or(1);

    let (mut line, mut column) = match placement {
        PopoverPlacement::BelowStart => (anchor.bottom() + i32::from(offset), anchor.column),
        PopoverPlacement::BelowEnd => (
            anchor.bottom() + i32::from(offset),
            anchor.right().saturating_sub(width),
        ),
        PopoverPlacement::AboveStart => (anchor.line - i32::from(offset) - height, anchor.column),
        PopoverPlacement::AboveEnd => (
            anchor.line - i32::from(offset) - height,
            anchor.right().saturating_sub(width),
        ),
        PopoverPlacement::Right => (anchor.line, anchor.right() + i32::from(offset)),
        PopoverPlacement::Left => (anchor.line, anchor.column - i32::from(offset) - width),
    };
    /* Flip when the preferred side does not fit. */
    if line + height > viewport.height as i32
        && matches!(
            placement,
            PopoverPlacement::BelowStart | PopoverPlacement::BelowEnd
        )
    {
        line = anchor.line - i32::from(offset) - height;
    } else if line < 0
        && matches!(
            placement,
            PopoverPlacement::AboveStart | PopoverPlacement::AboveEnd
        )
    {
        line = anchor.bottom() + i32::from(offset);
    }
    if column + width > viewport.width as i32 && matches!(placement, PopoverPlacement::Right) {
        column = anchor.column - i32::from(offset) - width;
    } else if column < 0 && matches!(placement, PopoverPlacement::Left) {
        column = anchor.right() + i32::from(offset);
    }
    line = line.clamp(0, (viewport.height as i32 - height).max(0));
    column = column.clamp(0, (viewport.width as i32 - width).max(0));

    let mut card = props.host_props(DomProps::default());
    card.style.layout.set_default(Layout::Vertical);
    card.style.width.set_default(Dimension::Cells(24));
    card.style.height.set_default(Dimension::Auto);
    card.style.line.set_default(AxisPosition::Cells(line));
    card.style.column.set_default(AxisPosition::Cells(column));
    card.style.padding.set_default(Edges::symmetric(1, 1));
    card.style.gap.set_default(0);
    card.style.background.set_default(theme.colors.popover);
    card.style
        .text
        .foreground
        .set_default(theme.colors.popover_foreground);
    card.style.border.kind.set_default(theme.borders.kind);
    card.style.border.edges.set_default(theme.borders.edges);
    card.style
        .border
        .foreground
        .set_default(theme.colors.border);
    card.style
        .border
        .background
        .set_default(theme.colors.popover);
    card.style.z_index.set_default(1);
    card.element_ref = Attr::Set(card_ref.clone());
    if take_focus {
        card.autofocus = true;
        card.focusable = Attr::Set(true);
    }
    let refresh_for_change = refresh.clone();
    card.element_change = Attr::Set(EventListener::new(move |_snapshot| {
        let _ = refresh_for_change.update(|value| *value += 1);
    }));

    if dismiss && let Some(listener) = on_close.clone() {
        let card_ref = card_ref.clone();
        let handle = handle.clone();
        card.events.app_pointer_capture =
            Attr::Set(EventListener::new(move |event: PointerEvent| {
                let Some(snapshot) = card_ref.current() else {
                    return;
                };
                let rect = snapshot.bounding_rect();
                let inside = event.position.line >= rect.line
                    && event.position.line < rect.bottom()
                    && event.position.column >= rect.column
                    && event.position.column < rect.right();
                if !inside {
                    listener.call(());
                    handle.request_frame();
                }
            }));
    }
    if let Some(listener) = on_close.clone() {
        let handle = handle.clone();
        card.events.app_key = Attr::Set(EventListener::new(move |event: KeyboardEvent| {
            if event.key.code == KeyCode::Esc {
                event.stop_propagation();
                listener.call(());
                handle.request_frame();
            }
        }));
    }

    let card = Node::element(card, props.children.clone());
    portal(
        cx,
        &Props::new(PortalProps {
            z_index: Attr::Set(1050),
        })
        .children([card]),
    )
}

/// One row in a [`menu`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// Stable identity reported to `on_select`.
    pub id: Key,
    /// Visible label.
    pub label: String,
    /// Optional shortcut hint painted at the right edge.
    pub shortcut: Option<String>,
    /// Whether the item can be selected.
    pub disabled: bool,
}

impl MenuItem {
    /// Builds an enabled item.
    pub fn new(id: impl Into<Key>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            shortcut: None,
            disabled: false,
        }
    }

    /// Attaches a shortcut hint.
    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Marks the item disabled.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// Configuration for [`menu`].
#[derive(Clone, Default)]
pub struct MenuProps {
    /// Whether the menu is painted; defaults to `false`.
    pub open: Attr<bool>,
    /// The element the menu is anchored to.
    pub anchor: Attr<ElementRef>,
    /// Rows to render.
    pub items: Attr<Vec<MenuItem>>,
    /// Preferred placement; defaults to [`PopoverPlacement::BelowStart`].
    pub placement: Attr<PopoverPlacement>,
    /// Invoked with the selected item id.
    pub on_select: Attr<EventListener<Key>>,
    /// Invoked when the menu closes.
    pub on_close: Attr<EventListener<()>>,
}

impl std::fmt::Debug for MenuProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuProps")
            .field("open", &self.open)
            .field("items", &self.items)
            .field("placement", &self.placement)
            .finish_non_exhaustive()
    }
}

/// An anchored list of selectable rows with keyboard navigation.
pub fn menu(cx: &mut ComponentContext, props: &Props<MenuProps>) -> Node {
    let theme = cx.use_theme();
    let open = props.open | false;
    let items = props.items.as_ref().cloned().unwrap_or_default();
    let active_state = cx.use_live_state(|| 0_usize);
    let on_select = props.on_select.as_ref().cloned();
    let on_close = props.on_close.as_ref().cloned();
    let reset_active = active_state.clone();
    cx.use_effect(open, move || {
        let _ = reset_active.set(0);
    });
    if !open {
        return crate::empty();
    }
    let active = active_state.read(|active| *active).unwrap_or(0);
    let active = if items.is_empty() {
        0
    } else {
        active.min(items.len() - 1)
    };

    let mut rows = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let style = if index == active && !item.disabled {
            TextStyle::default()
                .foreground(theme.roles.chip_on_foreground)
                .background(theme.roles.chip_on)
        } else if item.disabled {
            TextStyle::default()
                .foreground(theme.colors.muted_foreground)
                .dim()
        } else {
            TextStyle::default().foreground(theme.colors.popover_foreground)
        };
        let mut label = Text::new(item.label.clone()).text_style(style.clone());
        if let Some(shortcut) = &item.shortcut {
            label = label.text_style(style);
            let mut row = DomProps::default();
            row.style.layout.set_default(Layout::Horizontal);
            row.style.width.set_default(Dimension::Max);
            row.style.gap = Attr::Set(1);
            let spacer = Text::new(" ");
            rows.push(Node::element(
                row,
                [
                    label.into(),
                    spacer.into(),
                    Text::new(shortcut.clone())
                        .text_style(
                            TextStyle::default()
                                .foreground(theme.colors.muted_foreground)
                                .background(if index == active {
                                    theme.roles.chip_on
                                } else {
                                    theme.colors.popover
                                }),
                        )
                        .into(),
                ],
            ));
            continue;
        }
        rows.push(label.into());
    }

    let active_for_key = active_state.clone();
    let items_for_key = items.clone();
    let on_select = on_select.clone();
    let on_close = on_close.clone();
    let on_close_for_popover = on_close.clone();
    let handle = cx.use_handle();
    let mut dom = DomProps::default();
    dom.style.layout.set_default(Layout::Vertical);
    dom.style.width.set_default(Dimension::Max);
    dom.style.gap.set_default(0);
    dom.focusable = Attr::Set(true);
    dom.autofocus = true;
    dom.events.key_down = Attr::Set(EventListener::new(move |event: KeyboardEvent| {
        let enabled: Vec<usize> = items_for_key
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (!item.disabled).then_some(index))
            .collect();
        if enabled.is_empty() {
            return;
        }
        let current_active = active_for_key.read(|active| *active).unwrap_or(0);
        let current = enabled
            .iter()
            .position(|index| *index >= current_active)
            .unwrap_or(enabled.len() - 1);
        let next = match event.key.code {
            KeyCode::Down => Some(enabled[(current + 1) % enabled.len()]),
            KeyCode::Up => Some(enabled[(current + enabled.len() - 1) % enabled.len()]),
            KeyCode::Home => Some(enabled[0]),
            KeyCode::End => Some(*enabled.last().expect("nonempty")),
            KeyCode::Enter => {
                if let Some(item) = enabled
                    .get(current)
                    .and_then(|index| items_for_key.get(*index))
                {
                    if let Some(listener) = &on_select {
                        listener.call(item.id.clone());
                    }
                    if let Some(listener) = &on_close {
                        listener.call(());
                    }
                    handle.request_frame();
                }
                event.stop_propagation();
                return;
            }
            KeyCode::Esc => {
                if let Some(listener) = &on_close {
                    listener.call(());
                }
                handle.request_frame();
                event.stop_propagation();
                return;
            }
            _ => None,
        };
        if let Some(next) = next {
            let _ = active_for_key.set(next);
            event.stop_propagation();
        }
    }));
    let menu_card = Node::element(dom, rows);

    let mut menu_props = PopoverProps {
        open: Attr::Set(true),
        anchor: props.anchor.clone(),
        placement: props.placement,
        offset: Attr::Set(0),
        dismiss_on_outside: Attr::Set(true),
        take_focus: Attr::Set(false),
        on_close: on_close_for_popover.into(),
    };
    let _ = &mut menu_props;
    popover(
        cx,
        &Props::with_parts(DomProps::default(), vec![menu_card], menu_props),
    )
}
