use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    thread,
};

use crossbeam_channel::{Receiver, Sender};

use crate::raster::ImageSourceKey;
use crate::runtime::limits::ResourceLimits;
use crate::{Cell, Image, ImageSource, RasterImage, RasterImageError, RasterPlacement};

pub(crate) type LoadResult = (ImageSource, Result<RasterImage, RasterImageError>);

// Bounded capacities, exposed through `ImageMetrics` so queue pressure and
// result backlog are observable rather than implicit.
pub(crate) const JOB_QUEUE_CAPACITY: usize = 32;
pub(crate) const RESULT_QUEUE_CAPACITY: usize = 64;

// Why a scheduling attempt did not (or did) enqueue work. A full worker queue is
// backpressure, never a decode failure: the request stays pending and retryable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScheduleResult {
    Queued,
    Backpressured,
    Closed,
}

#[derive(Debug)]
struct ImageLoader {
    jobs: Sender<ImageSource>,
    results: Receiver<LoadResult>,
}

impl ImageLoader {
    fn with_workers(
        workers: usize,
        gate: Option<Receiver<()>>,
        limits: ResourceLimits,
        budget: Arc<ByteBudget>,
    ) -> Self {
        let (jobs, job_rx) = crossbeam_channel::bounded::<ImageSource>(JOB_QUEUE_CAPACITY);
        // Bounded results: decoded pixels cannot accumulate without limit while
        // the renderer is busy. Workers block once the queue is full, which is
        // exactly the backpressure the render loop drains.
        let (result_tx, results) = crossbeam_channel::bounded(RESULT_QUEUE_CAPACITY);
        for _ in 0..workers {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            let gate = gate.clone();
            let budget = budget.clone();
            thread::Builder::new()
                .name("icmd-image-loader".to_string())
                .spawn(move || {
                    while let Ok(source) = job_rx.recv() {
                        if let Some(gate) = &gate {
                            // Test-only stall point: a disconnected gate
                            // releases every present and future job.
                            let _ = gate.recv();
                        }
                        let result = source.load_with_limits(&limits);
                        // The reservation is released as soon as the decoded
                        // pixels leave this worker, on every return path.
                        let bytes = result
                            .as_ref()
                            .map(|image| image.rgba8().len())
                            .unwrap_or(0);
                        let _reservation = budget.reserve(bytes).ok();
                        if result_tx.send((source, result)).is_err() {
                            break;
                        }
                    }
                })
                .expect("failed to spawn image loader");
        }
        Self { jobs, results }
    }

    fn schedule(&self, source: ImageSource) -> ScheduleResult {
        match self.jobs.try_send(source) {
            Ok(()) => ScheduleResult::Queued,
            Err(crossbeam_channel::TrySendError::Full(_)) => ScheduleResult::Backpressured,
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => ScheduleResult::Closed,
        }
    }
}

// Concurrency-safe byte accounting shared by every image worker. A reservation
// is RAII: capacity returns to the pool when the decoded buffer is dropped, on
// both the success and failure paths.
#[derive(Debug)]
pub(crate) struct ByteBudget {
    limit: usize,
    used: std::sync::atomic::AtomicUsize,
}

