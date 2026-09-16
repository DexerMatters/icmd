//! Selectable text region that delegates selection arithmetic to the shared
//! selection engine.

use std::ops::Range;
use std::sync::Arc;

use crossterm::event::KeyModifiers;

use crate::basic::selection::{
    ClipboardIntent, DocPoint, Selection, SelectionConfig, SelectionDocument, SelectionProbe,
    SelectionStyles, clipboard_intent, clipboard_store, extends_selection, motion_for,
};
use crate::basic::text_layout::HitBias;
use crate::{
    Align, Attr, Dimension, DomProps, EventListener, FocusEvent, Justify, KeyboardEvent, Layout,
    Node, PointerEvent, Props, TextStyle, basic::ComponentContext, ui, view,
};

use super::input::{TextClipboardAction, TextClipboardEvent};

/// A selection change, reported after it happens.
///
/// The range is in document bytes: the region's text leaves concatenated in
/// paint order with a newline between blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSelectionEvent {
    /// Byte range of the selection in the region's document.
    pub range: Range<usize>,
    /// Selected text sliced from the committed document.
    pub text: String,
}

/// The region's selection is exactly the engine's interval; the component adds
/// only the interaction state around it (whether a drag is in progress, whether
/// the region is focused, and what was last reported).
#[derive(Default)]
struct AreaState {
    /// The engine's selection interval.
    selection: Selection,
    /// Whether a pointer drag is in progress.
    dragging: bool,
    /// The document offset the press landed on, kept so a drag can resolve the
    /// glyph under the pointer inclusively in both directions.
    pressed: usize,
    /// Whether the region currently holds focus.
    focused: bool,
    /// The last range reported to `on_selection_change`.
    reported: Option<(usize, usize)>,
}

/// Configuration for [`selection_area`].
#[derive(Clone, Default)]
pub struct SelectionAreaProps {
    /// Style for the active selection; defaults to the theme's active
    /// selection style.
    pub selection_style: Attr<TextStyle>,
    /// Style for the selection while the region is unfocused; defaults to the
    /// theme's inactive selection style.
    pub selection_inactive_style: Attr<TextStyle>,
    /// Whether selection and focus are refused, making the region a selection
    /// barrier; defaults to `false`.
    ///
    /// A disabled region still delimits its subtree: the text below it belongs
    /// to no ancestor's document, and a primary-button press inside it is
    /// consumed rather than handed to an ancestor. That is how a subtree is kept
    /// out of a surrounding selectable region - an embedded example, say - while
    /// an enabled region nested inside it remains selectable on its own terms.
    pub disabled: Attr<bool>,
    /// Whether the region requests focus on mount; defaults to `false`.
    pub autofocus: Attr<bool>,
    /// Keyboard selection is opt-in per region. Turning it off leaves pointer
    /// selection working, which also means Ctrl+C is no longer answered here.
    /// Defaults to `true`.
    pub enable_keyboard: Attr<bool>,
    /// Listener for the clipboard Copy requests answered by the region.
    pub on_clipboard: Attr<EventListener<TextClipboardEvent>>,
    /// Listener for each distinct selection range after it changes.
    pub on_selection_change: Attr<EventListener<TextSelectionEvent>>,
}

impl std::fmt::Debug for SelectionAreaProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionAreaProps")
            .field("selection_style", &self.selection_style)
            .field("selection_inactive_style", &self.selection_inactive_style)
            .field("disabled", &self.disabled)
            .field("autofocus", &self.autofocus)
            .field("enable_keyboard", &self.enable_keyboard)
            .finish_non_exhaustive()
    }
}

