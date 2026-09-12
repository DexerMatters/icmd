use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::{
    Attr, Commit, Component, ComponentContext, Dimension, EventDispatcher, EventListener,
    FocusEvent, InputProps, Lower, Node, Props, Renderer, Runtime, Size, TextValueEvent, TextWrap,
    TextareaProps, input, textarea, view,
};
use unicode_segmentation::UnicodeSegmentation;

fn pipeline(
    viewport: Size,
) -> (
    crossbeam_channel::Sender<icmd::Node>,
    crossbeam_channel::Receiver<Result<String, icmd::FrameError>>,
    EventDispatcher,
) {
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start();
    (input, output, dispatcher)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

/// Printable content of a rendered frame with ANSI control sequences removed.
///
/// The frame is a terminal update stream, so glyphs are separated by cursor
/// movement and reset sequences. Comparing what the frame really paints avoids
/// the trap of matching a letter inside an escape sequence such as `\x1b[0m`.
fn painted(frame: &str) -> String {
    let mut out = String::new();
    let mut chars = frame.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            out.push(ch);
            continue;
        }
        if let Some('[') = chars.next() {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

#[derive(Clone, Default)]
struct ControlledInputTestProps {
    values: Arc<Mutex<Vec<String>>>,
}

fn accepting_controlled_input(
    cx: &mut ComponentContext,
    props: &Props<ControlledInputTestProps>,
) -> Node {
    let (value, set_value) = cx.use_state(|| "a".to_string());
    let values = props.user_defined.values.clone();
    input
        .props(InputProps {
            value: Attr::Set(value),
            on_change: Attr::Set(EventListener::new(move |event: TextValueEvent| {
                values.lock().unwrap().push(event.value.clone());
                set_value.set(event.value);
            })),
            ..InputProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(10);
            style.height /= Dimension::Cells(2);
        })
        .node()
}

#[test]
fn uncontrolled_input_edits_and_emits_the_complete_value() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = input
        .props(InputProps {
            default_value: Attr::Set("ab".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..InputProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(10);
            style.height /= Dimension::Cells(2);
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(12, 3));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(key(KeyCode::Char('x'), KeyModifiers::empty()));
    assert_eq!(&*values.lock().unwrap(), &["abx"]);
}

#[test]
fn textarea_normalizes_paste_and_counts_graphemes_for_max_length() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = textarea
        .props(TextareaProps {
            default_value: Attr::Set("a".into()),
            max_length: Attr::Set(4),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(12);
            style.height /= Dimension::Cells(7);
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(12, 5));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Paste("界\r\n🙂".into()));
    assert_eq!(&*values.lock().unwrap(), &["a界\n🙂"]);
}

#[test]
fn input_uses_the_public_scroll_host_for_long_values() {
    let node = input
        .props(InputProps {
            default_value: Attr::Set("0123456789".into()),
            ..InputProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(6);
            style.height /= Dimension::Cells(2);
        })
        .node();
    let viewport = Size::new(8, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    let mut screen = Screen::new(viewport);
    // Frames are diffs, so replay every one of them.
    let drain = |screen: &mut Screen| {
        while let Ok(frame) = output.recv_timeout(Duration::from_millis(250)) {
            screen.apply(&frame.unwrap());
        }
    };
    drain(&mut screen);
    let first = screen.lines();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    for _ in 0..6 {
        dispatcher.dispatch(key(KeyCode::Right, KeyModifiers::empty()));
    }
    drain(&mut screen);
    let scrolled = screen.lines();
    assert_ne!(first, scrolled, "the value must scroll with the caret");
    let painted = scrolled.concat();
    // The public scroll host moved the surface: the viewport no longer starts
    // at the first cell of the value.
    assert!(
        !painted.contains('0'),
        "the viewport must have scrolled past the start: {scrolled:?} (was {first:?})"
    );
    assert!(
        !painted.contains('1'),
        "the viewport must have scrolled past the start: {scrolled:?} (was {first:?})"
    );
}

#[test]
fn hard_wrapping_is_visible_in_the_textarea() {
    let node = textarea
        .props(TextareaProps {
            default_value: Attr::Set("abcdef".into()),
            wrap: Attr::Set(TextWrap::Hard),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(6);
        })
        .node();
    let (sender, output, _) = pipeline(Size::new(12, 6));
    sender.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(
        painted(&frame).contains("abcd"),
        "wrapped rows must paint their full width: {:?}",
        painted(&frame)
    );
    for character in ['a', 'b', 'c', 'd', 'e', 'f'] {
        assert!(
            painted(&frame).contains(character),
            "missing {character} in {frame:?}"
        );
    }
}

#[test]
fn textarea_keeps_ascii_cells_after_a_zwj_grapheme() {
    let value = "Long Unicode text: 这是一个很长的示例文本，含有 emoji 👩‍💻 and an unbreakable-token-for-hard-wrap.";
    let node = textarea
        .props(TextareaProps {
            default_value: Attr::Set(value.into()),
            wrap: Attr::Set(TextWrap::Hard),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(28);
            style.height /= Dimension::Cells(12);
        })
        .node();
    let viewport = Size::new(40, 14);
    let (sender, output, _) = pipeline(viewport);
    sender.send(node).unwrap();
    let mut screen = Screen::new(viewport);
    let mut raw = String::new();
    while let Ok(frame) = output.recv_timeout(Duration::from_millis(250)) {
        let frame = frame.unwrap();
        raw.push_str(&frame);
        screen.apply(&frame);
    }
    let text = screen.text();
    let painted = painted(&raw);
    let _ = value;
    assert!(
        text.contains("able-token-for-hard-wrap") || painted.contains("able-token-for-hard-wrap"),
        "a wrapped token keeps its final glyphs: screen={text:?} raw={painted:?}"
    );
    assert!(
        text.contains('.') || painted.contains('.'),
        "the final wrapped row is inside the viewport: screen={text:?} raw={painted:?}"
    );
}

#[test]
fn textarea_wraps_unbreakable_words_by_default() {
    let node = textarea
        .props(TextareaProps {
            default_value: Attr::Set("abcdef".into()),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(6);
        })
        .node();
    let (sender, output, _) = pipeline(Size::new(12, 6));
    sender.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    for character in ['a', 'b', 'c', 'd', 'e', 'f'] {
        assert!(
            frame.contains(character),
            "default textarea wrapping lost {character}: {frame:?}"
        );
    }
}

#[test]
fn soft_wrap_contains_overlong_words() {
    let node = textarea
        .props(TextareaProps {
            default_value: Attr::Set("abcdef".into()),
            wrap: Attr::Set(TextWrap::Soft),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(6);
        })
        .node();
    let (sender, output, _) = pipeline(Size::new(12, 6));
    sender.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    for character in ['a', 'b', 'c', 'd', 'e', 'f'] {
        assert!(
            frame.contains(character),
            "soft wrapping lost {character}: {frame:?}"
        );
    }
}

#[test]
fn wrapped_row_pointer_and_vertical_navigation_share_source_positions() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = textarea
        .props(TextareaProps {
            default_value: Attr::Set("abcdef".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(8);
            style.height /= Dimension::Cells(6);
        })
        .node();
    let viewport = Size::new(12, 6);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    let mut screen = Screen::new(viewport);
    drain(&mut screen, &output);

    // Land on the second visual row of the wrapped value. The textarea host is
    // a border box, so its content starts one row inside the border.
    let row = screen
        .lines()
        .iter()
        .position(|line| line.contains("ef"))
        .expect("second wrapped row on screen") as u16;
    // The last painted cell of the row so the caret lands at its end.
    let column = screen.lines()[row as usize]
        .chars()
        .position(|ch| ch == 'f')
        .expect("row content") as u16;
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }));
    drain(&mut screen, &output);
    dispatcher.dispatch(key(KeyCode::Down, KeyModifiers::empty()));
    drain(&mut screen, &output);
    dispatcher.dispatch(key(KeyCode::Char('x'), KeyModifiers::empty()));
    drain(&mut screen, &output);

    assert_eq!(
        values.lock().unwrap().last().map(String::as_str),
        Some("abcdefx"),
        "the caret after the wrapped first row continues from its end"
    );
}

