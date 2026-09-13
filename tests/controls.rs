// API-05: controls named as interactive must own their interaction semantics.
// One parameterized suite covers pointer, keyboard, disabled, focus, duplicate
// activation, and controlled-state changes for every control.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use icmd::advanced::{Commit, Lower, Renderer, Runtime};
use icmd::{
    Attr, ButtonProps, CheckboxProps, Component, EventListener, Node, RadioProps, Size,
    SwitchProps, button, checkbox, radio, switch,
};

fn pipeline(
    viewport: Size,
) -> (
    crossbeam_channel::Sender<Node>,
    crossbeam_channel::Receiver<Result<String, icmd::advanced::FrameError>>,
    icmd::advanced::EventDispatcher,
) {
    let (commit, _, dispatcher) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start_handle();
    (runtime.input(), runtime.output(), dispatcher)
}

fn click(dispatcher: &icmd::advanced::EventDispatcher, column: u16, row: u16) {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        dispatcher.dispatch(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        }));
    }
}

fn key(dispatcher: &icmd::advanced::EventDispatcher, code: KeyCode) {
    dispatcher.dispatch(Event::Key(KeyEvent::new_with_kind(
        code,
        KeyModifiers::empty(),
        KeyEventKind::Press,
    )));
}

struct Harness {
    // Held so the pipeline stays open for the lifetime of the test.
    _input: crossbeam_channel::Sender<Node>,
    output: crossbeam_channel::Receiver<Result<String, icmd::advanced::FrameError>>,
    dispatcher: icmd::advanced::EventDispatcher,
}

impl Harness {
    fn mount(node: Node) -> Self {
        let (input, output, dispatcher) = pipeline(Size::new(24, 6));
        input.send(node).unwrap();
        output
            .recv_timeout(Duration::from_secs(2))
            .expect("initial frame")
            .expect("render");
        Self {
            _input: input,
            output,
            dispatcher,
        }
    }

    fn settle(&self) {
        let _ = self.output.recv_timeout(Duration::from_millis(150));
    }
}

#[test]
fn button_activates_once_per_press_from_pointer_and_keyboard() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_for_listener = hits.clone();
    let harness = Harness::mount(
        button
            .props(ButtonProps {
                on_press: Attr::Set(EventListener::new(move |_| {
                    hits_for_listener.fetch_add(1, Ordering::SeqCst);
                })),
                ..ButtonProps::default()
            })
            .node(),
    );

    // Pointer: one semantic activation per press, not one per event.
    click(&harness.dispatcher, 1, 0);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "one click is one activation"
    );

    // Keyboard: focus by click, then Space and Enter each activate once.
    harness.settle();
    key(&harness.dispatcher, KeyCode::Char(' '));
    assert_eq!(hits.load(Ordering::SeqCst), 2, "Space activates");
    key(&harness.dispatcher, KeyCode::Enter);
    assert_eq!(hits.load(Ordering::SeqCst), 3, "Enter activates");
}

#[test]
fn disabled_button_suppresses_activation_and_focus() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_for_listener = hits.clone();
    let harness = Harness::mount(
        button
            .props(ButtonProps {
                disabled: Attr::Set(true),
                on_press: Attr::Set(EventListener::new(move |_| {
                    hits_for_listener.fetch_add(1, Ordering::SeqCst);
                })),
                ..ButtonProps::default()
            })
            .node(),
    );

    click(&harness.dispatcher, 1, 0);
    key(&harness.dispatcher, KeyCode::Enter);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "a disabled control must not activate"
    );
    assert!(
        harness.dispatcher.focused().is_none(),
        "a disabled control must not take focus"
    );
}

#[test]
fn checkbox_and_switch_request_the_proposed_state() {
    let checkbox_changes = Arc::new(Mutex::new(Vec::new()));
    let changes = checkbox_changes.clone();
    let harness = Harness::mount(
        checkbox
            .props(CheckboxProps {
                checked: Attr::Set(false),
                label: Attr::Set("agree".into()),
                on_change: Attr::Set(EventListener::new(move |next: bool| {
                    changes.lock().unwrap().push(next);
                })),
                ..CheckboxProps::default()
            })
            .node(),
    );
    click(&harness.dispatcher, 1, 0);
    assert_eq!(
        &*checkbox_changes.lock().unwrap(),
        &[true],
        "a controlled checkbox requests the toggled state"
    );

    let switch_changes = Arc::new(Mutex::new(Vec::new()));
    let changes = switch_changes.clone();
    let harness = Harness::mount(
        switch
            .props(SwitchProps {
                on: Attr::Set(true),
                label: Attr::Set("power".into()),
                on_change: Attr::Set(EventListener::new(move |next: bool| {
                    changes.lock().unwrap().push(next);
                })),
                ..SwitchProps::default()
            })
            .node(),
    );
    click(&harness.dispatcher, 1, 0);
    assert_eq!(
        &*switch_changes.lock().unwrap(),
        &[false],
        "a controlled switch requests the toggled state"
    );
}

#[test]
fn radio_selection_reports_through_the_group_owner() {
    // Two radios in a group. Selecting one reports through that radio's
    // callback; the component never mutates its own selection, so exclusive
    // policy stays with the owner.
    let first_hits = Arc::new(AtomicUsize::new(0));
    let second_hits = Arc::new(AtomicUsize::new(0));

    let first = {
        let hits = first_hits.clone();
        radio
            .props(RadioProps {
                selected: Attr::Set(true),
                label: Attr::Set("one".into()),
                on_select: Attr::Set(EventListener::new(move |_| {
                    hits.fetch_add(1, Ordering::SeqCst);
                })),
                ..RadioProps::default()
            })
            .node()
    };
    let second = {
        let hits = second_hits.clone();
        radio
            .props(RadioProps {
                selected: Attr::Set(false),
                label: Attr::Set("two".into()),
                on_select: Attr::Set(EventListener::new(move |_| {
                    hits.fetch_add(1, Ordering::SeqCst);
                })),
                ..RadioProps::default()
            })
            .node()
    };

    let harness = Harness::mount(icmd::fragment([first, second]));
    // The second radio is rendered on the following row.
    click(&harness.dispatcher, 1, 1);
    assert_eq!(
        second_hits.load(Ordering::SeqCst),
        1,
        "second radio selected"
    );
    assert_eq!(
        first_hits.load(Ordering::SeqCst),
        0,
        "only the activated radio reports"
    );
}

#[test]
fn disabled_selection_controls_do_not_change() {
    let changes = Arc::new(Mutex::new(Vec::new()));
    let changes_for_listener = changes.clone();
    let harness = Harness::mount(
        checkbox
            .props(CheckboxProps {
                checked: Attr::Set(false),
                disabled: Attr::Set(true),
                on_change: Attr::Set(EventListener::new(move |next: bool| {
                    changes_for_listener.lock().unwrap().push(next);
                })),
                ..CheckboxProps::default()
            })
            .node(),
    );
    click(&harness.dispatcher, 1, 0);
    key(&harness.dispatcher, KeyCode::Char(' '));
    assert!(
        changes.lock().unwrap().is_empty(),
        "a disabled checkbox must not request a change"
    );
}

#[test]
fn autofocus_is_an_explicit_request_not_an_inference() {
    let harness = Harness::mount(
        button
            .props(ButtonProps {
                autofocus: Attr::Set(true),
                ..ButtonProps::default()
            })
            .node(),
    );
    assert!(
        harness.dispatcher.focused().is_some(),
        "autofocus must publish a focus request the runtime grants"
    );
}
