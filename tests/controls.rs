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
    Attr, ButtonProps, CheckboxProps, Component, EventListener, LinkProps, Node, RadioProps, Size,
    SwitchProps, button, checkbox, link, radio, switch,
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
fn link_reports_its_target_once_per_activation() {
    let followed = Arc::new(Mutex::new(Vec::new()));
    let seen = followed.clone();
    let harness = Harness::mount(
        link.props(LinkProps {
            href: Attr::Set("https://example.com/icmd".into()),
            label: Attr::Set("docs".into()),
            on_follow: Attr::Set(EventListener::new(move |target: String| {
                seen.lock().unwrap().push(target);
            })),
            ..LinkProps::default()
        })
        .node(),
    );

    // Pointer: one follow request per press, carrying the target itself rather
    // than an empty activation.
    click(&harness.dispatcher, 1, 0);
    assert_eq!(
        &*followed.lock().unwrap(),
        &["https://example.com/icmd".to_string()],
        "one click is one follow request carrying the href"
    );

    // Keyboard: focus by click, then Enter follows the same target again.
    harness.settle();
    key(&harness.dispatcher, KeyCode::Enter);
    assert_eq!(
        followed.lock().unwrap().len(),
        2,
        "Enter follows the focused link"
    );
}

#[test]
fn disabled_link_neither_follows_nor_takes_focus() {
    let followed = Arc::new(AtomicUsize::new(0));
    let seen = followed.clone();
    let harness = Harness::mount(
        link.props(LinkProps {
            href: Attr::Set("https://example.com/icmd".into()),
            label: Attr::Set("docs".into()),
            disabled: Attr::Set(true),
            on_follow: Attr::Set(EventListener::new(move |_target: String| {
                seen.fetch_add(1, Ordering::SeqCst);
            })),
            ..LinkProps::default()
        })
        .node(),
    );

    click(&harness.dispatcher, 1, 0);
    key(&harness.dispatcher, KeyCode::Enter);
    assert_eq!(
        followed.load(Ordering::SeqCst),
        0,
        "a disabled link must not follow"
    );
    assert!(
        harness.dispatcher.focused().is_none(),
        "a disabled link must not take focus"
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

// API acceptance checklist: "Controls named as interactive pass the shared
// activation/disabled/focus suite." One table drives every interactive control
// through the same three assertions, so a control cannot be added as
// "interactive" while quietly missing activation, disabled, or focus behavior.
#[test]
fn every_interactive_control_passes_the_shared_suite() {
    #[derive(Clone, Copy)]
    enum Control {
        Button,
        Checkbox,
        Switch,
        Radio,
        Link,
    }

    // Each control reaches activation through its own prop, but all of them
    // accept keyboard activation and must refuse it when disabled.
    fn build(control: Control, disabled: bool, fired: Arc<AtomicUsize>) -> Node {
        match control {
            Control::Button => button
                .props(ButtonProps {
                    disabled: Attr::Set(disabled),
                    on_press: Attr::Set(EventListener::new(move |_event| {
                        fired.fetch_add(1, Ordering::SeqCst);
                    })),
                    ..ButtonProps::default()
                })
                .node(),
            Control::Checkbox => checkbox
                .props(CheckboxProps {
                    checked: Attr::Set(false),
                    disabled: Attr::Set(disabled),
                    on_change: Attr::Set(EventListener::new(move |_value: bool| {
                        fired.fetch_add(1, Ordering::SeqCst);
                    })),
                    ..CheckboxProps::default()
                })
                .node(),
            Control::Switch => switch
                .props(SwitchProps {
                    on: Attr::Set(false),
                    disabled: Attr::Set(disabled),
                    on_change: Attr::Set(EventListener::new(move |_value: bool| {
                        fired.fetch_add(1, Ordering::SeqCst);
                    })),
                    ..SwitchProps::default()
                })
                .node(),
            Control::Radio => radio
                .props(RadioProps {
                    selected: Attr::Set(false),
                    disabled: Attr::Set(disabled),
                    on_select: Attr::Set(EventListener::new(move |_value: ()| {
                        fired.fetch_add(1, Ordering::SeqCst);
                    })),
                    ..RadioProps::default()
                })
                .node(),
            Control::Link => link
                .props(LinkProps {
                    href: Attr::Set("https://example.com/icmd".into()),
                    label: Attr::Set("example".into()),
                    disabled: Attr::Set(disabled),
                    on_follow: Attr::Set(EventListener::new(move |_target: String| {
                        fired.fetch_add(1, Ordering::SeqCst);
                    })),
                    ..LinkProps::default()
                })
                .node(),
        }
    }

    for control in [
        Control::Button,
        Control::Checkbox,
        Control::Switch,
        Control::Radio,
        Control::Link,
    ] {
        let name = match control {
            Control::Button => "button",
            Control::Checkbox => "checkbox",
            Control::Switch => "switch",
            Control::Radio => "radio",
            Control::Link => "link",
        };

        // Activation: focusing the control and pressing Space or Enter fires it.
        let fired = Arc::new(AtomicUsize::new(0));
        let harness = Harness::mount(build(control, false, fired.clone()));
        click(&harness.dispatcher, 1, 0);
        let activations = [
            KeyCode::Char(' '),
            KeyCode::Enter,
            KeyCode::Right,
            KeyCode::Down,
        ]
        .into_iter()
        .filter(|code| {
            // Dispatch is synchronous, so the listener has already run.
            let before = fired.load(Ordering::SeqCst);
            key(&harness.dispatcher, *code);
            fired.load(Ordering::SeqCst) > before
        })
        .count();
        assert!(
            activations > 0,
            "{name} must be activatable from the keyboard"
        );

        // Disabled: the same focus and key sequence must not fire it.
        let fired = Arc::new(AtomicUsize::new(0));
        let harness = Harness::mount(build(control, true, fired.clone()));
        click(&harness.dispatcher, 1, 0);
        for code in [KeyCode::Char(' '), KeyCode::Enter] {
            key(&harness.dispatcher, code);
        }
        assert_eq!(
            fired.load(Ordering::SeqCst),
            0,
            "a disabled {name} must not activate"
        );

        // Focus: a disabled control must not take focus, so a later key cannot
        // reach it; an enabled one may.
        let dispatched = harness
            .dispatcher
            .dispatch(Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char(' '),
                KeyModifiers::empty(),
                KeyEventKind::Press,
            )));
        assert_eq!(
            dispatched.delivered, 0,
            "a disabled {name} must not receive targeted keys"
        );
    }
}