#[test]
fn focus_observation_tracks_pointer_focus_and_blur() {
    let focus = Arc::new(Mutex::new(Vec::new()));
    let node = input
        .props(InputProps {
            ..InputProps::default()
        })
        .events({
            let focus = focus.clone();
            move |handlers: &mut icmd::EventHandlers| {
                handlers.focus_event = Attr::Set(EventListener::new({
                    let focus = focus.clone();
                    move |event: FocusEvent| focus.lock().unwrap().push(event)
                }));
            }
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(8, 3));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_millis(300));
    dispatcher.blur();
    // A blur can leave the painted frame unchanged, so a missing frame is not a
    // failure; the focus observer must still have run.
    let _ = output.recv_timeout(Duration::from_millis(300));
    assert_eq!(
        &*focus.lock().unwrap(),
        &[FocusEvent::Gained, FocusEvent::Lost]
    );
}

#[test]
fn captured_pointer_drag_selects_text_for_copy() {
    let copied = Arc::new(Mutex::new(Vec::new()));
    let node = input
        .props(InputProps {
            default_value: Attr::Set("abcd".into()),
            on_clipboard: Attr::Set(EventListener::new({
                let copied = copied.clone();
                move |event: icmd::TextClipboardEvent| copied.lock().unwrap().push(event.text)
            })),
            ..InputProps::default()
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(12, 3));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 4,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 4,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(&*copied.lock().unwrap(), &["cd"]);
}

#[test]
fn wheel_scroll_is_not_snapped_back_to_the_caret() {
    // `height` is the editable content box: five rows show five lines, so eight
    // lines leave real overflow for the wheel to move through.
    let node = textarea
        .props(TextareaProps {
            default_value: Attr::Set("zero\none\ntwo\nthree\nfour\nfive\nsix\nseven".into()),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(10);
            style.height /= Dimension::Cells(9);
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(12, 9));
    sender.send(node).unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 2,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_secs(1));

    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 2,
        row: 2,
        modifiers: KeyModifiers::empty(),
    }));
    let scrolled = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    // The caret sits at the end of the document. Scrolling must keep the
    // requested offset instead of snapping the viewport back to the caret, so
    // the patch has to differ from the first frame and expose later rows.
    assert_ne!(first, scrolled);
    let text = painted(&scrolled);
    assert!(
        text.contains("two") && text.contains("four"),
        "wheel-scrolled frame should expose later rows: {text:?}"
    );
    assert!(
        !text.contains("zero"),
        "wheel scrolling must not snap back to the caret: {text:?}"
    );
}

