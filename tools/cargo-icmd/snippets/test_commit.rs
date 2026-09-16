//! Committing a component tree at a fixed viewport.

use icmd::advanced::{Commit, Lower, Runtime, ShutdownPolicy};
use icmd::{Component, ComponentContext, Node, Props, Size, heading, ui};
use std::time::Duration;

/// Render tests fix the viewport so geometry assertions are reproducible.
#[test]
fn app_commits_a_non_empty_frame() {
    let viewport = Size::new(80, 24);
    let (commit, _, _) = Commit::new_with_events(viewport);
    let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
    let input = runtime.input();
    let output = runtime.output();
    input
        .send(app.apply(()))
        .expect("the tree enters the pipeline");
    let frame = output
        .recv_timeout(Duration::from_secs(2))
        .expect("a frame commits");
    assert!(!frame.operations.is_empty());
    drop(input);
    runtime.shutdown(ShutdownPolicy::default()).unwrap();
}

/// The component under test.
pub fn app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    ui! { <heading>"tested at a fixed viewport"</heading> }
}
