//! `raw_input`: the one primitive that owns text-entry behavior.
//!
//! The component renders one semantic host: it receives caller `DomProps`, is
//! the focus target, and carries the composed internal-plus-caller event
//! listeners. A `scroll_area` nested inside that host supplies the scrolling
//! mechanism, so there is no second event or focus identity. `input` and
//! `textarea` are thin policy wrappers over it.
//!
//! Geometry - wrapping, caret placement, pointer hit-testing, and scroll
//! extents - comes from the canonical layout in [`crate::basic::text_layout`];
//! the editor does not implement its own wrapping.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::basic::editor_surface::{EditorSurface, LayoutProbe};
use crate::basic::text_layout::{self, ComputedText, HitBias};
use crate::{
    Attr, Dimension, DomProps, EventListener, FocusEvent, KeyboardEvent, Node, PasteEvent,
    PointerEvent, Props, ScrollAxes, ScrollEvent, ScrollOffset, ScrollbarVisibility, Style, Text,
    TextStyle, TextWrap, basic::ComponentContext, scroll_area, ui, view,
};

use super::model::{EditAction, EditModel, EditPolicy};
use super::{TextClipboardAction, TextClipboardEvent, TextValueEvent};

/// Whether `raw_input` lays its value out on one line or many.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RawInputMode {
    #[default]
    SingleLine,
    Multiline,
}

/// Editor-specific decoration that ordinary host [`Style`] cannot express.
///
/// Defaults are terminal-native: inherit text colors, dim the placeholder, and
/// reverse the caret and selection. `input` and `textarea` translate the
/// application theme into this appearance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInputAppearance {
    pub placeholder: TextStyle,
    pub selection: TextStyle,
    /// Selection painting when the control does not hold focus.
    pub selection_inactive: TextStyle,
    pub caret: TextStyle,
    /// Border foreground applied while focused, when the theme has one.
    pub focused_border: Option<crossterm::style::Color>,
}

impl Default for RawInputAppearance {
    fn default() -> Self {
        Self {
            placeholder: TextStyle::default().dim(),
            selection: TextStyle::default().reverse(),
            selection_inactive: TextStyle::default().dim(),
            caret: TextStyle::default().reverse(),
            focused_border: None,
        }
    }
}

/// Behavior of a `raw_input`.
///
/// It carries no width or height: layout has exactly one owner,
/// `props.dom.style`.
#[derive(Clone, Default)]
pub struct RawInputProps {
    pub mode: Attr<RawInputMode>,
    pub value: Attr<String>,
    pub default_value: Attr<String>,
    pub placeholder: Attr<String>,
    pub wrap: Attr<TextWrap>,
    pub max_length: Attr<usize>,
    pub disabled: Attr<bool>,
    pub read_only: Attr<bool>,
    pub appearance: Attr<RawInputAppearance>,
    pub on_change: Attr<EventListener<TextValueEvent>>,
    pub on_submit: Attr<EventListener<TextValueEvent>>,
    pub on_clipboard: Attr<EventListener<TextClipboardEvent>>,
}

impl std::fmt::Debug for RawInputProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawInputProps")
            .field("mode", &self.mode)
            .field("value", &self.value)
            .field("default_value", &self.default_value)
            .field("placeholder", &self.placeholder)
            .field("wrap", &self.wrap)
            .field("max_length", &self.max_length)
            .field("disabled", &self.disabled)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

/// Interaction state the durable model deliberately does not own.
#[derive(Default)]
struct InputState {
    model: EditModel,
    focused: bool,
    dragging: bool,
    scroll_x: usize,
    scroll_y: usize,
    reveal_caret: bool,
    /// The visible viewport height the last render used, forwarded to the
    /// commit pass so the scroll extent is measured against the viewport rather
    /// than the document's own box.
    viewport_height: Option<usize>,
}

/// Immutable render-time configuration, shared with event handlers.
struct Config {
    multiline: bool,
    wrap: TextWrap,
    /// The requested content box, used only until the first frame commits.
    width: usize,
    height: usize,
    policy: EditPolicy,
    appearance: RawInputAppearance,
    placeholder: String,
}

impl Config {
    fn scroll_axes(&self) -> ScrollAxes {
        if !self.multiline {
            // A single logical line never wraps, so the viewport pans across it.
            ScrollAxes::Horizontal
        } else if self.wrap == TextWrap::NoWrap {
            // Unwrapped multiline content can overflow both axes.
            ScrollAxes::Both
        } else {
            // Wrapped multiline rows are built at the granted width, so only
            // the vertical axis scrolls.
            ScrollAxes::Vertical
        }
    }
}