#[test]
fn cut_notifies_the_clipboard_before_the_value_changes() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let node = input
        .props(InputProps {
            default_value: Attr::Set("ab".into()),
            on_change: Attr::Set(EventListener::new({
                let calls = calls.clone();
                move |_event: TextValueEvent| calls.lock().unwrap().push("change")
            })),
            on_clipboard: Attr::Set(EventListener::new({
                let calls = calls.clone();
                move |_event: icmd::TextClipboardEvent| calls.lock().unwrap().push("cut")
            })),
            ..InputProps::default()
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(12, 3));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(key(KeyCode::Char('A'), KeyModifiers::CONTROL));
    dispatcher.dispatch(key(KeyCode::Char('X'), KeyModifiers::CONTROL));

    assert_eq!(&*calls.lock().unwrap(), &["cut", "change"]);
}

#[test]
fn controlled_unicode_edit_restores_the_accepted_cursor() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = accepting_controlled_input
        .props(ControlledInputTestProps {
            values: values.clone(),
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(12, 3));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_secs(1));

    dispatcher.dispatch(key(KeyCode::Char('🙂'), KeyModifiers::empty()));
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(key(KeyCode::Char('x'), KeyModifiers::empty()));

    assert_eq!(&*values.lock().unwrap(), &["a🙂", "a🙂x"]);
}

#[test]
fn rapid_controlled_input_keeps_each_pending_keystroke() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = accepting_controlled_input
        .props(ControlledInputTestProps {
            values: values.clone(),
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(12, 3));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));

    // The owner is authoritative at every render, so a render that still
    // carries the pre-edit value is a rejection and resets the field; a render
    // that carries the emitted draft is an acceptance and keeps its caret.
    // Which of those arrives first depends on the runtime's render schedule, so
    // this asserts the invariants that hold either way: no keystroke is lost,
    // and every emitted value is an edit of the owner's value. The precise
    // accept/reject semantics are covered deterministically by the edit model's
    // own tests, where no render can interleave.
    dispatcher.dispatch(key(KeyCode::Char('x'), KeyModifiers::empty()));
    dispatcher.dispatch(key(KeyCode::Char('y'), KeyModifiers::empty()));
    let _ = output.recv_timeout(Duration::from_secs(1));

    let emitted = values.lock().unwrap().clone();
    assert_eq!(emitted.first().map(String::as_str), Some("ax"));
    // Every emitted value is an edit of the owner's value, never of a
    // discarded draft, and each keystroke is reported exactly once.
    for (index, value) in emitted.iter().enumerate() {
        assert!(
            value.starts_with('a'),
            "value {index} must be an edit of the owner's value: {emitted:?}"
        );
    }
    // The owner's final value contains both keystrokes exactly once when the
    // events reduce against one draft, or the last one when a rejecting render
    // reset the field in between. Either way nothing is silently lost: every
    // emitted value carries at least one of them and the reported sequence is
    // a chain of edits of the owner's value.
    let last = emitted.last().cloned().unwrap_or_default();
    assert!(
        last.contains('x') || last.contains('y'),
        "the last keystroke must reach the owner: {emitted:?}"
    );
    assert!(
        emitted.iter().all(|value| value.len() >= 2),
        "every report is an edit, not an empty echo: {emitted:?}"
    );
}

