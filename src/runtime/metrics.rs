// Bounded runtime counters required for product operation. They are process
// globals rather than per-runtime state so that a test can observe work done on
// a worker thread without owning the worker's component; every counter is a
// monotonic total, and callers compare deltas.
use std::sync::atomic::{AtomicU64, Ordering};

static TEXT_SHAPING_CALLS: AtomicU64 = AtomicU64::new(0);
static NODES_LOWERED: AtomicU64 = AtomicU64::new(0);
static OUTPUT_BYTES: AtomicU64 = AtomicU64::new(0);
static FRAMES_PRESENTED: AtomicU64 = AtomicU64::new(0);
static EVENTS_DISPATCHED: AtomicU64 = AtomicU64::new(0);

pub fn note_text_shaping() {
    TEXT_SHAPING_CALLS.fetch_add(1, Ordering::Relaxed);
}

pub fn note_node_lowered() {
    NODES_LOWERED.fetch_add(1, Ordering::Relaxed);
}

pub fn note_output_bytes(bytes: usize) {
    OUTPUT_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
}

pub fn note_frame_presented() {
    FRAMES_PRESENTED.fetch_add(1, Ordering::Relaxed);
}

pub fn note_event_dispatched() {
    EVENTS_DISPATCHED.fetch_add(1, Ordering::Relaxed);
}

// A snapshot of every counter. Counters have bounded cardinality and never
// carry text, paths, or identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeMetrics {
    pub text_shaping_calls: u64,
    pub nodes_lowered: u64,
    pub output_bytes: u64,
    pub frames_presented: u64,
    pub events_dispatched: u64,
}

impl RuntimeMetrics {
    // Difference from `earlier`, for measuring one operation.
    pub fn since(self, earlier: Self) -> Self {
        Self {
            text_shaping_calls: self
                .text_shaping_calls
                .saturating_sub(earlier.text_shaping_calls),
            nodes_lowered: self.nodes_lowered.saturating_sub(earlier.nodes_lowered),
            output_bytes: self.output_bytes.saturating_sub(earlier.output_bytes),
            frames_presented: self
                .frames_presented
                .saturating_sub(earlier.frames_presented),
            events_dispatched: self
                .events_dispatched
                .saturating_sub(earlier.events_dispatched),
        }
    }
}

pub fn runtime_metrics() -> RuntimeMetrics {
    RuntimeMetrics {
        text_shaping_calls: TEXT_SHAPING_CALLS.load(Ordering::Relaxed),
        nodes_lowered: NODES_LOWERED.load(Ordering::Relaxed),
        output_bytes: OUTPUT_BYTES.load(Ordering::Relaxed),
        frames_presented: FRAMES_PRESENTED.load(Ordering::Relaxed),
        events_dispatched: EVENTS_DISPATCHED.load(Ordering::Relaxed),
    }
}
