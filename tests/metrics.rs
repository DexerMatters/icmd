// Runtime counter tests. The counters are process-global, so the delta helper
// is what makes them usable from an integration test.
use icmd::__private::{
    note_event_dispatched, note_frame_presented, note_node_lowered, note_output_bytes,
    note_text_shaping,
};
use icmd::advanced::runtime_metrics;

#[test]
fn deltas_are_monotonic() {
    let before = runtime_metrics();
    note_text_shaping();
    note_node_lowered();
    note_output_bytes(7);
    note_frame_presented();
    note_event_dispatched();
    let after = runtime_metrics();
    let delta = after.since(before);
    assert!(delta.text_shaping_calls >= 1);
    assert!(delta.nodes_lowered >= 1);
    assert!(delta.output_bytes >= 7);
    assert!(delta.frames_presented >= 1);
    assert!(delta.events_dispatched >= 1);
}