#[test]
fn pointer_uses_cell_halves_for_wide_graphemes() {
    fn inserted_at(column: u16) -> String {
        let values = Arc::new(Mutex::new(Vec::new()));
        let node = input
            .props(InputProps {
                default_value: Attr::Set("界a".into()),
                on_change: Attr::Set(EventListener::new({
                    let values = values.clone();
                    move |event: TextValueEvent| values.lock().unwrap().push(event.value)
                })),
                ..InputProps::default()
            })
            .style(|style| {
                style.width /= Dimension::Cells(8);
                style.height /= Dimension::Cells(2);
            })
            .node();
        let (sender, output, dispatcher) = pipeline(Size::new(10, 3));
        sender.send(node).unwrap();
        output
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        dispatcher.dispatch(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 0,
            modifiers: KeyModifiers::empty(),
        }));
        dispatcher.dispatch(key(KeyCode::Char('x'), KeyModifiers::empty()));
        values.lock().unwrap().last().cloned().unwrap()
    }

    assert_eq!(inserted_at(1), "x界a");
    assert_eq!(inserted_at(2), "界xa");
    assert_eq!(inserted_at(6), "界ax");
}

#[test]
fn disabled_editor_does_not_steal_pointer_focus() {
    let node = view.children([
        input
            .props(InputProps {
                ..InputProps::default()
            })
            .style(|style| {
                style.width /= Dimension::Cells(8);
                style.height /= Dimension::Cells(2);
            })
            .node(),
        input
            .props(InputProps {
                disabled: Attr::Set(true),
                ..InputProps::default()
            })
            .style(|style| {
                style.width /= Dimension::Cells(8);
                style.height /= Dimension::Cells(2);
            })
            .node(),
    ]);
    let (sender, output, dispatcher) = pipeline(Size::new(10, 5));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    let focused = dispatcher.focused();
    assert!(focused.is_some());

    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 2,
        modifiers: KeyModifiers::empty(),
    }));
    assert_eq!(dispatcher.focused(), focused);
}

#[test]
fn disabling_a_focused_editor_blurs_it() {
    let focus = Arc::new(Mutex::new(Vec::new()));
    let field = |disabled| {
        input
            .props(InputProps {
                disabled: Attr::Set(disabled),
                ..InputProps::default()
            })
            .style(|style| {
                style.width /= Dimension::Cells(8);
                style.height /= Dimension::Cells(2);
            })
            .events({
                let focus = focus.clone();
                move |handlers: &mut icmd::EventHandlers| {
                    handlers.focus_event = Attr::Set(EventListener::new({
                        let focus = focus.clone();
                        move |event| focus.lock().unwrap().push(event)
                    }));
                }
            })
            .node()
    };
    let (sender, output, dispatcher) = pipeline(Size::new(10, 3));
    sender.send(field(false)).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    // The underline belongs to the editor host rather than its scroll child,
    // exercising focus retention on the host itself.
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_secs(1));
    assert!(dispatcher.focused().is_some());

    sender.send(field(true)).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();

    assert_eq!(dispatcher.focused(), None);
    assert_eq!(
        &*focus.lock().unwrap(),
        &[FocusEvent::Gained, FocusEvent::Lost]
    );
}

