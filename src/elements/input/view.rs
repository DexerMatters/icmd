//! Raw text editor surface: caret, selection, scrolling, and clipboard
//! handling shared by `input` and `textarea`.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::basic::editor_surface::{EditorSurface, LayoutProbe};
use crate::basic::selection::{
    ClipboardIntent, DocPoint, SelectionDocument, SelectionMotion, clipboard_intent,
    clipboard_load, clipboard_store, motion_for,
};
use crate::basic::text_layout::{self, ComputedText, HitBias};
use crate::{
    Attr, Dimension, DomProps, EmojiMerging, EventListener, FocusEvent, KeyboardEvent, Node,
    PasteEvent, PointerEvent, Props, ScrollAxes, ScrollEvent, ScrollOffset, ScrollbarVisibility,
    Style, Text, TextStyle, TextWrap, basic::ComponentContext, scroll_area, ui, view,
};

use super::model::{EditAction, EditIntent, EditModel, EditPolicy};
use super::{TextClipboardAction, TextClipboardEvent, TextValueEvent};

/// Editing mode for [`raw_input`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RawInputMode {
    /// One logical line; no wrapping, scrolls horizontally.
    #[default]
    SingleLine,
    /// Multiple logical lines, optionally wrapped.
    Multiline,
}

/// Visual style configuration for [`raw_input`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInputAppearance {
    /// Style for placeholder text.
    pub placeholder: TextStyle,
    /// Style for the focused selection.
    pub selection: TextStyle,
    /// Style for the selection while unfocused.
    pub selection_inactive: TextStyle,
    /// Style for the caret.
    pub caret: TextStyle,
    /// Optional border color applied while focused; `None` leaves the host
    /// border unchanged.
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

/// Configuration for [`raw_input`].
#[derive(Clone, Default)]
pub struct RawInputProps {
    /// Editing mode; defaults to [`RawInputMode::SingleLine`].
    pub mode: Attr<RawInputMode>,
    /// Controlled value; when set, the control renders it and requests changes
    /// instead of mutating itself.
    pub value: Attr<String>,
    /// Initial value for an uncontrolled control.
    pub default_value: Attr<String>,
    /// Text shown while the value is empty.
    pub placeholder: Attr<String>,
    /// Line wrapping for multi-line mode; defaults to [`TextWrap::Soft`] and is
    /// ignored for single-line mode.
    pub wrap: Attr<TextWrap>,
    /// Maximum value length in display units; unbounded when unset.
    pub max_length: Attr<usize>,
    /// Whether editing and focus are refused; defaults to `false`.
    pub disabled: Attr<bool>,
    /// Whether the value may be selected and copied but not edited; defaults
    /// to `false`.
    pub read_only: Attr<bool>,
    /// Whether the control requests focus on mount; defaults to `false`.
    pub autofocus: Attr<bool>,
    /// Visual styles; defaults to [`RawInputAppearance::default`].
    pub appearance: Attr<RawInputAppearance>,
    /// Listener for each committed value change.
    pub on_change: Attr<EventListener<TextValueEvent>>,
    /// Listener for Enter in single-line mode.
    pub on_submit: Attr<EventListener<TextValueEvent>>,
    /// Listener for copy and cut operations answered by the control.
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

/// Mutable interaction state owned by one raw input across renders.
#[derive(Default)]
struct InputState {
    /// The editing model: value, caret, and draft reconciliation.
    model: EditModel,
    /// Whether the control currently holds focus.
    focused: bool,
    /// Whether a pointer drag is selecting.
    dragging: bool,
    /// The offset the press landed on, so a drag resolves the glyph under the
    /// pointer inclusively in both directions - the same rule a selectable
    /// region uses.
    pressed: usize,
    /// Horizontal scroll offset in cells.
    scroll_x: usize,
    /// Vertical scroll offset in rows.
    scroll_y: usize,
    /// Whether the next reconcile should scroll the caret into view.
    reveal_caret: bool,
}

/// Resolved per-frame configuration derived from props and the committed
/// layout.
struct Config {
    /// Whether the control is multi-line.
    multiline: bool,
    /// Resolved wrap mode.
    wrap: TextWrap,
    /// Bootstrap content width in cells before the first commit.
    width: usize,
    /// Bootstrap content height in rows before the first commit.
    height: usize,
    /// Editing policy forwarded to the model.
    policy: EditPolicy,
    /// Resolved visual styles.
    appearance: RawInputAppearance,
    /// Placeholder text.
    placeholder: String,
}

impl Config {
    /// Axes this configuration scrolls on.
    ///
    /// A single logical line never wraps, so the viewport pans across it;
    /// unwrapped multiline content can overflow both axes; wrapped multiline
    /// rows are built at the granted width, so only the vertical axis scrolls.
    fn scroll_axes(&self) -> ScrollAxes {
        if !self.multiline {
            ScrollAxes::Horizontal
        } else if self.wrap == TextWrap::NoWrap {
            ScrollAxes::Both
        } else {
            ScrollAxes::Vertical
        }
    }
}

/// Resolve the per-frame configuration from props.
///
/// The requested size only bootstraps the first frame before the commit pass
/// has published a content box; the editor never derives its own content
/// geometry, so padding and borders are never subtracted here.
fn config_from(props: &Props<RawInputProps>, emoji_merging: EmojiMerging) -> Config {
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
            emoji_merging,
        },
        appearance: props.appearance.clone() | RawInputAppearance::default(),
        placeholder: props.placeholder.clone() | String::new(),
    }
}

