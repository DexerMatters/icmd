//! Titled frames, panes, and weighted split containers.

use crossterm::event::KeyCode;

use crate::{
    Attr, ComponentContext, Dimension, DomProps, Edges, EventListener, FocusEvent, Justify,
    KeyboardEvent, Layout, Node, PointerButton, PointerEvent, Props, Text,
};

/// Configuration for [`frame`].
#[derive(Clone, Debug, Default)]
pub struct FrameProps {
    /// Optional title painted in a header row inside the border.
    pub title: Attr<String>,
    /// Optional footer painted above the bottom border.
    pub footer: Attr<String>,
}

/// A bordered box with an optional title header and footer.
pub fn frame(cx: &mut ComponentContext, props: &Props<FrameProps>) -> Node {
    let theme = cx.use_theme();
    let title = props.title.clone() | String::new();
    let footer = props.footer.clone() | String::new();
    let mut dom = props.host_props(DomProps::default());
    dom.style.layout.set_default(Layout::Vertical);
    dom.style.width.set_default(Dimension::Max);
    dom.style.padding.set_default(Edges::symmetric(0, 1));
    dom.style.gap.set_default(0);
    dom.style.background.set_default(theme.colors.card);
    dom.style
        .text
        .foreground
        .set_default(theme.colors.card_foreground);
    dom.style.border.kind.set_default(theme.borders.kind);
    dom.style.border.edges.set_default(theme.borders.edges);
    dom.style
        .border
        .foreground
        .set_default(theme.borders.foreground);
    dom.style.border.background.set_default(theme.colors.card);
    let mut children = Vec::new();
    if !title.is_empty() {
        children.push(
            Text::new(title)
                .text_style(theme.typography.heading.clone())
                .into(),
        );
    }
    children.push(props.children_node());
    if !footer.is_empty() {
        children.push(
            Text::new(footer)
                .text_style(theme.typography.muted.clone())
                .into(),
        );
    }
    Node::element(dom, children)
}

/// Configuration for [`pane`].
#[derive(Clone, Debug, Default)]
pub struct PaneProps {
    /// Optional title painted in a header row inside the border.
    pub title: Attr<String>,
}

/// A framed pane that fills its slot along both axes.
///
/// In a [`split`] the main-axis style supplied by the split wrapper wins, so a
/// pane grows with its weight while still filling the cross axis.
pub fn pane(cx: &mut ComponentContext, props: &Props<PaneProps>) -> Node {
    let theme = cx.use_theme();
    let title = props.title.clone() | String::new();
    let mut dom = props.host_props(DomProps::default());
    dom.style.layout.set_default(Layout::Vertical);
    dom.style.width.set_default(Dimension::Grow(1));
    dom.style.height.set_default(Dimension::Grow(1));
    dom.style.padding.set_default(Edges::symmetric(0, 1));
    dom.style.gap.set_default(0);
    dom.style.background.set_default(theme.colors.card);
    dom.style
        .text
        .foreground
        .set_default(theme.colors.card_foreground);
    dom.style.border.kind.set_default(theme.borders.kind);
    dom.style.border.edges.set_default(theme.borders.edges);
    dom.style
        .border
        .foreground
        .set_default(theme.borders.foreground);
    dom.style.border.background.set_default(theme.colors.card);
    let mut children = Vec::new();
    if !title.is_empty() {
        children.push(
            Text::new(title)
                .text_style(theme.typography.heading.clone())
                .into(),
        );
    }
    children.push(props.children_node());
    Node::element(dom, children)
}

/// The weights a split proposes after a resize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitResizeEvent {
    /// Weights in child order, same length as the child list.
    pub weights: Vec<u16>,
}

/// Configuration for [`split`].
#[derive(Clone, Default)]
pub struct SplitProps {
    /// Splitting axis; defaults to [`Layout::Horizontal`].
    pub axis: Attr<Layout>,
    /// Controlled weights. When unset the split owns them.
    pub weights: Attr<Vec<u16>>,
    /// Whether splitters can be dragged or moved with the keyboard; defaults
    /// to `true`.
    pub draggable: Attr<bool>,
    /// Splitter thickness in cells; defaults to `1`.
    pub splitter_size: Attr<u16>,
    /// Minimum weight/cells a pane keeps while dragging; defaults to `1`.
    pub min_pane: Attr<u16>,
    /// Fired with the proposed weights after a drag or keyboard adjustment.
    pub on_weights_change: Attr<EventListener<SplitResizeEvent>>,
}