fn config_from(props: &Props<RawInputProps>) -> Config {
    let mode = props.mode | RawInputMode::SingleLine;
    let multiline = matches!(mode, RawInputMode::Multiline);
    let wrap = if multiline {
        props.wrap | TextWrap::Soft
    } else {
        TextWrap::NoWrap
    };
    let width = match props.dom.style.width {
        Attr::Set(Dimension::Cells(value)) => value as usize,
        _ => 24,
    };
    let height = match props.dom.style.height {
        Attr::Set(Dimension::Cells(value)) => value as usize,
        _ => 1,
    };
    // These are only the *requested* size, used to bootstrap the first frame
    // before the commit pass has published a content box. The editor never
    // derives its own content geometry: the committed box is authoritative, so
    // padding and borders are never subtracted here.
    Config {
        multiline,
        wrap,
        width: width.max(1),
        height: height.max(1),
        policy: EditPolicy {
            multiline,
            max_length: props.max_length.as_ref().copied(),
            read_only: props.read_only | false,
            disabled: props.disabled | false,
        },
        appearance: props.appearance.clone() | RawInputAppearance::default(),
        placeholder: props.placeholder.clone() | String::new(),
    }
}

/// Render the raw text-entry primitive.
///
/// This component owns its text presentation; children supplied through the
/// generic component API are ignored, because they cannot be mapped safely to
/// source positions. Extension means wrapping this primitive and forwarding
/// props, style, and events.
pub fn raw_input(cx: &mut ComponentContext, props: &Props<RawInputProps>) -> Node {
    let config = Arc::new(config_from(props));
    let state_ref = cx.use_ref(InputState::default);
    let (_, redraw) = cx.use_state(|| 0_u64);

    // The commit pass publishes the layout it painted here; this component only
    // reads it, so wrapping never needs a feedback render.
    let probe = {
        let cell = cx.use_ref(LayoutProbe::new);
        cell.lock().expect("layout probe poisoned").clone()
    };

    // Reconcile the owner's value, then bring the caret into view using the
    // layout of the last painted frame.
    let (scroll_offset, text) = {
        let mut state = state_ref.lock().expect("input state poisoned");
        state.model.render(
            props.value.as_ref().map(String::as_str),
            props.default_value.as_ref().map(String::as_str),
            config.multiline,
        );
        if config.policy.disabled {
            state.dragging = false;
            state.reveal_caret = false;
        }
        // The caret and scroll position belong to the model's current (possibly
        // optimistic) value, and the geometry must be exactly what the last
        // committed frame painted. The commit pass publishes that content box;
        // the requested size only bootstraps the very first frame.
        // A single unwrapped line is horizontally scrollable, so its viewport
        // width is the granted box rather than the document's own width; a
        // wrapped or multiline document scrolls vertically inside the viewport
        // the commit pass actually painted.
        let committed = probe.committed();
        let (view_width, view_height) = match &committed {
            Some(committed) => (
                committed.viewport_width.max(1),
                committed.viewport_height.max(1),
            ),
            None => (config.width.max(1), config.height.max(1)),
        };
        let layout = build_layout_at(&state.model.value, &config, view_width);
        reconcile_scroll(&mut state, &layout, view_width, view_height);
        state.reveal_caret = false;
        let focused = state.focused && !config.policy.disabled;
        let scroll_offset = ScrollOffset::new(state.scroll_x as u32, state.scroll_y as u32);
        let (scroll_x, scroll_y) = (state.scroll_x, state.scroll_y);
        let surface = EditorSurface {
            value: state.model.value.clone(),
            selection: if state.model.caret().is_collapsed() {
                None
            } else {
                Some(state.model.caret().range())
            },
            caret: state.model.caret().cursor,
            focused,
            placeholder: config.placeholder.clone(),
            wrap: config.wrap,
            placeholder_style: config.appearance.placeholder.clone(),
            selection_style: config.appearance.selection.clone(),
            selection_inactive_style: config.appearance.selection_inactive.clone(),
            caret_style: config.appearance.caret.clone(),
            scroll_x: state.scroll_x,
            scroll_y: state.scroll_y,
        };
        state.viewport_height = Some(view_height);
        let text = Text::from_spans(Vec::new())
            .with_style(surface_style())
            .wrap(config.wrap)
            .editor_surface(
                surface,
                probe.clone(),
                (scroll_x, scroll_y),
                state.viewport_height,
            );
        (scroll_offset, text)
    };

    // Compose internal behavior with caller observers on one host.
    let caller = props.dom.events.clone();
    let on_change = props.on_change.as_ref().cloned();
    let on_submit = props.on_submit.as_ref().cloned();
    let on_clipboard = props.on_clipboard.as_ref().cloned();

    let key = {
        let state_ref = state_ref.clone();
        let redraw = redraw.clone();
        let config = config.clone();
        let probe = probe.clone();
        let on_change = on_change.clone();
        let on_submit = on_submit.clone();
        let on_clipboard = on_clipboard.clone();
        let caller = caller.key_down.as_ref().cloned();
        EventListener::compose(
            move |event: KeyboardEvent| {
                if config.policy.disabled {
                    return;
                }
                let mut value_event = None;
                let mut clipboard_event = None;
                let mut submit_event = None;
                let mut focus_gained = false;
                let mut caret_moved = false;
                let copy_selection = is_copy(&event);
                {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    if !state.focused {
                        state.focused = true;
                        focus_gained = true;
                    }
                    if !config.multiline
                        && event.key.code == KeyCode::Enter
                        && !event.key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        event.stop_propagation();
                        submit_event = Some(TextValueEvent {
                            value: state.model.value().to_string(),
                        });
                    } else if is_cut(&event) && !state.model.caret().is_collapsed() {
                        // Cut removes the selection and reports the removed text.
                        // The model's backspace over a non-collapsed selection is
                        // exactly that operation.
                        event.stop_propagation();
                        let outcome = state
                            .model
                            .reduce(EditAction::Backspace { word: false }, config.policy);
                        if outcome.changed {
                            state.reveal_caret = true;
                            value_event = outcome.value.map(|value| TextValueEvent { value });
                        }
                        if let Some(text) = outcome.clipboard {
                            clipboard_event = Some(TextClipboardEvent {
                                action: TextClipboardAction::Cut,
                                text,
                            });
                        }
                    } else if let Some(action) = key_action(&event, &config) {
                        event.stop_propagation();
                        // Vertical movement is resolved from the same row table
                        // the pointer and the painter use, so a caret crossing
                        // wrapped rows lands where the user sees it.
                        let outcome = if let Some((direction, steps)) = vertical_steps(&action) {
                            let layout = committed_layout(&probe, &state, &config);
                            // A page is the viewport the layout actually
                            // painted, not the requested border box.
                            let steps = if matches!(
                                action,
                                EditAction::PageUp { .. } | EditAction::PageDown { .. }
                            ) {
                                probe
                                    .committed()
                                    .map_or(steps, |committed| committed.viewport_height.max(1))
                            } else {
                                steps
                            };
                            let extend = event.key.modifiers.contains(KeyModifiers::SHIFT);
                            state.model.vertical_move(
                                &layout,
                                direction,
                                steps,
                                extend,
                                config.policy,
                            )
                        } else {
                            state.model.reduce(action, config.policy)
                        };
                        if outcome.changed {
                            value_event = outcome.value.map(|value| TextValueEvent { value });
                        }
                        caret_moved = outcome.reveal_caret;
                        state.reveal_caret |= outcome.reveal_caret;
                        if let Some(text) = outcome.clipboard {
                            clipboard_event = Some(TextClipboardEvent {
                                action: TextClipboardAction::Cut,
                                text,
                            });
                        }
                    } else if copy_selection {
                        let caret = state.model.caret();
                        if !caret.is_collapsed() {
                            let (start, end) = caret.range();
                            event.stop_propagation();
                            clipboard_event = Some(TextClipboardEvent {
                                action: TextClipboardAction::Copy,
                                text: state.model.value()[start..end].to_string(),
                            });
                        }
                    }
                }
                if let Some(listener) = &on_clipboard
                    && let Some(event) = clipboard_event
                {
                    listener.call(event);
                }
                if let Some(listener) = &on_change
                    && let Some(event) = value_event.clone()
                {
                    listener.call(event);
                }
                if let Some(listener) = &on_submit
                    && let Some(event) = submit_event
                {
                    listener.call(event);
                }
                if value_event.is_some() || focus_gained || caret_moved {
                    redraw.update(|value| *value += 1);
                }
            },
            caller,
        )
    };

    let paste = {
        let state_ref = state_ref.clone();
        let redraw = redraw.clone();
        let config = config.clone();
        let on_change = on_change.clone();
        let caller = caller.paste_event.as_ref().cloned();
        EventListener::compose(
            move |event: PasteEvent| {
                if config.policy.disabled || config.policy.read_only {
                    return;
                }
                let value_event;
                {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    state.focused = true;
                    let outcome = state
                        .model
                        .reduce(EditAction::Insert(event.text.clone()), config.policy);
                    if outcome.changed {
                        state.reveal_caret = true;
                    }
                    value_event = outcome.value.map(|value| TextValueEvent { value });
                }
                if let Some(listener) = &on_change
                    && let Some(event) = value_event
                {
                    listener.call(event);
                    redraw.update(|value| *value += 1);
                }
            },
            caller,
        )
    };

    let pointer_down = {
        let state_ref = state_ref.clone();
        let redraw = redraw.clone();
        let config = config.clone();
        let probe = probe.clone();
        let caller = caller.pointer_down.as_ref().cloned();
        EventListener::compose(
            move |event: PointerEvent| {
                if config.policy.disabled || !event.is_primary_button() {
                    return;
                }
                {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    state.focused = true;
                    let layout = committed_layout(&probe, &state, &config);
                    let offset = hit_offset(&layout, event.local_position, probe.committed());
                    let extend = event.modifiers.contains(KeyModifiers::SHIFT);
                    state
                        .model
                        .reduce(EditAction::PlaceCaret { offset, extend }, config.policy);
                    state.dragging = true;
                }
                redraw.update(|value| *value += 1);
            },
            caller,
        )
    };

    let pointer_move = {
        let state_ref = state_ref.clone();
        let redraw = redraw.clone();
        let config = config.clone();
        let probe = probe.clone();
        let caller = caller.pointer_move.as_ref().cloned();
        EventListener::compose(
            move |event: PointerEvent| {
                let mut state = state_ref.lock().expect("input state poisoned");
                if !state.dragging {
                    return;
                }
                let layout = committed_layout(&probe, &state, &config);
                let offset = hit_offset(&layout, event.local_position, probe.committed());
                state.model.reduce(
                    EditAction::PlaceCaret {
                        offset,
                        extend: true,
                    },
                    config.policy,
                );
                state.reveal_caret = false;
                drop(state);
                redraw.update(|value| *value += 1);
            },
            caller,
        )
    };

    let pointer_up = {
        let state_ref = state_ref.clone();
        let caller = caller.pointer_up.as_ref().cloned();
        EventListener::compose(
            move |_event: PointerEvent| {
                state_ref.lock().expect("input state poisoned").dragging = false;
            },
            caller,
        )
    };

    let pointer_cancel = {
        let state_ref = state_ref.clone();
        let caller = caller.pointer_cancel.as_ref().cloned();
        EventListener::compose(
            move |_event: PointerEvent| {
                state_ref.lock().expect("input state poisoned").dragging = false;
            },
            caller,
        )
    };

    let focus = {
        let state_ref = state_ref.clone();
        let redraw = redraw.clone();
        let caller = caller.focus_event.as_ref().cloned();
        EventListener::compose(
            move |event: FocusEvent| {
                let mut state = state_ref.lock().expect("input state poisoned");
                let next = event == FocusEvent::Gained;
                if state.focused != next {
                    state.focused = next;
                    if !next {
                        state.dragging = false;
                    }
                    drop(state);
                    redraw.update(|value| *value += 1);
                }
            },
            caller,
        )
    };

    let scroll = {
        let state_ref = state_ref.clone();
        let redraw = redraw.clone();
        let caller = caller.scroll.as_ref().cloned();
        EventListener::compose(
            move |event: ScrollEvent| {
                {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    state.scroll_x = event.offset.x as usize;
                    state.scroll_y = event.offset.y as usize;
                    // Physical scrolling never re-centers on the caret; only a
                    // later edit or navigation action requests reveal.
                    state.reveal_caret = false;
                }
                redraw.update(|value| *value += 1);
            },
            caller,
        )
    };

    // The editor's single semantic host: caller DOM props and explicit
    // focusability. The caller's observers are composed inside each internal
    // listener, so the host's own event slots stay free for the `ui!`
    // attributes below, and the scroll area nested inside it provides scrolling
    // without introducing a second event or focus identity.
    let mut scroll_host = props.host_props(DomProps::default());
    scroll_host.focusable = !config.policy.disabled;
    if focus_active(&state_ref, &config)
        && let Some(color) = config.appearance.focused_border
    {
        scroll_host.style.border.foreground = Attr::Set(color);
    }
    let scroll_axes = config.scroll_axes();
    let disabled = config.policy.disabled;
    // The editor host owns caller DOM props, focus (via `focusable`), and the
    // composed listeners, and it is the node that `scroll_area` wraps. The
    // scroll area supplies the scrolling mechanism and the content box the
    // surface wraps to, so pointer coordinates and painted rows share one
    // content space even though the scroll engine lives on the inner node.
    let scrollable = if disabled {
        ui! {
            <view style={|style| {
                style.width /= Dimension::Max;
                style.height /= Dimension::Max;
            }}>
                {text}
            </view>
        }
    } else {
        ui! {
            <scroll_area
                axes={scroll_axes}
                scrollbar_visibility={ScrollbarVisibility::Hidden}
                offset={scroll_offset}
                enable_keyboard={false}
                style={|style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Max;
                }}
            >
                {text}
            </scroll_area>
        }
    };

    // A disabled control must not retain focus. Rendering the enabled host
    // under a distinct key removes its DOM id from the region set, which is
    // what makes the runtime deliver exactly one `Lost` focus event.
    if disabled {
        ui! {
            <view key="raw-input-disabled" dom={scroll_host}>{scrollable}</view>
        }
    } else {
        ui! {
            <view key="raw-input-enabled" dom={scroll_host}
                on_key_down={key}
                on_paste_event={paste}
                on_pointer_down={pointer_down}
                on_pointer_move={pointer_move}
                on_pointer_up={pointer_up}
                on_pointer_cancel={pointer_cancel}
                on_focus_event={focus}
                on_scroll={scroll}
            >
                {scrollable}
            </view>
        }
    }
}

