// Image scheduling tests: queue saturation, retryable backpressure, and byte
// accounting. The manager runs inside the crate, so its internals are reached
// through the hidden module.
use std::sync::Arc;

use icmd::__private::{ByteBudget, ImageLoader, ImageManager, SourceRequest, SourceState};
use icmd::advanced::ResourceLimits;
use icmd::image::ImageSource;

fn source(name: &str) -> ImageSource {
    ImageSource::file(format!("/nonexistent/icmd-test/{name}.png"))
}

#[test]
fn a_full_queue_is_backpressure_not_failure() {
    // Two workers, both stalled; the 32-slot job queue then fills up.
    let (gate_tx, gate_rx) = crossbeam_channel::bounded(0);
    let limits = ResourceLimits::default();
    let budget = Arc::new(ByteBudget::new(limits.max_in_flight_image_bytes));
    let loader = ImageLoader::with_workers(2, Some(gate_rx), limits, budget.clone());
    let mut manager = ImageManager::with_loader(loader, budget);
    let mut backpressured = None;
    for index in 0..64 {
        let outcome = manager.request(&source(&index.to_string()));
        if outcome == SourceRequest::Backpressured {
            backpressured = Some(index);
            break;
        }
    }
    let index = backpressured.expect("filling the worker queue must backpressure");
    assert!(index > 30, "backpressure started too early at {index}");
    assert!(manager.pending_count() >= 1);

    // The deferred source is still retryable: it must not be cached as a
    // permanent failure, so no caller sees the "×" fallback for it.
    let deferred = source(&index.to_string());
    assert!(manager.source_image(&deferred).is_none());
    assert!(
        !manager
            .source_cache
            .get(&deferred.cache_key())
            .is_some_and(|entry| matches!(entry.state, SourceState::Failed)),
        "backpressure must not poison the cache with a failure"
    );

    // Releasing the workers lets every referenced source settle: each file
    // does not exist, so each ends in a real (decode/io) failure, not a
    // synthetic one caused by the full queue.
    drop(gate_tx);
    for _ in 0..200 {
        let results = manager.take_results();
        for (source, result) in results {
            manager.store_result(source, result);
        }
        if manager.pending_count() == 0 && manager.loading.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(manager.pending_count(), 0, "deferred work must drain");
    assert!(manager.loading.is_empty(), "every request must settle");
}

#[test]
fn a_disconnected_queue_reports_closed() {
    // Zero workers drop the job receiver immediately, so scheduling has no
    // destination at all.
    let limits = ResourceLimits::default();
    let budget = Arc::new(ByteBudget::new(limits.max_in_flight_image_bytes));
    let loader = ImageLoader::with_workers(0, None, limits, budget.clone());
    let mut manager = ImageManager::with_loader(loader, budget);
    assert_eq!(manager.request(&source("closed")), SourceRequest::Closed);
    // A closed queue is not a decode failure and must not be cached as one.
    assert!(manager.source_image(&source("closed")).is_none());
    assert!(manager.source_cache.is_empty());
}