impl std::fmt::Debug for SplitProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitProps")
            .field("axis", &self.axis)
            .field("weights", &self.weights)
            .field("draggable", &self.draggable)
            .field("splitter_size", &self.splitter_size)
            .field("min_pane", &self.min_pane)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    index: usize,
    start: i32,
    main_cells: i32,
    base: [u16; 2],
}

/// A row or column of children sized by weights, with draggable splitters.
pub fn split(cx: &mut ComponentContext, props: &Props<SplitProps>) -> Node {
    let theme = cx.use_theme();
    let horizontal = !matches!(props.axis | Layout::Horizontal, Layout::Vertical);
    let count = props.children.len();
    let size = (props.splitter_size | 1).max(1);
    let min_pane = (props.min_pane | 1).max(1);
    let draggable = props.draggable | true;
    let container = cx.use_element_ref();
    let internal = cx.use_live_state(Vec::<u16>::new);
    let drag = cx.use_ref(|| None::<Drag>);
    let active_splitter = cx.use_live_state(|| None::<usize>);
    let on_change = props.on_weights_change.as_ref().cloned();
    if count == 0 {
        return crate::empty();
    }

    /*
     * Normalize equal weights on first render and whenever the child count
     * changes.
     */
    let fallback = {
        let mut current = internal.read(|weights| weights.clone()).unwrap_or_default();
        if current.len() != count {
            current = vec![1; count];
            let _ = internal.update_quiet(|weights| *weights = current.clone());
        }
        current
    };
    let weights = props.weights.as_ref().cloned().unwrap_or(fallback);
    let weights: Vec<u16> = (0..count)
        .map(|index| weights.get(index).copied().unwrap_or(1).max(1))
        .collect();

    let set_weights = {
        let internal = internal.clone();
        let on_change = on_change.clone();
        move |next: Vec<u16>| {
            let _ = internal.update(|weights| *weights = next.clone());
            if let Some(listener) = &on_change {
                listener.call(SplitResizeEvent { weights: next });
            }
        }
    };

    let mut dom = props.host_props(DomProps::default());
    dom.style.layout = Attr::Set(if horizontal {
        Layout::Horizontal
    } else {
        Layout::Vertical
    });
    dom.style.width.set_default(Dimension::Max);
    dom.style.height.set_default(Dimension::Max);
    dom.style.gap.set_default(0);
    dom.style.justify.set_default(Justify::Start);
    dom.element_ref = Attr::Set(container.clone());

    let mut children = Vec::with_capacity(count * 2 - 1);
    for (index, child) in props.children.iter().cloned().enumerate() {
        let mut slot = DomProps::default();
        slot.style.layout = Attr::Set(Layout::Vertical);
        slot.style.gap.set_default(0);
        if horizontal {
            slot.style.width = Attr::Set(Dimension::Grow(weights[index]));
            slot.style.height = Attr::Set(Dimension::Max);
        } else {
            slot.style.width = Attr::Set(Dimension::Max);
            slot.style.height = Attr::Set(Dimension::Grow(weights[index]));
        }
        children.push(Node::element(slot, [child]));

        if index + 1 < count {
            let mut splitter = DomProps::default();
            let highlighted = active_splitter
                .read(|active| *active == Some(index))
                .unwrap_or(false);
            splitter.style.background = Attr::Set(if highlighted {
                theme.colors.primary
            } else {
                theme.colors.border
            });
            splitter.focusable = Attr::Set(draggable);
            let active_for_focus = active_splitter.clone();
            splitter.events.focus_event =
                Attr::Set(EventListener::new(move |event: FocusEvent| {
                    let _ = active_for_focus.update(|active| match event {
                        FocusEvent::Gained => *active = Some(index),
                        FocusEvent::Lost => {
                            if *active == Some(index) {
                                *active = None;
                            }
                        }
                    });
                }));
            if horizontal {
                splitter.style.width = Attr::Set(Dimension::Cells(size));
                splitter.style.height = Attr::Set(Dimension::Max);
            } else {
                splitter.style.width = Attr::Set(Dimension::Max);
                splitter.style.height = Attr::Set(Dimension::Cells(size));
            }
            if draggable {
                let container_for_down = container.clone();
                let drag_for_down = drag.clone();
                let active_splitter = active_splitter.clone();
                let weights_for_down = weights.clone();
                splitter.events.pointer_down =
                    Attr::Set(EventListener::new(move |event: PointerEvent| {
                        if event.button != PointerButton::Primary {
                            return;
                        }
                        event.stop_propagation();
                        let rect = container_for_down
                            .current()
                            .map(|snapshot| snapshot.content_rect())
                            .unwrap_or(crate::ElementRect::new(0, 0, 0, 0));
                        let main_cells = if horizontal {
                            i32::try_from(rect.width).unwrap_or(i32::MAX)
                        } else {
                            i32::try_from(rect.height).unwrap_or(i32::MAX)
                        }
                        .saturating_sub(
                            (count.saturating_sub(1) as i32).saturating_mul(size as i32),
                        )
                        .max(1);
                        let coordinate = if horizontal {
                            event.position.column
                        } else {
                            event.position.line
                        };
                        let _ = active_splitter.set(Some(index));
                        *drag_for_down.lock().expect("split drag poisoned") = Some(Drag {
                            index,
                            start: coordinate,
                            main_cells,
                            base: [
                                *weights_for_down.get(index).unwrap_or(&1),
                                *weights_for_down.get(index + 1).unwrap_or(&1),
                            ],
                        });
                    }));
                let drag_for_move = drag.clone();
                let weights_for_move = weights.clone();
                let set_for_move = set_weights.clone();
                splitter.events.pointer_move =
                    Attr::Set(EventListener::new(move |event: PointerEvent| {
                        let Some(active) = *drag_for_move.lock().expect("split drag poisoned")
                        else {
                            return;
                        };
                        if active.index != index {
                            return;
                        }
                        event.stop_propagation();
                        let coordinate = if horizontal {
                            event.position.column
                        } else {
                            event.position.line
                        };
                        let delta = coordinate.saturating_sub(active.start);
                        let total: u32 = weights_for_move.iter().map(|w| u32::from(*w)).sum();
                        if total == 0 || active.main_cells <= 0 {
                            return;
                        }
                        let cells_a = (i64::from(active.main_cells) * i64::from(active.base[0])
                            / i64::from(total)) as i32;
                        let cells_b = (i64::from(active.main_cells) * i64::from(active.base[1])
                            / i64::from(total)) as i32;
                        let min = i32::from(min_pane);
                        let next_a = (cells_a + delta).max(min);
                        let next_b = (cells_b - delta).max(min);
                        let mut next = weights_for_move.clone();
                        next[index] = u16::try_from(next_a).unwrap_or(u16::MAX).max(min_pane);
                        next[index + 1] = u16::try_from(next_b).unwrap_or(u16::MAX).max(min_pane);
                        set_for_move(next);
                    }));
                let drag_for_up = drag.clone();
                splitter.events.pointer_up =
                    Attr::Set(EventListener::new(move |_event: PointerEvent| {
                        *drag_for_up.lock().expect("split drag poisoned") = None;
                    }));
                let drag_for_cancel = drag.clone();
                splitter.events.pointer_cancel =
                    Attr::Set(EventListener::new(move |_event: PointerEvent| {
                        *drag_for_cancel.lock().expect("split drag poisoned") = None;
                    }));
                let weights_for_key = weights.clone();
                let set_for_key = set_weights.clone();
                splitter.events.key_down =
                    Attr::Set(EventListener::new(move |event: KeyboardEvent| {
                        let delta = match (horizontal, event.key.code) {
                            (true, KeyCode::Left) | (false, KeyCode::Up) => -1,
                            (true, KeyCode::Right) | (false, KeyCode::Down) => 1,
                            _ => return,
                        };
                        event.stop_propagation();
                        let mut next = weights_for_key.clone();
                        let min = i32::from(min_pane);
                        let a = i32::from(next[index]);
                        let b = i32::from(next[index + 1]);
                        next[index] = u16::try_from((a + delta).max(min)).unwrap_or(u16::MAX);
                        next[index + 1] = u16::try_from((b - delta).max(min)).unwrap_or(u16::MAX);
                        set_for_key(next);
                    }));
            }
            children.push(Node::element(splitter, []));
        }
    }
    Node::element(dom, children)
}