fn focus_active(state_ref: &crate::basic::Ref<InputState>, config: &Config) -> bool {
    let state = state_ref.lock().expect("input state poisoned");
    state.focused && !config.policy.disabled
}

/// Lay out `value` at an explicit content width.
fn build_layout_at(value: &str, config: &Config, width: usize) -> text_layout::TextLayout {
    text_layout::layout_text(
        &Text::new(value).wrap(config.wrap),
        width.max(1),
        ComputedText::default(),
        |parent, _| parent,
        |style| *style,
    )
}

/// Fallback layout used before the first frame has been committed.
fn build_layout(value: &str, config: &Config) -> text_layout::TextLayout {
    build_layout_at(value, config, config.width)
}

/// The layout that produced the last painted frame, falling back to a local
/// build before the first frame settles.
fn committed_layout(
    probe: &LayoutProbe,
    state: &InputState,
    config: &Config,
) -> text_layout::TextLayout {
    probe
        .committed()
        .map(|committed| (*committed.layout).clone())
        .unwrap_or_else(|| build_layout(&state.model.value, config))
}

/// Clamp the viewport and, when requested, bring the caret into view.
fn reconcile_scroll(
    state: &mut InputState,
    layout: &text_layout::TextLayout,
    view_width: usize,
    view_height: usize,
) {
    let view_width = view_width.max(1);
    let view_height = view_height.max(1);
    let rows = layout.row_count().max(1);
    state.scroll_y = state.scroll_y.min(rows.saturating_sub(view_height));
    let widest = layout.max_row_width();
    state.scroll_x = state.scroll_x.min(widest.saturating_sub(view_width));
    if !state.focused || !state.reveal_caret {
        return;
    }
    let (row, cell, caret_width) = layout.caret(state.model.caret().cursor);
    if row < state.scroll_y {
        state.scroll_y = row;
    } else if row >= state.scroll_y.saturating_add(view_height) {
        state.scroll_y = row + 1 - view_height;
    }
    if cell < state.scroll_x {
        state.scroll_x = cell;
    } else {
        let caret_end = cell.saturating_add(caret_width.max(1));
        if caret_end > state.scroll_x.saturating_add(view_width) {
            state.scroll_x = caret_end - view_width;
        }
    }
}

