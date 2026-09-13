use std::sync::{Arc, Mutex};
use std::time::Duration;

use icmd::{
    Commit, Component, ComponentContext, Lower, Node, Props, Renderer, Runtime, Size, StateSetter,
    create_context,
};

#[derive(Clone, PartialEq, Eq)]
struct Theme(&'static str);

fn frame_for(node: Node) -> (String, impl FnOnce()) {
    let (commit, _) = Commit::new(Size::new(40, 4));
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(Size::new(40, 4)).unwrap())
        .start();
    input.send(node).unwrap();
    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .expect("runtime did not produce a frame")
        .expect("renderer failed");
    (frame, || drop(input))
}

fn contains_text(frame: &str, text: &str) -> bool {
    text.chars().all(|character| frame.contains(character))
}

#[test]
fn use_context_reads_default_and_nearest_provider() {
    let context = create_context(Theme("default"));
    let consumer_context = context.clone();
    let root_context = context.clone();

    let root = (move |_cx: &mut ComponentContext, _props: &Props<()>| {
        let consumer_context = consumer_context.clone();
        let consumer = move |cx: &mut ComponentContext, _props: &Props<()>| {
            let theme = cx.use_context(|| &consumer_context);
            theme.0.into()
        };

        root_context.provider(
            Theme("outer"),
            [root_context.provider(Theme("inner"), [consumer.apply(())])],
        )
    })
    .apply(());

    let (frame, cleanup) = frame_for(root);
    assert!(contains_text(&frame, "inner"));
    cleanup();

    let context = create_context(Theme("fallback"));
    let consumer_context = context.clone();
    let root = (move |_cx: &mut ComponentContext, _props: &Props<()>| {
        let consumer_context = consumer_context.clone();
        let consumer = move |cx: &mut ComponentContext, _props: &Props<()>| {
            cx.use_context(|| &consumer_context).0.into()
        };
        consumer.apply(())
    })
    .apply(());

    let (frame, cleanup) = frame_for(root);
    assert!(contains_text(&frame, "fallback"));
    cleanup();
}

#[test]
fn provider_value_changes_are_visible_to_consumers() {
    let context = create_context(Theme("default"));
    let setter = Arc::new(Mutex::new(None::<StateSetter<Theme>>));
    let root_context = context.clone();
    let consumer_context = context.clone();
    let setter_for_root = setter.clone();

    let root = (move |cx: &mut ComponentContext, _props: &Props<()>| {
        let (theme, set_theme) = cx.use_state(|| Theme("light"));
        *setter_for_root.lock().unwrap() = Some(set_theme);

        let consumer_context = consumer_context.clone();
        let consumer = move |cx: &mut ComponentContext, _props: &Props<()>| {
            cx.use_context(|| &consumer_context).0.into()
        };
        root_context.provider(theme, [consumer.apply(())])
    })
    .apply(());

    let (commit, _) = Commit::new(Size::new(40, 4));
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(Size::new(40, 4)).unwrap())
        .start();
    input.send(root).unwrap();
    let first = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(contains_text(&first, "light"));

    setter.lock().unwrap().as_ref().unwrap().set(Theme("dark"));
    let second = output
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert!(contains_text(&second, "dark"));
}

// SAF-11 / DUP-03: the generic provider component is gone. `ContextKey::provider`
// is the canonical path and requires the key and value at the call site, so a
// missing required value cannot be expressed, let alone panic a worker.
#[test]
fn context_provider_requires_key_and_value_at_construction() {
    let context = create_context(Theme("default"));
    let node = context.provider(Theme("inner"), [icmd::text("child")]);
    let (frame, teardown) = frame_for(node);
    assert!(contains_text(&frame, "child"));
    teardown();

    // A provider value can be changed by the owner and observed by consumers,
    // which is the behavior the removed component used to wrap.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_consumer = seen.clone();
    let consumer_context = context.clone();
    let consumer = move |cx: &mut ComponentContext, _props: &Props<()>| {
        let theme = cx.use_context(|| &consumer_context);
        seen_for_consumer.lock().unwrap().push(theme.0);
        icmd::text("x")
    };
    let node = context.provider(Theme("outer"), [consumer.apply(())]);
    let (_, teardown) = frame_for(node);
    teardown();
    assert_eq!(&*seen.lock().unwrap(), &["outer"]);
}

// SAF-12: dynamic fill assignment is fallible and never panics on ordinary
// caller input.
#[test]
fn invalid_dynamic_fill_is_a_typed_error_not_a_panic() {
    let mut style = icmd::Style::default();
    assert!(
        style.fill.set_fill("").is_err(),
        "empty fill must be rejected"
    );
    assert!(
        style.fill.set_fill("ab").is_err(),
        "a multi-grapheme fill must be rejected"
    );
    assert!(
        style.fill.set_fill("\u{7}").is_err(),
        "a control fill must be rejected"
    );
    assert!(
        style.fill.set_fill("\u{200b}").is_err(),
        "a zero-width fill must be rejected"
    );
    style
        .fill
        .set_fill("█")
        .expect("a single printable grapheme is valid");
    assert_eq!(style.fill.fill().map(|fill| fill.symbol()), Some("█"));
}

#[test]
fn fill_operator_assignment_ignores_invalid_input_instead_of_panicking() {
    // The operator surface cannot report failure; it must therefore refuse
    // rather than panic.
    let mut style = icmd::Style::default();
    style.fill /= "";
    assert!(
        style.fill.fill().is_none(),
        "an invalid fill is not applied"
    );
    style.fill /= 'x';
    assert_eq!(style.fill.fill().map(|fill| fill.symbol()), Some("x"));
}

// API-08: shared state is encapsulated behind `StateRef`, and poisoning is a
// typed error rather than a panic at every call site.
#[test]
fn state_ref_read_update_and_poison_report_typed_results() {
    use icmd::StateRef;
    use std::sync::{Arc, Mutex};

    let state = StateRef::new(1u32);
    assert_eq!(state.read(|value| *value).unwrap(), 1);
    state.update(|value| *value += 1).unwrap();
    assert_eq!(state.read(|value| *value).unwrap(), 2);

    // `try_update` succeeds when the lock is free.
    assert_eq!(
        state
            .try_update(|value| {
                *value += 10;
                *value
            })
            .unwrap(),
        Some(12)
    );

    // Wrapping an existing allocation observes the same value.
    let plain: icmd::Ref<u32> = Arc::new(Mutex::new(5));
    let shared = StateRef::from_shared(plain.clone());
    assert_eq!(shared.read(|value| *value).unwrap(), 5);
    shared.update(|value| *value = 9).unwrap();
    assert_eq!(*plain.lock().unwrap(), 9);
}

#[test]
fn poisoned_state_reports_an_error_instead_of_panicking() {
    use icmd::{Ref, StateRef};
    use std::sync::{Arc, Mutex};

    // Simulate a poisoned lock: a thread panics while holding it.
    let inner: Ref<u32> = Arc::new(Mutex::new(1));
    let poison = inner.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poison.lock().unwrap();
        panic!("poison the lock");
    })
    .join();

    let state = StateRef::from_shared(inner);
    assert!(state.read(|value| *value).is_err());
    assert!(state.update(|value| *value += 1).is_err());
}
