//! `raw_input`: the one primitive that owns text-entry behavior.
//!
//! The component renders a single scroll-capable host. That host is the focus
//! target, receives caller `DomProps`, and carries composed internal-plus-caller
//! event listeners. `input` and `textarea` are thin policy wrappers over it.
//!
//! Geometry - wrapping, caret placement, pointer hit-testing, and scroll
//! extents - comes from the canonical layout in [`crate::basic::text_layout`];
//! the editor does not implement its own wrapping.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

use crate::basic::text_layout::{self, ComputedText, HitBias, ItemKind};
use crate::{
    Attr, Dimension, DomProps, Edges, EventListener, FocusEvent, KeyboardEvent, Node, PasteEvent,
    PointerEvent, Props, ScrollAxes, ScrollEvent, ScrollOffset, ScrollbarVisibility, Span, Style,
    Text, TextStyle, TextWrap, basic::ComponentContext, scroll_area, ui, view,
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
}

impl Default for RawInputAppearance {
    fn default() -> Self {
        Self {
            placeholder: TextStyle::default().dim(),
            selection: TextStyle::default().reverse(),
            selection_inactive: TextStyle::default().dim(),
            caret: TextStyle::default().reverse(),
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
}

/// Immutable render-time configuration, shared with event handlers.
struct Config {
    multiline: bool,
    wrap: TextWrap,
    width: usize,
    height: usize,
    policy: EditPolicy,
    appearance: RawInputAppearance,
    placeholder: String,
}

impl Config {
    fn scroll_axes(&self) -> ScrollAxes {
        if self.multiline {
            ScrollAxes::Both
        } else {
            ScrollAxes::Horizontal
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
    let padding = props
        .dom
        .style
        .padding
        .as_ref()
        .copied()
        .unwrap_or(Edges::all(0));
    let inset = usize::from(padding.left) + usize::from(padding.right);
    Config {
        multiline,
        wrap,
        width: width.saturating_sub(inset).max(1),
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

    // Reconcile the owner's value, rebuild the layout that pointer handlers
    // read, and bring the caret into view when it moved.
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
        let layout = build_layout(&state.model.value, &config);
        reconcile_scroll(&mut state, &layout, &config);
        state.reveal_caret = false;
        let focused = state.focused && !config.policy.disabled;
        let scroll_offset = ScrollOffset::new(state.scroll_x as u32, state.scroll_y as u32);
        let text = surface_text(&state, &layout, &config, focused);
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
                    } else if let Some(action) = key_action(&event, &config) {
                        event.stop_propagation();
                        let outcome = state.model.reduce(action, config.policy);
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
                if value_event.is_some() || focus_gained {
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
        let caller = caller.pointer_down.as_ref().cloned();
        EventListener::compose(
            move |event: PointerEvent| {
                if config.policy.disabled || !event.is_primary_button() {
                    return;
                }
                {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    state.focused = true;
                    let layout = build_layout(&state.model.value, &config);
                    let offset = hit_offset(
                        &layout,
                        event.local_position.column,
                        event.local_position.line,
                        state.scroll_x,
                        state.scroll_y,
                    );
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
        let caller = caller.pointer_move.as_ref().cloned();
        EventListener::compose(
            move |event: PointerEvent| {
                let mut state = state_ref.lock().expect("input state poisoned");
                if !state.dragging {
                    return;
                }
                let layout = build_layout(&state.model.value, &config);
                let offset = hit_offset(
                    &layout,
                    event.local_position.column,
                    event.local_position.line,
                    state.scroll_x,
                    state.scroll_y,
                );
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
                if event == FocusEvent::Lost {
                    let mut state = state_ref.lock().expect("input state poisoned");
                    if state.focused {
                        state.focused = false;
                        state.dragging = false;
                        drop(state);
                        redraw.update(|value| *value += 1);
                    }
                }
            },
            caller,
        )
    };

    let scroll = {
        let state_ref = state_ref.clone();
        let caller = caller.scroll.as_ref().cloned();
        EventListener::compose(
            move |event: ScrollEvent| {
                let mut state = state_ref.lock().expect("input state poisoned");
                state.scroll_x = event.offset.x as usize;
                state.scroll_y = event.offset.y as usize;
                state.reveal_caret = false;
            },
            caller,
        )
    };

    // One host: caller DOM props, explicit focusability, and the scroll engine.
    let mut host = props.host_props(DomProps::default());
    host.focusable = !config.policy.disabled;
    let content = if config.policy.disabled {
        ui! {
            <view style={|style| {
                style.width /= Dimension::Max;
                style.height /= Dimension::Max;
            }}>{text}</view>
        }
    } else {
        ui! {
            <scroll_area
                axes={config.scroll_axes()}
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

    ui! {
        <view dom={host}
            on_key_down={key}
            on_paste_event={paste}
            on_pointer_down={pointer_down}
            on_pointer_move={pointer_move}
            on_pointer_up={pointer_up}
            on_pointer_cancel={pointer_cancel}
            on_focus_event={focus}
            on_scroll={scroll}
        >
            {content}
        </view>
    }
}

fn build_layout(value: &str, config: &Config) -> text_layout::TextLayout {
    text_layout::layout_text(
        &Text::new(value).wrap(config.wrap),
        config.width,
        ComputedText::default(),
        |parent, _| parent,
        |style| *style,
    )
}

/// Clamp the viewport and, when requested, bring the caret into view.
fn reconcile_scroll(state: &mut InputState, layout: &text_layout::TextLayout, config: &Config) {
    let rows = layout.row_count().max(1);
    state.scroll_y = state.scroll_y.min(rows.saturating_sub(config.height));
    let widest = layout.max_row_width();
    state.scroll_x = state.scroll_x.min(widest.saturating_sub(config.width));
    if !state.focused || !state.reveal_caret {
        return;
    }
    let (row, cell, caret_width) = layout.caret(state.model.caret().cursor);
    if row < state.scroll_y {
        state.scroll_y = row;
    } else if row >= state.scroll_y.saturating_add(config.height) {
        state.scroll_y = row + 1 - config.height;
    }
    if cell < state.scroll_x {
        state.scroll_x = cell;
    } else {
        let caret_end = cell.saturating_add(caret_width.max(1));
        if caret_end > state.scroll_x.saturating_add(config.width) {
            state.scroll_x = caret_end - config.width;
        }
    }
}

fn surface_text(
    state: &InputState,
    layout: &text_layout::TextLayout,
    config: &Config,
    focused: bool,
) -> Text {
    let value = &state.model.value;
    let mut spans = Vec::new();
    if value.is_empty() {
        if config.placeholder.is_empty() {
            if focused {
                spans.push(Span::new(" ").style(config.appearance.caret.clone()));
            }
        } else {
            for (index, grapheme) in config.placeholder.graphemes(true).enumerate() {
                let mut span = Span::new(grapheme).style(config.appearance.placeholder.clone());
                if focused && index == 0 {
                    span = span.style(config.appearance.caret.clone());
                }
                spans.push(span);
            }
        }
        return Text::from_spans(spans)
            .with_style(surface_style())
            .wrap(TextWrap::NoWrap);
    }
    let selection = if state.model.caret().is_collapsed() {
        None
    } else {
        Some(state.model.caret().range())
    };
    let selection_style = if focused {
        config.appearance.selection.clone()
    } else {
        config.appearance.selection_inactive.clone()
    };
    let caret = state.model.caret().cursor;
    for item in layout.items() {
        if item.kind == ItemKind::Separator {
            continue;
        }
        let symbol = if item.symbol == "\t" {
            " ".repeat(item.width)
        } else {
            item.symbol.clone()
        };
        let mut style = TextStyle::default();
        if let Some((start, end)) = selection
            && item.source.start < end
            && item.source.end > start
        {
            style = selection_style.clone();
        }
        if focused && caret >= item.source.start && caret < item.source.end {
            style = config.appearance.caret.clone();
        }
        spans.push(Span::new(symbol).style(style));
    }
    if focused && caret == value.len() {
        spans.push(Span::new(" ").style(config.appearance.caret.clone()));
    }
    Text::from_spans(spans)
        .with_style(surface_style())
        .wrap(TextWrap::NoWrap)
}

fn surface_style() -> Style {
    let mut style = Style::default();
    style.width /= Dimension::Max;
    style.height /= Dimension::Max;
    style
}

fn is_copy(event: &KeyboardEvent) -> bool {
    event.key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(event.key.code, KeyCode::Char('c' | 'C'))
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
fn hit_offset(
    layout: &text_layout::TextLayout,
    column: i32,
    line: i32,
    scroll_x: usize,
    scroll_y: usize,
) -> usize {
    let row = (line.max(0) as usize)
        .saturating_add(scroll_y)
        .min(layout.row_count().saturating_sub(1));
    let cell = (column.max(0) as usize).saturating_add(scroll_x);
    layout.hit(row, cell, POINTER_HIT_BIAS)
}
