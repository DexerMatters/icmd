use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use icmd::advanced::{Commit, Lower, Runtime};
use icmd::{Dimension, ElementRef, ElementSnapshot, Size, Style, Visibility, ui, view};

fn commit_one(node: icmd::Node, viewport: Size) -> icmd::Frame {
    let (commit, _, _) = Commit::new_with_events(viewport);
    let (input, output) = Runtime::new(Lower::default()).then(commit).start();
    input.send(node).expect("lower stage accepts node");
    output
        .recv_timeout(Duration::from_secs(2))
        .expect("commit stage emits a frame")
}

#[test]
fn element_ref_publishes_committed_geometry() {
    let element_ref = ElementRef::new();
    let node = ui! {
        <view element_ref={element_ref.clone()} style={|style: &mut Style| {
            style.width /= Dimension::Cells(4);
            style.height /= Dimension::Cells(2);
        }} />
    };
    let _ = commit_one(node, Size::new(20, 10));

    let snapshot = element_ref.current().expect("ref is committed");
    assert_eq!(snapshot.bounding_rect().width, 4);
    assert_eq!(snapshot.bounding_rect().height, 2);
    assert_eq!(snapshot.content_rect().width, 4);
    assert_eq!(snapshot.content_rect().height, 2);
    assert!(snapshot.visible_rect().is_some());
}

#[test]
fn element_ref_can_be_read_from_its_change_callback() {
    let element_ref = ElementRef::new();
    let callback_ref = element_ref.clone();
    let node = ui! {
        <view
            element_ref={element_ref.clone()}
            on_element_change={move |snapshot: Option<ElementSnapshot>| {
                assert!(snapshot.is_some());
                assert!(callback_ref.current().is_some());
            }}
        />
    };
    let _ = commit_one(node, Size::new(20, 10));
}

#[test]
fn element_change_notifies_on_mount_and_unmount() {
    let (commit, _, _) = Commit::new_with_events(Size::new(20, 10));
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();
    let events: Arc<Mutex<Vec<Option<ElementSnapshot>>>> = Arc::new(Mutex::new(Vec::new()));
    let events_for_listener = events.clone();
    let node = ui! {
        <view on_element_change={move |snapshot: Option<ElementSnapshot>| {
            events_for_listener.lock().unwrap().push(snapshot);
        }} />
    };
    input.send(node).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
    assert!(events.lock().unwrap()[0].is_some());

    let events_for_hidden = events.clone();
    let hidden = ui! {
        <view style={|style: &mut Style| style.visibility /= Visibility::Hidden}
            on_element_change={move |snapshot: Option<ElementSnapshot>| {
                events_for_hidden.lock().unwrap().push(snapshot);
            }} />
    };
    input.send(hidden).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(events.lock().unwrap().iter().any(Option::is_none));
    drop(input);
    runtime
        .shutdown(icmd::advanced::ShutdownPolicy::default())
        .unwrap();
}

#[test]
fn element_change_failure_is_reported_by_commit() {
    let (commit, _, _) = Commit::new_with_events(Size::new(20, 10));
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let errors = runtime.errors();
    input
        .send(ui! {
            <view on_element_change={move |_snapshot: Option<ElementSnapshot>| {
                panic!("measurement callback failed");
            }} />
        })
        .unwrap();
    let error = errors
        .recv_timeout(Duration::from_secs(2))
        .expect("commit reports callback failure");
    assert!(matches!(
        error,
        icmd::advanced::RuntimeError::ApplicationCallback(_)
    ));
    drop(input);
    runtime
        .shutdown(icmd::advanced::ShutdownPolicy::default())
        .unwrap();
}

#[test]
fn clipped_elements_keep_geometry_but_have_no_visible_rect() {
    let element_ref = ElementRef::new();
    let node = ui! {
        <view style={|style: &mut Style| {
            style.layout /= icmd::Layout::Absolute;
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
        }}>
            <view element_ref={element_ref.clone()} style={|style: &mut Style| {
                style.line /= icmd::AxisPosition::Cells(8);
                style.column /= icmd::AxisPosition::Cells(2);
                style.width /= Dimension::Cells(3);
                style.height /= Dimension::Cells(2);
            }} />
        </view>
    };
    let _ = commit_one(node, Size::new(20, 5));
    let snapshot = element_ref
        .current()
        .expect("offscreen host remains measurable");
    assert_eq!(snapshot.bounding_rect().line, 8);
    assert_eq!(snapshot.bounding_rect().column, 2);
    assert_eq!(snapshot.bounding_rect().width, 3);
    assert_eq!(snapshot.bounding_rect().height, 2);
    assert!(snapshot.visible_rect().is_none());
    assert!(!snapshot.is_visible());
}