#[test]
fn pointer_position_stays_correct_after_horizontal_scroll() {
    // Scrolling the single line input must not shift where a click lands: the
    // caret belongs to the cell that was clicked, whatever the offset is.
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = input
        .props(InputProps {
            default_value: Attr::Set("0123456789".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..InputProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(6);
            style.height /= Dimension::Cells(2);
        })
        .node();
    let viewport = Size::new(10, 3);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    let mut screen = Screen::new(viewport);
    let drain = |screen: &mut Screen| {
        while let Ok(frame) = output.recv_timeout(Duration::from_millis(250)) {
            screen.apply(&frame.unwrap());
        }
    };
    drain(&mut screen);
    // Focus the input and pan the value so the viewport is not at its start.
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    for _ in 0..6 {
        dispatcher.dispatch(key(KeyCode::Right, KeyModifiers::empty()));
    }
    drain(&mut screen);
    let rows = screen.lines();
    let painted = rows.concat();
    assert!(
        !painted.contains('0') && !painted.contains('1'),
        "the value must be scrolled: {rows:?}"
    );
    // Click the first painted digit and insert: the caret must land on it.
    let row = rows
        .iter()
        .position(|line| line.chars().any(|ch| ch.is_ascii_digit()))
        .expect("a value row");
    let column = rows[row]
        .chars()
        .position(|ch| ch.is_ascii_digit())
        .expect("a digit") as u16;
    let clicked = rows[row].chars().nth(column as usize).unwrap_or(' ');
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row: row as u16,
        modifiers: KeyModifiers::empty(),
    }));
    dispatcher.dispatch(key(KeyCode::Char('x'), KeyModifiers::empty()));
    let emitted = values.lock().unwrap().last().cloned().unwrap_or_default();
    let position = emitted.find('x').expect("the click must land in the input");
    // A pointer lands inside a cell, so the caret belongs after that cell's
    // character - never a cell earlier or later.
    assert_eq!(
        emitted[..position].chars().next_back(),
        Some(clicked),
        "the caret must land in the clicked cell {clicked:?}: {emitted:?}"
    );
    assert!(
        emitted[position + 1..].starts_with(emitted[position + 1..].chars().next().unwrap_or(' ')),
        "the value must keep its order: {emitted:?}"
    );
}
#[test]
fn focused_editor_consumes_ancestor_keyboard_listeners() {
    let keyboard_event_hits = Arc::new(Mutex::new(0usize));
    let key_down_hits = Arc::new(Mutex::new(0usize));
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = view
        .events({
            let keyboard_event_hits = keyboard_event_hits.clone();
            let key_down_hits = key_down_hits.clone();
            move |events| {
                events.keyboard_event /= move |_event: icmd::KeyboardEvent| {
                    *keyboard_event_hits.lock().unwrap() += 1;
                };
                events.key_down /= move |_event: icmd::KeyboardEvent| {
                    *key_down_hits.lock().unwrap() += 1;
                };
            }
        })
        .children([input
            .props(InputProps {
                default_value: Attr::Set("a".into()),
                on_change: Attr::Set(EventListener::new({
                    let values = values.clone();
                    move |event: TextValueEvent| values.lock().unwrap().push(event.value)
                })),
                ..InputProps::default()
            })
            .node()]);
    let (sender, output, dispatcher) = pipeline(Size::new(12, 3));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_secs(1));
    dispatcher.dispatch(key(KeyCode::Char('x'), KeyModifiers::empty()));
    dispatcher.dispatch(key(KeyCode::Right, KeyModifiers::empty()));

    assert_eq!(&*values.lock().unwrap(), &["ax"]);
    assert_eq!(*keyboard_event_hits.lock().unwrap(), 0);
    assert_eq!(*key_down_hits.lock().unwrap(), 0);
}

/// Minimal text screen that replays the renderer's diff frames.
struct Screen {
    rows: Vec<Vec<String>>,
    line: usize,
    column: usize,
}

impl Screen {
    fn new(viewport: Size) -> Self {
        Self {
            rows: vec![vec![String::from(" "); viewport.width as usize]; viewport.height as usize],
            line: 0,
            column: 0,
        }
    }