/// The editor surface keeps auto dimensions so it reports its own width
/// (intrinsic for `NoWrap`, the granted width for wrapped modes) and its row
/// count.
/// The editor surface reports both dimensions itself: intrinsic width for
/// `NoWrap`, the granted width for wrapped modes, and its full document height
/// so the scroll host can see the overflow and pan across it.
fn surface_style() -> Style {
    Style::default()
}

fn is_cut(event: &KeyboardEvent) -> bool {
    event.key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(event.key.code, KeyCode::Char('x' | 'X'))
}

fn is_copy(event: &KeyboardEvent) -> bool {
    event.key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(event.key.code, KeyCode::Char('c' | 'C'))
}

/// The signed row delta and repeat count for a vertical navigation action.
fn vertical_steps(action: &EditAction) -> Option<(i32, usize)> {
    match action {
        EditAction::MoveUp { .. } => Some((-1, 1)),
        EditAction::MoveDown { .. } => Some((1, 1)),
        EditAction::PageUp { rows, .. } => Some((-1, (*rows).max(1))),
        EditAction::PageDown { rows, .. } => Some((1, (*rows).max(1))),
        _ => None,
    }
}

/// The edit action a key press maps to, or `None` when the key does not belong
/// to this mode.
fn key_action(event: &KeyboardEvent, config: &Config) -> Option<EditAction> {
    let modifiers = event.key.modifiers;
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    match event.key.code {
        KeyCode::Left => Some(EditAction::MoveLeft {
            extend: shift,
            word: ctrl,
        }),
        KeyCode::Right => Some(EditAction::MoveRight {
            extend: shift,
            word: ctrl,
        }),
        KeyCode::Home => Some(EditAction::Home {
            extend: shift,
            document: ctrl,
        }),
        KeyCode::End => Some(EditAction::End {
            extend: shift,
            document: ctrl,
        }),
        KeyCode::Up if config.multiline => Some(EditAction::MoveUp { extend: shift }),
        KeyCode::Down if config.multiline => Some(EditAction::MoveDown { extend: shift }),
        KeyCode::PageUp if config.multiline => Some(EditAction::PageUp {
            extend: shift,
            rows: config.height,
        }),
        KeyCode::PageDown if config.multiline => Some(EditAction::PageDown {
            extend: shift,
            rows: config.height,
        }),
        KeyCode::Char('a' | 'A') if ctrl => Some(EditAction::SelectAll),
        KeyCode::Backspace => Some(EditAction::Backspace { word: ctrl }),
        KeyCode::Delete => Some(EditAction::Delete { word: ctrl }),
        KeyCode::Enter if config.multiline => Some(EditAction::InsertNewline),
        KeyCode::Char(ch)
            if !ctrl
                && !ch.is_control()
                && !modifiers.intersects(
                    KeyModifiers::ALT
                        | KeyModifiers::SUPER
                        | KeyModifiers::HYPER
                        | KeyModifiers::META,
                ) =>
        {
            Some(EditAction::Insert(ch.to_string()))
        }
        _ => None,
    }
}

