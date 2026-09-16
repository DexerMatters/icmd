//! Image source manager: worker pool, bounded queues, byte budget, and the source cache.
//! Owns request scheduling, backpressure handling, result storage, and cache eviction bookkeeping.

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

pub(crate) const JOB_QUEUE_CAPACITY: usize = 32;
pub(crate) const RESULT_QUEUE_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScheduleResult {
    Queued,
    Backpressured,
    Closed,
}

/// Owns the image worker pool and its bounded job and result channels.
#[derive(Debug)]
pub struct ImageLoader {
    jobs: Sender<ImageSource>,
    results: Receiver<LoadResult>,
    high_water: Arc<QueueHighWater>,
}

impl ImageLoader {
    /// Spawns `workers` loader threads over a 32-job queue feeding a 64-result queue, sharing `budget`, the in-flight decoded-byte cap.
    pub fn with_workers(
        workers: usize,
        gate: Option<Receiver<()>>,
        limits: ResourceLimits,
        budget: Arc<ByteBudget>,
    ) -> Self {
        let high_water = Arc::new(QueueHighWater::default());
        let (jobs, job_rx) = crossbeam_channel::bounded::<ImageSource>(JOB_QUEUE_CAPACITY);
        let (result_tx, results) = crossbeam_channel::bounded(RESULT_QUEUE_CAPACITY);
        for _ in 0..workers {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            let gate = gate.clone();
            let high_water = high_water.clone();
            let budget = budget.clone();
            thread::Builder::new()
                .name("icmd-image-loader".to_string())
                .spawn(move || {
                    while let Ok(source) = job_rx.recv() {
                        if let Some(gate) = &gate {
                            let _ = gate.recv();
                        }
                        let result = source.load_with_limits(&limits);
                        let bytes = result
                            .as_ref()
                            .map(|image| image.rgba8().len())
                            .unwrap_or(0);
                        let _reservation = budget.reserve(bytes).ok();
                        high_water.observe_results(result_tx.len().saturating_add(1));
                        if result_tx.send((source, result)).is_err() {
                            break;
                        }
                    }
                })
                .expect("failed to spawn image loader");
        }
        Self {
            jobs,
            results,
            high_water,
        }
    }

    fn schedule(&self, source: ImageSource) -> ScheduleResult {
        self.high_water
            .observe_jobs(self.jobs.len().saturating_add(1));
        match self.jobs.try_send(source) {
            Ok(()) => ScheduleResult::Queued,
            Err(crossbeam_channel::TrySendError::Full(_)) => ScheduleResult::Backpressured,
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => ScheduleResult::Closed,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct QueueHighWater {
    job: std::sync::atomic::AtomicUsize,
    result: std::sync::atomic::AtomicUsize,
}

impl QueueHighWater {
    pub(crate) fn observe_jobs(&self, depth: usize) {
        self.job
            .fetch_max(depth, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn observe_results(&self, depth: usize) {
        self.result
            .fetch_max(depth, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn jobs(&self) -> usize {
        self.job.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn results(&self) -> usize {
        self.result.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Concurrency-safe accounting of decoded image bytes in flight, shared by every worker.
/// A reservation is RAII: capacity returns to the pool when the decoded buffer is dropped, on both success and failure.
#[derive(Debug)]
pub struct ByteBudget {
    limit: usize,
    used: std::sync::atomic::AtomicUsize,
}

impl ByteBudget {
    /// Creates a budget admitting at most `limit` decoded bytes in flight.
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            used: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(crate) fn reserve(self: &Arc<Self>, bytes: usize) -> Result<ByteReservation, ()> {
        let mut current = self.used.load(std::sync::atomic::Ordering::SeqCst);
        loop {
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

/// Cached load state and last-use accounting for one image source.
#[derive(Debug)]
pub struct SourceCacheEntry {
    /// Current load state of the source.
    pub state: SourceState,
    bytes: usize,
    used: u64,
}

/// Load state of a cached image source.
#[derive(Debug)]
pub enum SourceState {
    /// A worker is decoding the source, or the request is queued for one.
    Loading,
    /// Decoded pixels are available.
    Ready(RasterImage),
    /// Decoding failed for this source.
    Failed,
}

/// Outcome of asking the manager to load an image source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRequest {
    /// The source is already loaded or cached, so no work was scheduled.
    AlreadyAvailable,
    /// The source was handed to a worker or is pending for one.
    Queued,
    /// The worker queue is momentarily full; the request stays pending and retryable, never a cache-poisoning error.
    Backpressured,
    /// The worker queue is gone, a terminal runtime condition rather than an image-decode failure.
    Closed,
}

/// Owns image-source loading, the source cache, and the deferred request queues.
#[derive(Debug)]
pub struct ImageManager {
    loader: ImageLoader,
    budget: Arc<ByteBudget>,
    /// Cache keyed by cache identity, so two spellings of the same opened file share one load and one entry.
    pub source_cache: HashMap<ImageSourceKey, SourceCacheEntry>,
    source_cache_bytes: usize,
    /// Keys currently requested or in flight, used to deduplicate loads.
    pub loading: HashSet<ImageSourceKey>,
    pending: VecDeque<(ImageSourceKey, ImageSource)>,
    pending_loads: VecDeque<LoadResult>,
    cache_tick: u64,
}

impl ImageManager {
    pub(crate) fn with_limits(limits: ResourceLimits) -> Self {
        let budget = Arc::new(ByteBudget::new(limits.max_in_flight_image_bytes));
        let loader = ImageLoader::with_workers(2, None, limits, budget.clone());
        Self::with_loader(loader, budget)
    }

    /// Creates a manager over an existing `loader` and `budget`.
    pub fn with_loader(loader: ImageLoader, budget: Arc<ByteBudget>) -> Self {
        Self {
            loader,
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

    /// Drains every completed load result and retries requests deferred by earlier backpressure.
    pub fn take_results(&mut self) -> Vec<LoadResult> {
        let mut results = Vec::new();
        while let Some(result) = self
            .pending_loads
            .pop_front()
            .or_else(|| self.loader.results.try_recv().ok())
        {
            results.push(result);
        }
        self.pump();
        results
    }

    /// Returns decoded pixels for `source` when it is loaded directly or cached as ready.
    pub fn source_image(&self, source: &ImageSource) -> Option<RasterImage> {
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

    /// Requests a load for `source`, reusing any cached, in-flight, or pending entry.
    pub fn request(&mut self, source: &ImageSource) -> SourceRequest {
        let key = source.cache_key();
        if source.loaded_image().is_some() || self.source_cache.contains_key(&key) {
            if let Some(entry) = self.source_cache.get_mut(&key) {
                self.cache_tick = self.cache_tick.saturating_add(1);
                entry.used = self.cache_tick;
            }
            return SourceRequest::AlreadyAvailable;
        }
        if !self.loading.insert(key.clone()) {
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

    /// Stores a completed load result in the source cache and returns whether it is ready.
    pub fn store_result(
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

    pub(crate) fn job_queue_high_water(&self) -> usize {
        self.loader.high_water.jobs()
    }

    pub(crate) fn result_queue_high_water(&self) -> usize {
        self.loader.high_water.results()
    }

    pub(crate) fn result_backlog(&self) -> usize {
        self.loader
            .results
            .len()
            .saturating_add(self.pending_loads.len())
    }

    /// Number of sources waiting for worker queue capacity.
    pub fn pending_count(&self) -> usize {
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