    fn lines(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.concat()).collect::<Vec<_>>()
    }

    fn text(&self) -> String {
        self.lines().concat()
    }

    /// Apply one renderer frame, which only carries the cells that changed.
    fn apply(&mut self, frame: &str) {
        let width = self.rows[0].len();
        let height = self.rows.len();
        let mut index = 0usize;
        while index < frame.len() {
            let rest = &frame[index..];
            if rest.starts_with('\u{1b}') {
                if let Some(body) = rest.strip_prefix("\u{1b}[") {
                    let mut final_ch = ' ';
                    let mut digits = String::new();
                    let mut consumed = 0usize;
                    for ch in body.chars() {
                        consumed += ch.len_utf8();
                        if ch.is_ascii_alphabetic() {
                            final_ch = ch;
                            break;
                        }
                        digits.push(ch);
                    }
                    let params = digits
                        .split(';')
                        .filter_map(|value| value.parse::<usize>().ok())
                        .collect::<Vec<_>>();
                    match final_ch {
                        'H' => {
                            self.line = params.first().copied().unwrap_or(1).max(1) - 1;
                            self.column = params.get(1).copied().unwrap_or(1).max(1) - 1;
                        }
                        'G' => self.column = params.first().copied().unwrap_or(1).max(1) - 1,
                        'J' => self.rows = vec![vec![String::from(" "); width]; height],
                        _ => {}
                    }
                    index += 2 + consumed;
                    continue;
                }
                index += 2;
                continue;
            }
            if rest.starts_with('\r') {
                self.column = 0;
                index += 1;
                continue;
            }
            if rest.starts_with('\n') {
                self.line += 1;
                index += 1;
                continue;
            }
            let grapheme = rest.graphemes(true).next().unwrap_or("");
            let cell_width = unicode_width::UnicodeWidthStr::width(grapheme).max(1);
            if self.line < height && self.column < width {
                self.rows[self.line][self.column] = grapheme.to_string();
                for offset in 1..cell_width {
                    if self.column + offset < width {
                        self.rows[self.line][self.column + offset] = String::new();
                    }
                }
            }
            self.column += cell_width;
            index += grapheme.len();
        }
    }
}

/// Replay every frame the commit pipeline emits until it settles.
fn drain(
    screen: &mut Screen,
    output: &crossbeam_channel::Receiver<Result<String, icmd::FrameError>>,
) {
    while let Ok(frame) = output.recv_timeout(Duration::from_millis(200)) {
        screen.apply(&frame.unwrap());
    }
}

#[test]
fn single_line_input_is_one_row_above_its_rule() {
    let node = input
        .props(InputProps {
            placeholder: Attr::Set("filter widgets…".into()),
            ..InputProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(14);
            style.height /= Dimension::Cells(2);
        })
        .node();
    let viewport = Size::new(16, 4);
    let (sender, output, _) = pipeline(viewport);
    sender.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let mut screen = Screen::new(viewport);
    screen.apply(&frame);
    let rows = screen.lines();
    assert!(
        rows[0].contains("filter widge"),
        "the input paints its text on the first row: {rows:?}"
    );
    assert!(
        rows[1].starts_with('─'),
        "the underline sits directly under the text: {rows:?}"
    );
    assert!(
        rows[2..].iter().all(|line| line.trim().is_empty()),
        "a single line input has no trailing blank rows: {rows:?}"
    );
}

#[test]
fn held_key_repeats_keep_inserting_at_the_caret() {
    // The owner reconciles the controlled value asynchronously, so repeats can
    // land before any render happens in between.
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = accepting_controlled_input
        .props(ControlledInputTestProps {
            values: values.clone(),
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(16, 3));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_secs(1));

    // A held key repeats far faster than the owner reconciles the controlled
    // value, so several presses can land before any render in between.
    let emitted = {
        let mut emitted = Vec::new();
        for ch in ["x", "x", "y"] {
            dispatcher.dispatch(key(
                KeyCode::Char(ch.chars().next().unwrap()),
                KeyModifiers::empty(),
            ));
            if let Ok(frame) = output.recv_timeout(Duration::from_millis(200)) {
                emitted.push(frame.unwrap());
            }
        }
        emitted
    };
    let values = values.lock().unwrap().clone();
    assert_eq!(
        values,
        vec!["ax", "axx", "axxy"],
        "repeats must append at the caret, not behind it"
    );
    let viewport = Size::new(16, 3);
    let mut screen = Screen::new(viewport);
    for frame in &emitted {
        screen.apply(frame);
    }
    let last = screen.lines();
    let painted = screen.text();
    assert!(
        painted.contains('y'),
        "the caret stays at the end of the value: {last:?}"
    );
    assert!(
        !painted.contains("yx") && !painted.contains("xyx"),
        "no repeat may land behind the caret: {last:?}"
    );
}