impl ByteBudget {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            used: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(crate) fn reserve(self: &Arc<Self>, bytes: usize) -> Result<ByteReservation, ()> {
        let mut current = self.used.load(std::sync::atomic::Ordering::SeqCst);
        loop {
            // `checked_add` keeps an accounting overflow from wrapping into a
            // silently small value.
            let Some(next) = current.checked_add(bytes) else {
                return Err(());
            };
            if next > self.limit {
                return Err(());
            }
            match self.used.compare_exchange_weak(
                current,
                next,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Ok(ByteReservation {
                        budget: self.clone(),
                        bytes,
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }

    pub(crate) fn used(&self) -> usize {
        self.used.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub(crate) struct ByteReservation {
    budget: Arc<ByteBudget>,
    bytes: usize,
}

impl Drop for ByteReservation {
    fn drop(&mut self) {
        self.budget
            .used
            .fetch_sub(self.bytes, std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct SourceCacheEntry {
    state: SourceState,
    bytes: usize,
    used: u64,
}

#[derive(Debug)]
enum SourceState {
    Loading,
    Ready(RasterImage),
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceRequest {
    AlreadyAvailable,
    Queued,
    // The worker queue is momentarily full. The request is retained and retried;
    // it is not an error and never poisons the cache.
    Backpressured,
    // The worker queue is gone. This is a terminal runtime condition, not an
    // image-decode failure.
    Closed,
}

#[derive(Debug)]
pub(crate) struct ImageManager {
    loader: ImageLoader,
    limits: ResourceLimits,
    budget: Arc<ByteBudget>,
    // Keyed by cache identity, so two spellings of the same opened file share
    // one load and one entry.
    source_cache: HashMap<ImageSourceKey, SourceCacheEntry>,
    source_cache_bytes: usize,
    loading: HashSet<ImageSourceKey>,
    // Deduplicated queue of sources that could not be handed to a worker yet.
    // The source travels with its key so a retry can still load it.
    pending: VecDeque<(ImageSourceKey, ImageSource)>,
    pending_loads: VecDeque<LoadResult>,
    cache_tick: u64,
}

impl ImageManager {
    pub(crate) fn with_limits(limits: ResourceLimits) -> Self {
        let budget = Arc::new(ByteBudget::new(limits.max_in_flight_image_bytes));
        let loader = ImageLoader::with_workers(2, None, limits, budget.clone());
        Self::with_loader(loader, limits, budget)
    }

    fn with_loader(loader: ImageLoader, limits: ResourceLimits, budget: Arc<ByteBudget>) -> Self {
        Self {
            loader,
            limits,
            budget,
            source_cache: HashMap::new(),
            source_cache_bytes: 0,
            loading: HashSet::new(),
            pending: VecDeque::new(),
            pending_loads: VecDeque::new(),
            cache_tick: 0,
        }
    }

    pub(crate) fn results(&self) -> &Receiver<LoadResult> {
        &self.loader.results
    }

    pub(crate) fn queue_result(&mut self, result: LoadResult) {
        self.pending_loads.push_back(result);
    }

    pub(crate) fn take_results(&mut self) -> Vec<LoadResult> {
        let mut results = Vec::new();
        while let Some(result) = self
            .pending_loads
            .pop_front()
            .or_else(|| self.loader.results.try_recv().ok())
        {
            results.push(result);
        }
        // Draining results frees worker capacity, so retry anything deferred by
        // earlier backpressure before the caller renders again.
        self.pump();
        results
    }

    pub(crate) fn source_image(&self, source: &ImageSource) -> Option<RasterImage> {
        match source {
            ImageSource::Loaded(image) => Some(image.clone()),
            ImageSource::File(_) => {
                let key = source.cache_key();
                self.source_cache
                    .get(&key)
                    .and_then(|entry| match &entry.state {
                        SourceState::Ready(image) => Some(image.clone()),
                        SourceState::Loading | SourceState::Failed => None,
                    })
            }
        }
    }

    pub(crate) fn initial_fallback(&self, raster: &RasterPlacement) -> Option<Image> {
        if raster.invalid_source {
            return placeholder(raster.width, raster.height, "×");
        }
        if raster.source.loaded_image().is_some() {
            return None;
        }
        let key = raster.source.cache_key();
        let symbol = match self.source_cache.get(&key).map(|entry| &entry.state) {
            Some(SourceState::Ready(_)) => return None,
            Some(SourceState::Failed) => "×",
            Some(SourceState::Loading) | None => "…",
        };
        placeholder(raster.width, raster.height, symbol)
    }

    pub(crate) fn request(&mut self, source: &ImageSource) -> SourceRequest {
        let key = source.cache_key();
        if source.loaded_image().is_some() || self.source_cache.contains_key(&key) {
            if let Some(entry) = self.source_cache.get_mut(&key) {
                self.cache_tick = self.cache_tick.saturating_add(1);
                entry.used = self.cache_tick;
            }
            return SourceRequest::AlreadyAvailable;
        }
        if !self.loading.insert(key.clone()) {
            // Already in flight or waiting for a worker: still not an error.
            return SourceRequest::AlreadyAvailable;
        }
        self.cache_tick = self.cache_tick.saturating_add(1);
        self.pending.push_back((key.clone(), source.clone()));
        match self.pump() {
            ScheduleResult::Closed => SourceRequest::Closed,
            _ if self.pending.iter().any(|(pending, _)| pending == &key) => {
                SourceRequest::Backpressured
            }
            _ => SourceRequest::Queued,
        }
    }

    // Move as many deferred sources into the worker queue as capacity allows.
    fn pump(&mut self) -> ScheduleResult {
        while let Some((key, source)) = self.pending.front().cloned() {
            match self.loader.schedule(source) {
                ScheduleResult::Queued => {
                    self.pending.pop_front();
                    self.cache_tick = self.cache_tick.saturating_add(1);
                    self.source_cache.insert(
                        key,
                        SourceCacheEntry {
                            state: SourceState::Loading,
                            bytes: 0,
                            used: self.cache_tick,
                        },
                    );
                }
                ScheduleResult::Backpressured => return ScheduleResult::Backpressured,
                ScheduleResult::Closed => {
                    // Do not convert a closed queue into a per-source decode
                    // failure: the cache stays untouched so the runtime can
                    // surface a terminal error instead.
                    self.pending.clear();
                    self.loading.clear();
                    return ScheduleResult::Closed;
                }
            }
        }
        ScheduleResult::Queued
    }

    pub(crate) fn remove_cached(&mut self, source: &ImageSource) {
        let key = source.cache_key();
        self.loading.remove(&key);
        self.pending.retain(|(pending, _)| pending != &key);
        if let Some(previous) = self.source_cache.remove(&key) {
            self.source_cache_bytes = self.source_cache_bytes.saturating_sub(previous.bytes);
        }
    }

    pub(crate) fn store_result(
        &mut self,
        source: ImageSource,
        result: Result<RasterImage, RasterImageError>,
    ) -> bool {
        let key = source.cache_key();
        self.loading.remove(&key);
        self.pending.retain(|(pending, _)| pending != &key);
        let bytes = result
            .as_ref()
            .map(|image| image.rgba8().len())
            .unwrap_or(0);
        self.cache_tick = self.cache_tick.saturating_add(1);
        self.source_cache_bytes = self.source_cache_bytes.saturating_add(bytes);
        let ready = result.is_ok();
        self.source_cache.insert(
            key,
            SourceCacheEntry {
                state: match result {
                    Ok(image) => SourceState::Ready(image),
                    Err(_) => SourceState::Failed,
                },
                bytes,
                used: self.cache_tick,
            },
        );
        ready
    }

    pub(crate) fn source_cache_bytes(&self) -> usize {
        self.source_cache_bytes
    }

    pub(crate) fn bytes_in_flight(&self) -> usize {
        self.budget.used()
    }

    pub(crate) fn job_capacity(&self) -> usize {
        JOB_QUEUE_CAPACITY
    }

    pub(crate) fn result_capacity(&self) -> usize {
        RESULT_QUEUE_CAPACITY
    }

    pub(crate) fn result_backlog(&self) -> usize {
        self.loader
            .results
            .len()
            .saturating_add(self.pending_loads.len())
    }

    pub(crate) fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn oldest_inactive_source(
        &self,
        pinned: &HashSet<ImageSourceKey>,
    ) -> Option<ImageSourceKey> {
        self.source_cache
            .iter()
            .filter(|(key, entry)| entry.bytes > 0 && !pinned.contains(*key))
            .min_by_key(|(_, entry)| entry.used)
            .map(|(key, _)| key.clone())
    }

    pub(crate) fn remove_cached_key(&mut self, key: &ImageSourceKey) {
        self.loading.remove(key);
        self.pending.retain(|(pending, _)| pending != key);
        if let Some(previous) = self.source_cache.remove(key) {
            self.source_cache_bytes = self.source_cache_bytes.saturating_sub(previous.bytes);
        }
    }
}

pub(crate) fn placeholder(width: u16, height: u16, symbol: &str) -> Option<Image> {
    if width == 0 || height == 0 {
        return None;
    }
    let cell = Cell::plain(symbol).ok()?;
    let mut image = Image::new(usize::from(width), usize::from(height), Cell::blank()).ok()?;
    let position = crate::ImagePosition::new(
        usize::from(height.saturating_sub(1)) / 2,
        usize::from(width.saturating_sub(1)) / 2,
    );
    image
        .patch_cells(&[crate::CellEdit { position, cell }])
        .ok()?;
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut manager = ImageManager::with_loader(loader, limits, budget);
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
        let mut manager = ImageManager::with_loader(loader, limits, budget);
        assert_eq!(manager.request(&source("closed")), SourceRequest::Closed);
        // A closed queue is not a decode failure and must not be cached as one.
        assert!(manager.source_image(&source("closed")).is_none());
        assert!(manager.source_cache.is_empty());
    }
}