/// Hit bias used for pointer placement.
const POINTER_HIT_BIAS: HitBias = HitBias::Trailing;

/// Resolve a pointer hit into a source offset using the canonical layout.
///
/// `local_position` already includes the runtime's own scroll offset, and the
/// model owns a delta the runtime has not applied yet, so the offsets
/// accumulated before the last committed frame are added back here. This
/// component never subtracts padding or borders by hand: the event system
/// supplies content-box coordinates.
fn hit_offset(
    layout: &text_layout::TextLayout,
    local: crate::ScreenPosition,
    committed: Option<crate::basic::editor_surface::CommittedLayout>,
) -> usize {
    // The runtime reports the pointer in the coordinates of the region it hit,
    // which is the child as painted. That frame was laid out with exactly the
    // committed applied offset, so adding that offset back once converts the
    // pointer into the layout's unscrolled content coordinates. The requested
    // offset is deliberately NOT consulted: the request and the frame that the
    // user actually clicked on can differ, and the painted frame is the truth.
    // Padding and borders are never subtracted by hand: the event system
    // supplies content-box coordinates.
    let (applied_x, applied_y) = committed.as_ref().map_or((0, 0), |committed| {
        (committed.applied_x, committed.applied_y)
    });
    let row = (local.line.max(0) as usize + applied_y).min(layout.row_count().saturating_sub(1));
    let cell = local.column.max(0) as usize + applied_x;
    layout.hit(row, cell, POINTER_HIT_BIAS)
}