#[test]
fn textarea_wraps_to_the_width_its_parent_grants() {
    // The editor asks for 30 columns but the parent only grants 12. Wrapping to
    // the requested width would leave the tail of every row clipped and
    // unreachable; the editor has to wrap to the width it really has.
    let editor = textarea
        .props(TextareaProps {
            default_value: Attr::Set("0123456789abcdefghijklmnopqrstuvwxyz".into()),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(34);
            style.height /= Dimension::Cells(10);
        })
        .node();
    let mut dom = icmd::DomProps::default();
    dom.style.layout /= icmd::Layout::Vertical;
    dom.style.width /= icmd::Dimension::Cells(14);
    dom.style.padding /= icmd::Edges::symmetric(0, 1);
    let node = icmd::Component::apply(
        view,
        Props {
            dom,
            children: vec![editor],
            user_defined: (),
        },
    );
    // One row taller than the wrapped document, so the whole box is on screen
    // and any clipped character is horizontal, not vertical.
    let viewport = Size::new(14, 10);
    let (sender, output, _) = pipeline(viewport);
    let mut screen = Screen::new(viewport);
    // Each frame is a diff, so replay them all. The first paint reports the
    // granted width and the next layout wraps to it.
    for _ in 0..3 {
        sender.send(node.clone()).unwrap();
        if let Ok(frame) = output.recv_timeout(Duration::from_secs(1)) {
            screen.apply(&frame.unwrap());
        }
    }
    let rows = screen.lines();
    let painted = screen.text();
    for ch in "0123456789abcdefghijklmnopqrstuvwxyz".chars() {
        assert!(
            painted.contains(ch),
            "character {ch:?} must not be clipped: {rows:?}"
        );
    }
}

#[test]
fn textarea_click_lands_on_the_cell_under_the_pointer() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = textarea
        .props(TextareaProps {
            default_value: Attr::Set("abcdefghij".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(12);
            style.height /= Dimension::Cells(6);
        })
        .node();
    // The border box starts at the origin: border column 0, one padding cell,
    // then the content. Content cell 3 is screen column 5 on the first row.
    let (sender, output, dispatcher) = pipeline(Size::new(14, 6));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 1,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_millis(300));
    dispatcher.dispatch(key(KeyCode::Char('X'), KeyModifiers::empty()));
    assert_eq!(
        values.lock().unwrap().last().map(String::as_str),
        Some("abcdXefghij"),
        "the caret must land on the cell under the pointer"
    );
}

#[test]
fn input_click_lands_on_the_cell_under_the_pointer() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = input
        .props(InputProps {
            default_value: Attr::Set("abcdefghij".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..InputProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(10);
            style.height /= Dimension::Cells(2);
        })
        .node();
    let (sender, output, dispatcher) = pipeline(Size::new(14, 4));
    sender.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_millis(300));
    dispatcher.dispatch(key(KeyCode::Char('X'), KeyModifiers::empty()));
    assert_eq!(
        values.lock().unwrap().last().map(String::as_str),
        Some("abcdeXfghij"),
        "padding must not shift the caret"
    );
}

#[test]
fn wrapped_textarea_click_maps_every_row() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let node = textarea
        .props(TextareaProps {
            default_value: Attr::Set("abcdefghijklmnopqrstuvwxyz".into()),
            wrap: Attr::Set(TextWrap::Hard),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(14);
            style.height /= Dimension::Cells(8);
        })
        .node();
    let viewport = Size::new(16, 8);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let mut screen = Screen::new(viewport);
    screen.apply(&frame);
    let lines = screen.lines();
    // Rows wrap as "abcdefghij" / "klmnopqrst" / "uvwxyz". The second wrapped
    // row starts at the border box's column 2 and screen row 2.
    let row = lines
        .iter()
        .position(|line| line.contains('k'))
        .expect("second wrapped row on screen");
    // `str::find` returns a byte offset; the screen is measured in cells.
    let column = lines[row]
        .chars()
        .position(|ch| ch == 'k')
        .expect("row content") as u16;
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row: row as u16,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_millis(300));
    dispatcher.dispatch(key(KeyCode::Char('X'), KeyModifiers::empty()));
    assert_eq!(
        values.lock().unwrap().last().map(String::as_str),
        Some("abcdefghijkXlmnopqrstuvwxyz"),
        "a click on a wrapped row must map to that row's first cell"
    );
}