/// A region makes the text below it selectable. The component owns no selection
/// arithmetic at all: it describes its painted text as the selection engine's
/// document and delegates every placement, motion, hit test, and copy to that
/// engine - the same one the editor controls drive.
///
/// The committed frame is the authority on where the text is, so the selection
/// is snapped into the document it describes and content that changed between
/// frames can never leave an out-of-range selection. The live selection is a
/// plain value on the node rather than shared mutable state, so a change always
/// changes the lowered tree and the renderer can never skip the repaint.
///
/// A disabled region is a barrier rather than an absence: it still owns the
/// document of its subtree, so no ancestor can select that text, and it consumes
/// a primary-button press so a drag inside it selects nothing instead of
/// selecting the surrounding prose. Its pointer listeners are attached for that
/// reason while its keyboard and focus listeners stay off, because a barrier is
/// not focusable. An enabled region nested inside a barrier takes over its own
/// subtree and stays fully selectable.
pub fn selection_area(cx: &mut ComponentContext, props: &Props<SelectionAreaProps>) -> Node {
    let theme = cx.use_theme();
    let state_ref = cx.use_ref(AreaState::default);
    let (_, redraw) = cx.use_state(|| 0_u64);
    let probe = {
        let cell = cx.use_ref(SelectionProbe::new);
        cell.lock().expect("selection probe poisoned").clone()
    };

    let disabled = props.disabled | false;
    let enable_keyboard = props.enable_keyboard | true;
    let theme_styles = SelectionStyles::from_theme(&theme);
    let styles = SelectionStyles {
        active: props
            .selection_style
            .as_ref()
            .cloned()
            .unwrap_or(theme_styles.active),
        inactive: props
            .selection_inactive_style
            .as_ref()
            .cloned()
            .unwrap_or(theme_styles.inactive),
    };

    let committed = probe.committed();
    let (selection, focused) = {
        let mut state = state_ref.lock().expect("selection area poisoned");
        if let Some(committed) = &committed {
            state.selection.clamp(&committed.document);
        }
        (state.selection, state.focused && !disabled)
    };

    let config = Arc::new(SelectionConfig {
        probe: probe.clone(),
        selection,
        focused,
        styles,
    });

    let caller = props.dom.events.clone();
    let on_clipboard = props.on_clipboard.as_ref().cloned();
    let on_selection_change = props.on_selection_change.as_ref().cloned();

    let pointer_down = {
        let state_ref = state_ref.clone();
        let redraw = redraw.clone();
        let probe = probe.clone();
        let on_selection_change = on_selection_change.clone();
        let caller = caller.pointer_down.as_ref().cloned();
        EventListener::compose(
            move |event: PointerEvent| {
                if !event.is_primary_button() {
                    return;
                }
                if disabled {
                    event.stop_propagation();
                    return;
                }
                let Some(committed) = probe.committed() else {
                    return;
                };
                event.stop_propagation();
                let extend = event.modifiers.contains(KeyModifiers::SHIFT);
                let changed = {
                    let mut state = state_ref.lock().expect("selection area poisoned");
                    state.dragging = true;
                    state.selection.place(
                        &committed.document,
                        DocPoint::Point {
                            line: event.position.line,
                            column: event.position.column,
                        },
                        HitBias::Leading,
                        extend,
                    );
                    if !extend {
                        state.pressed = state.selection.anchor;
                    }
                    report_selection(&mut state, &committed.document, &on_selection_change)
                };
                if changed {
                    redraw.update(|value| *value += 1);
                }
            },
            caller,
        )
    };

    let pointer_move = {
        let state_ref = state_ref.clone();
        let redraw = redraw.clone();
        let probe = probe.clone();
        let on_selection_change = on_selection_change.clone();
        let caller = caller.pointer_move.as_ref().cloned();
        EventListener::compose(
            move |event: PointerEvent| {
                let mut state = state_ref.lock().expect("selection area poisoned");
                if !state.dragging {
                    return;
                }
                let Some(committed) = probe.committed() else {
                    return;
                };
                let pressed = state.pressed;
                state.selection.drag_to(
                    &committed.document,
                    DocPoint::Point {
                        line: event.position.line,
                        column: event.position.column,
                    },
                    pressed,
                );
                let changed =
                    report_selection(&mut state, &committed.document, &on_selection_change);
                drop(state);
                if changed {
                    redraw.update(|value| *value += 1);
                }
            },
            caller,
        )
    };

    let pointer_up = {
        let state_ref = state_ref.clone();
        let caller = caller.pointer_up.as_ref().cloned();
        EventListener::compose(
            move |_event: PointerEvent| {
                state_ref.lock().expect("selection area poisoned").dragging = false;
            },
            caller,
        )
    };

    let pointer_cancel = {
        let state_ref = state_ref.clone();
        let caller = caller.pointer_cancel.as_ref().cloned();
        EventListener::compose(
            move |_event: PointerEvent| {
                state_ref.lock().expect("selection area poisoned").dragging = false;
            },
            caller,
        )
    };

    let key = {
        let state_ref = state_ref.clone();
        let redraw = redraw.clone();
        let probe = probe.clone();
        let on_clipboard = on_clipboard.clone();
        let on_selection_change = on_selection_change.clone();
        let caller = caller.key_down.as_ref().cloned();
        EventListener::compose(
            move |event: KeyboardEvent| {
                if disabled || !enable_keyboard {
                    return;
                }
                let Some(committed) = probe.committed() else {
                    return;
                };
                if let Some(intent) = clipboard_intent(&event) {
                    let mut state = state_ref.lock().expect("selection area poisoned");
                    match intent {
                        ClipboardIntent::Copy => {
                            if let Some(text) = state.selection.text(&committed.document) {
                                event.stop_propagation();
                                clipboard_store(&text);
                                if let Some(listener) = &on_clipboard {
                                    listener.call(TextClipboardEvent {
                                        action: TextClipboardAction::Copy,
                                        text,
                                    });
                                }
                            }
                        }
                        ClipboardIntent::SelectAll => {
                            event.stop_propagation();
                            state.selection.select_all(&committed.document);
                            report_selection(&mut state, &committed.document, &on_selection_change);
                            drop(state);
                            redraw.update(|value| *value += 1);
                        }
                        ClipboardIntent::Cut | ClipboardIntent::Paste => {}
                    }
                    return;
                }
                let Some(motion) = motion_for(&event) else {
                    return;
                };
                let mut state = state_ref.lock().expect("selection area poisoned");
                let extend = extends_selection(&event);
                if !extend && state.selection.is_collapsed() {
                    return;
                }
                event.stop_propagation();
                state.selection.apply(&committed.document, motion, extend);
                let changed =
                    report_selection(&mut state, &committed.document, &on_selection_change);
                drop(state);
                if changed {
                    redraw.update(|value| *value += 1);
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
                let mut state = state_ref.lock().expect("selection area poisoned");
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

    let mut host = props.host_props(DomProps::default());
    host.style.layout /= Layout::Vertical;
    host.style.justify /= Justify::Start;
    host.style.align /= Align::Start;
    host.style.width /= Dimension::Max;
    host.style.text = theme.typography.body.clone();
    host.focusable = Attr::Set(!disabled);
    host.autofocus = (props.autofocus | false) && !disabled;
    host = host.with_selection_host(config);

    let children = props.children_node();
    if disabled {
        ui! {
            <view key="selection-area-disabled" dom={host}
                on_pointer_down={pointer_down}
                on_pointer_move={pointer_move}
                on_pointer_up={pointer_up}
                on_pointer_cancel={pointer_cancel}
            >
                {children}
            </view>
        }
    } else {
        ui! {
            <view key="selection-area-enabled" dom={host}
                on_pointer_down={pointer_down}
                on_pointer_move={pointer_move}
                on_pointer_up={pointer_up}
                on_pointer_cancel={pointer_cancel}
                on_key_down={key}
                on_focus_event={focus}
            >
                {children}
            </view>
        }
    }
}

/// Record the selection's new range and report it once per distinct range; the
/// text is sliced by the engine from the document that was actually committed.
fn report_selection(
    state: &mut AreaState,
    document: &SelectionDocument,
    listener: &Option<EventListener<TextSelectionEvent>>,
) -> bool {
    if state.selection.is_collapsed() {
        let changed = state.reported.take().is_some();
        return changed;
    }
    let (start, end) = state.selection.range();
    if state.reported == Some((start, end)) {
        return false;
    }
    state.reported = Some((start, end));
    if let Some(listener) = listener {
        listener.call(TextSelectionEvent {
            range: start..end,
            text: document.slice(start..end),
        });
    }
    true
}