/// Raw single- or multi-line text editor; see [`RawInputProps`] for its
/// configuration.
///
/// The editor never infers focus from receiving a key: the dispatcher routes a
/// targeted key here only when this region is the focused target, and the
/// `focus_event` listener records the transition.
///
/// A pointer gesture that lands on the editor belongs to the editor: it places
/// the caret and extends its own selection, and it stops propagation so a
/// selectable ancestor does not also select from the same gesture. Stopping
/// propagation leaves focus on press and any caller-supplied pointer listener
/// untouched, because neither is suppressed by it.
pub fn raw_input(cx: &mut ComponentContext, props: &Props<RawInputProps>) -> Node {
    let state_ref = cx.use_ref(InputState::default);
    let (_, redraw) = cx.use_state(|| 0_u64);

    let probe = {
        let cell = cx.use_ref(LayoutProbe::new);
        cell.lock().expect("layout probe poisoned").clone()
    };
    let config = Arc::new(config_from(props, probe.emoji_merging()));

    let (scroll_offset, text) = {
        let mut state = state_ref.lock().expect("input state poisoned");
        state.model.render(
            props.value.as_ref().map(String::as_str),
            props.default_value.as_ref().map(String::as_str),
            config.multiline,
            config.policy.emoji_merging,
        );
        if config.policy.disabled {
            state.dragging = false;
            state.reveal_caret = false;
        }
        let committed = probe.committed();
        let merging = committed
            .as_ref()
            .map_or(EmojiMerging::Merge, |committed| committed.emoji_merging);
        let (view_width, view_height) = match &committed {
            Some(committed) => (
                committed.viewport_width.max(1),
                committed.viewport_height.max(1),
            ),
            None => (config.width.max(1), config.height.max(1)),
        };
        let layout = build_layout_at(&state.model.value, &config, view_width, merging);
        reconcile_scroll(
            &mut state,
            &layout,
            view_width,
            view_height,
            config.scroll_axes(),
        );
        state.reveal_caret = false;
        let focused = state.focused && !config.policy.disabled;
        let scroll_offset = ScrollOffset::new(state.scroll_x as u32, state.scroll_y as u32);
        let surface = EditorSurface {
            value: state.model.value.clone(),
            selection: state
                .model
                .caret()
                .local_range(0, state.model.value().len()),
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
        let text = Text::from_spans(Vec::new())
            .layout_style(surface_style())
            .wrap(config.wrap)
            .editor_surface(surface, probe.clone());
        (scroll_offset, text)
    };

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
                let policy = EditPolicy {
                    emoji_merging: probe.emoji_merging(),
                    ..config.policy
                };
                if policy.disabled {
                    return;
                }
                let mut value_event = None;
                let mut clipboard_event = None;
                let mut submit_event = None;
                let mut caret_moved = false;
                let clipboard = clipboard_intent(&event);
                let copy_selection = clipboard == Some(ClipboardIntent::Copy);
                {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    if !config.multiline
                        && event.key.code == KeyCode::Enter
                        && !event.key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        event.stop_propagation();
                        submit_event = Some(TextValueEvent {
                            value: state.model.value().to_string(),
                        });
                    } else if clipboard == Some(ClipboardIntent::Cut)
                        && !state.model.caret().is_collapsed()
                    {
                        event.stop_propagation();
                        let outcome = state.model.reduce(EditAction::Cut, policy);
                        if outcome.changed {
                            state.reveal_caret = true;
                            value_event = outcome.value.map(|value| TextValueEvent { value });
                        }
                        if outcome.intent == EditIntent::Cut
                            && let Some(text) = outcome.removed_text
                        {
                            clipboard_store(&text);
                            clipboard_event = Some(TextClipboardEvent {
                                action: TextClipboardAction::Cut,
                                text,
                            });
                        }
                    } else if clipboard == Some(ClipboardIntent::Paste) {
                        event.stop_propagation();
                        if let Some(text) = clipboard_load() {
                            let outcome = state.model.reduce(EditAction::Paste(text), policy);
                            if outcome.changed {
                                state.reveal_caret = true;
                                value_event = outcome.value.map(|value| TextValueEvent { value });
                            }
                        }
                    } else if let Some(action) = key_action(&event, &config) {
                        event.stop_propagation();
                        let outcome = if let Some((direction, steps)) = vertical_steps(&action) {
                            let document = committed_document(&probe, &state, &config);
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
                            state
                                .model
                                .vertical_move(&document, direction, steps, extend, policy)
                        } else {
                            state.model.reduce(action, policy)
                        };
                        if outcome.changed {
                            value_event = outcome.value.map(|value| TextValueEvent { value });
                        }
                        caret_moved = outcome.reveal_caret;
                        state.reveal_caret |= outcome.reveal_caret;
                        if outcome.intent == EditIntent::Cut
                            && let Some(text) = outcome.removed_text
                        {
                            clipboard_event = Some(TextClipboardEvent {
                                action: TextClipboardAction::Cut,
                                text,
                            });
                        }
                    } else if copy_selection && let Some(text) = state.model.selected_text() {
                        event.stop_propagation();
                        clipboard_store(&text);
                        clipboard_event = Some(TextClipboardEvent {
                            action: TextClipboardAction::Copy,
                            text,
                        });
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
                if value_event.is_some() || caret_moved {
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
        let probe = probe.clone();
        let on_change = on_change.clone();
        let caller = caller.paste_event.as_ref().cloned();
        EventListener::compose(
            move |event: PasteEvent| {
                let policy = EditPolicy {
                    emoji_merging: probe.emoji_merging(),
                    ..config.policy
                };
                if policy.disabled || policy.read_only {
                    return;
                }
                let (changed, value_event) = {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    let outcome = state
                        .model
                        .reduce(EditAction::Paste(event.text.to_string()), policy);
                    if outcome.changed {
                        state.reveal_caret = true;
                    }
                    (
                        outcome.changed,
                        outcome.value.map(|value| TextValueEvent { value }),
                    )
                };
                if changed {
                    redraw.update(|value| *value += 1);
                }
                if let Some(listener) = &on_change
                    && let Some(event) = value_event
                {
                    listener.call(event);
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
                let policy = EditPolicy {
                    emoji_merging: probe.emoji_merging(),
                    ..config.policy
                };
                if policy.disabled || !event.is_primary_button() {
                    return;
                }
                event.stop_propagation();
                {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    let offset = pointer_offset(&probe, &state, &config, event.local_position);
                    let extend = event.modifiers.contains(KeyModifiers::SHIFT);
                    state
                        .model
                        .reduce(EditAction::PlaceCaret { offset, extend }, policy);
                    if !extend {
                        state.pressed = offset;
                    }
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
                let policy = EditPolicy {
                    emoji_merging: probe.emoji_merging(),
                    ..config.policy
                };
                let mut state = state_ref.lock().expect("input state poisoned");
                if !state.dragging {
                    return;
                }
                event.stop_propagation();
                let committed = committed_document(&probe, &state, &config);
                let point = document_point(&probe, event.local_position);
                let pressed = state.pressed;
                state.model.drag_selection(&committed, point, pressed);
                let _ = policy;
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
            move |event: PointerEvent| {
                let was_dragging = {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    let was_dragging = state.dragging;
                    state.dragging = false;
                    was_dragging
                };
                if was_dragging {
                    event.stop_propagation();
                }
            },
            caller,
        )
    };

    let pointer_cancel = {
        let state_ref = state_ref.clone();
        let caller = caller.pointer_cancel.as_ref().cloned();
        EventListener::compose(
            move |event: PointerEvent| {
                let was_dragging = {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    let was_dragging = state.dragging;
                    state.dragging = false;
                    was_dragging
                };
                if was_dragging {
                    event.stop_propagation();
                }
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
                    state.reveal_caret = false;
                }
                redraw.update(|value| *value += 1);
            },
            caller,
        )
    };

    let mut scroll_host = props.host_props(DomProps::default());
    scroll_host.focusable = Attr::Set(!config.policy.disabled);
    scroll_host.autofocus = (props.autofocus | false) && !config.policy.disabled;
    if focus_active(&state_ref, &config)
        && let Some(color) = config.appearance.focused_border
    {
        scroll_host.style.border.foreground = Attr::Set(color);
    }
    let scroll_axes = config.scroll_axes();
    let disabled = config.policy.disabled;
    let host_height = if config.multiline {
        Dimension::Max
    } else {
        Dimension::Cells(1)
    };
    let scrollable = if disabled {
        ui! {
            <view style={move |style| {
                style.width /= Dimension::Max;
                style.height /= host_height;
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
                style={move |style| {
                    style.width /= Dimension::Max;
                    style.height /= host_height;
                }}
            >
                {text}
            </scroll_area>
        }
    };

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

/// Whether the control currently holds focus and is enabled.
fn focus_active(state_ref: &crate::basic::Ref<InputState>, config: &Config) -> bool {
    let state = state_ref.lock().expect("input state poisoned");
    state.focused && !config.policy.disabled
}

/// Lay `value` out at an explicit width with the given emoji-merging mode.
fn build_layout_at(
    value: &str,
    config: &Config,
    width: usize,
    merging: EmojiMerging,
) -> text_layout::TextLayout {
    text_layout::layout_text(
        &Text::new(value).wrap(config.wrap),
        width.max(1),
        ComputedText::default(),
        merging,
        |parent, _| parent,
        |style| *style,
    )
}

/// Lay `value` out at the configuration's bootstrap width.
fn build_layout(value: &str, config: &Config, merging: EmojiMerging) -> text_layout::TextLayout {
    build_layout_at(value, config, config.width, merging)
}

/// Describe the committed frame as the selection engine's one-segment document,
/// so vertical motion and pointer hit testing share the engine's rows with
/// every other selectable surface.
fn committed_document(
    probe: &LayoutProbe,
    state: &InputState,
    config: &Config,
) -> SelectionDocument {
    let layout = committed_layout(probe, state, config);
    let viewport = probe.committed().map_or(config.height.max(1), |committed| {
        committed.viewport_height.max(1)
    });
    SelectionDocument::single(
        Arc::from(state.model.value()),
        layout,
        probe.emoji_merging(),
        viewport,
    )
}

/// The committed layout, shared rather than copied: pointer and vertical
/// queries only read it, and deep-cloning a layout per event scaled with the
/// document.
fn committed_layout(
    probe: &LayoutProbe,
    state: &InputState,
    config: &Config,
) -> Arc<text_layout::TextLayout> {
    probe
        .committed()
        .map(|committed| committed.layout.clone())
        .unwrap_or_else(|| {
            Arc::new(build_layout(
                &state.model.value,
                config,
                probe.emoji_merging(),
            ))
        })
}

/// Bring the caret into view and clamp the offset to the extent the runtime
/// will actually apply.
///
/// The extent is a property of the layout, the viewport, and the axes the host
/// actually scrolls on; requesting an offset on a disabled axis would be
/// silently refused and leave requested and painted offsets out of step.
fn reconcile_scroll(
    state: &mut InputState,
    layout: &text_layout::TextLayout,
    view_width: usize,
    view_height: usize,
    axes: ScrollAxes,
) {
    let view_width = view_width.max(1);
    let view_height = view_height.max(1);
    let horizontal = matches!(axes, ScrollAxes::Horizontal | ScrollAxes::Both);
    let vertical = matches!(axes, ScrollAxes::Vertical | ScrollAxes::Both);
    let max_x = if horizontal {
        layout.max_row_width().saturating_sub(view_width)
    } else {
        0
    };
    let max_y = if vertical {
        layout.row_count().max(1).saturating_sub(view_height)
    } else {
        0
    };
    state.scroll_x = state.scroll_x.min(max_x);
    state.scroll_y = state.scroll_y.min(max_y);

    if state.focused && state.reveal_caret {
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
    state.scroll_x = state.scroll_x.min(max_x);
    state.scroll_y = state.scroll_y.min(max_y);
}

/// Style for the editor's text leaf.
///
/// The leaf keeps its own text extent: the scroll host pans a single unwrapped
/// line by exactly that extent, and pointer mapping, wrapping and hit testing
/// all read the same box.
fn surface_style() -> Style {
    Style::default()
}

/// Map a vertical navigation action to a direction and step count.
fn vertical_steps(action: &EditAction) -> Option<(i32, usize)> {
    match action {
        EditAction::MoveUp { .. } => Some((-1, 1)),
        EditAction::MoveDown { .. } => Some((1, 1)),
        EditAction::PageUp { rows, .. } => Some((-1, (*rows).max(1))),
        EditAction::PageDown { rows, .. } => Some((1, (*rows).max(1))),
        _ => None,
    }
}

/// Derive the editor's action vocabulary from the selection engine's one
/// keymap.
///
/// The component contributes only the policy the engine has no opinion on: a
/// single-line control has no row or page motion, and Control means a word step
/// here rather than any other editor convention.
fn key_action(event: &KeyboardEvent, config: &Config) -> Option<EditAction> {
    let modifiers = event.key.modifiers;
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    if let Some(motion) = motion_for(event) {
        return match motion {
            SelectionMotion::CharLeft => Some(EditAction::MoveLeft {
                extend: shift,
                word: false,
            }),
            SelectionMotion::CharRight => Some(EditAction::MoveRight {
                extend: shift,
                word: false,
            }),
            SelectionMotion::WordLeft => Some(EditAction::MoveLeft {
                extend: shift,
                word: true,
            }),
            SelectionMotion::WordRight => Some(EditAction::MoveRight {
                extend: shift,
                word: true,
            }),
            SelectionMotion::RowStart => Some(EditAction::Home {
                extend: shift,
                document: false,
            }),
            SelectionMotion::RowEnd => Some(EditAction::End {
                extend: shift,
                document: false,
            }),
            SelectionMotion::DocStart => Some(EditAction::Home {
                extend: shift,
                document: true,
            }),
            SelectionMotion::DocEnd => Some(EditAction::End {
                extend: shift,
                document: true,
            }),
            SelectionMotion::RowUp if config.multiline => {
                Some(EditAction::MoveUp { extend: shift })
            }
            SelectionMotion::RowDown if config.multiline => {
                Some(EditAction::MoveDown { extend: shift })
            }
            SelectionMotion::PageUp if config.multiline => Some(EditAction::PageUp {
                extend: shift,
                rows: config.height,
            }),
            SelectionMotion::PageDown if config.multiline => Some(EditAction::PageDown {
                extend: shift,
                rows: config.height,
            }),
            _ => None,
        };
    }
    if clipboard_intent(event) == Some(ClipboardIntent::SelectAll) {
        return Some(EditAction::SelectAll);
    }
    match event.key.code {
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

/// Convert a pointer position into the document coordinates the selection
/// engine hit tests in.
///
/// The runtime reports the pointer in the coordinates of the region it hit,
/// which is the child as painted; that frame was laid out with exactly the
/// committed applied offset, so adding that offset back once is the whole
/// conversion. The press and every drag endpoint go through here, so a
/// selection can never be expressed in a different space from the glyphs it
/// covers.
fn document_point(probe: &LayoutProbe, local: crate::ScreenPosition) -> DocPoint {
    let (applied_x, applied_y) = probe.committed().map_or((0, 0), |committed| {
        (committed.applied_x, committed.applied_y)
    });
    DocPoint::Point {
        line: local.line.max(0) + applied_y as i32,
        column: local.column.max(0) + applied_x as i32,
    }
}

/// Resolve a pointer position to a document offset.
///
/// The row/cell math lives in the selection engine's document, so the editor
/// and every other selectable surface hit test identically. Leading bias places
/// the selection on the glyph the pointer is over, not after it, so clicking a
/// cell and dragging over text behaves the same here and in a selectable
/// region.
fn pointer_offset(
    probe: &LayoutProbe,
    state: &InputState,
    config: &Config,
    local: crate::ScreenPosition,
) -> usize {
    let document = committed_document(probe, state, config);
    document.hit(document_point(probe, local), HitBias::Leading)
}