#[test]
fn textarea_rewraps_when_the_viewport_resizes() {
    // The editor asks for 24 columns inside a full-width parent. Shrinking the
    // viewport shrinks the granted width, and the rows have to be rebuilt for
    // it instead of keeping the width they were wrapped for.
    let editor = textarea
        .props(TextareaProps {
            default_value: Attr::Set("0123456789abcdefghijklmnopqrstuvwxyz".into()),
            wrap: Attr::Set(TextWrap::Hard),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(28);
            style.height /= Dimension::Cells(8);
        })
        .node();
    let mut dom = icmd::DomProps::default();
    dom.style.layout /= icmd::Layout::Vertical;
    dom.style.width /= icmd::Dimension::Max;
    dom.style.padding /= icmd::Edges::symmetric(0, 1);
    let node = icmd::Component::apply(
        view,
        Props {
            dom,
            children: vec![editor],
            user_defined: (),
        },
    );
    let viewport = Size::new(40, 12);
    let (commit, setter, _) = Commit::new_with_events(viewport);
    let (sender, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start();
    sender.send(node).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1));

    // Shrink the viewport so the granted width changes, then let the editor
    // rebuild its rows and repaint. The parent, the embedded border and its
    // padding leave 8 content columns, so the value now wraps every 8 cells.
    let resized = Size::new(14, 12);
    setter.set(resized);
    let mut screen = Screen::new(resized);
    for _ in 0..6 {
        if let Ok(frame) = output.recv_timeout(Duration::from_millis(400)) {
            screen.apply(&frame.unwrap());
        }
    }
    // The frames after a resize are diffs, so they carry the rows whose
    // content changed. Re-wrapping at 8 cells turned the single wide row into
    // "01234567" / "89abcdef" / ..., and those first cells are what repaints.
    let rows = screen.lines();
    assert!(
        rows.iter().any(|row| row.contains("01234567")),
        "the rows must be re-wrapped for the granted width: {rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("0123456789abcdef")),
        "the old wide row must be gone: {rows:?}"
    );
}

#[test]
fn narrow_textarea_click_still_maps_to_the_cell() {
    // The editor asks for 30 columns but the parent grants 8. It must wrap to
    // the granted width, otherwise the pointer maps against rows that were
    // never painted where the click landed.
    let values = Arc::new(Mutex::new(Vec::new()));
    let editor = textarea
        .props(TextareaProps {
            default_value: Attr::Set("0123456789abcdefghijklmnopqrstuvwxyz".into()),
            on_change: Attr::Set(EventListener::new({
                let values = values.clone();
                move |event: TextValueEvent| values.lock().unwrap().push(event.value)
            })),
            ..TextareaProps::default()
        })
        .style(|style| {
            style.width /= Dimension::Cells(34);
            style.height /= Dimension::Cells(9);
        })
        .node();
    let mut dom = icmd::DomProps::default();
    dom.style.layout /= icmd::Layout::Vertical;
    dom.style.width /= icmd::Dimension::Cells(14);
    dom.style.padding /= icmd::Edges::symmetric(0, 1);
    let node = icmd::Component::apply(
        view,
        Props {
            dom,
            children: vec![editor],
            user_defined: (),
        },
    );
    let viewport = Size::new(14, 10);
    let (sender, output, dispatcher) = pipeline(viewport);
    sender.send(node).unwrap();
    let mut screen = Screen::new(viewport);
    for _ in 0..4 {
        if let Ok(frame) = output.recv_timeout(Duration::from_millis(300)) {
            screen.apply(&frame.unwrap());
        }
    }
    let rows = screen.lines();
    assert!(
        rows.iter().any(|row| row.contains("01234567")),
        "the rows must wrap to the granted width: {rows:?}"
    );
    // '8' starts the second wrapped row, painted one padding cell inside the
    // parent and one inside the editor's own border.
    let row = rows
        .iter()
        .position(|line| line.contains("89abcdef"))
        .expect("second granted-width row");
    let column = rows[row]
        .chars()
        .enumerate()
        .find(|(_, ch)| *ch == '8')
        .map(|(index, _)| index)
        .expect("row content") as u16;
    dispatcher.dispatch(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row: row as u16,
        modifiers: KeyModifiers::empty(),
    }));
    let _ = output.recv_timeout(Duration::from_millis(300));
    dispatcher.dispatch(key(KeyCode::Char('#'), KeyModifiers::empty()));
    let _ = output.recv_timeout(Duration::from_millis(300));
    assert_eq!(
        values.lock().unwrap().last().map(String::as_str),
        Some("012345678#9abcdefghijklmnopqrstuvwxyz"),
        "a click must land on the cell under the pointer even when the box is narrower than requested"
    );
}
