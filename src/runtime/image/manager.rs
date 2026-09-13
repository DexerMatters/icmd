use std::{
    collections::{HashMap, HashSet, VecDeque},
    thread,
};

use crossbeam_channel::{Receiver, Sender};

use crate::raster::ImageSourceKey;
use crate::{Cell, Image, ImageSource, RasterImage, RasterImageError, RasterPlacement};

pub(crate) type LoadResult = (ImageSource, Result<RasterImage, RasterImageError>);

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
    fn new() -> Self {
        Self::with_workers(2, None)
    }

    fn with_workers(workers: usize, gate: Option<Receiver<()>>) -> Self {
        let (jobs, job_rx) = crossbeam_channel::bounded::<ImageSource>(32);
        // Bounded results: decoded pixels cannot accumulate without limit while
        // the renderer is busy. Workers block once the queue is full, which is
        // exactly the backpressure the render loop drains.
        let (result_tx, results) = crossbeam_channel::bounded(64);
        for _ in 0..workers {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            let gate = gate.clone();
            thread::spawn(move || {
                while let Ok(source) = job_rx.recv() {
                    if let Some(gate) = &gate {
                        // Test-only stall point: a disconnected gate releases
                        // every present and future job.
                        let _ = gate.recv();
                    }
                    let result = source.load();
                    if result_tx.send((source, result)).is_err() {
                        break;
                    }
                }
            });
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
    pub(crate) fn new() -> Self {
        Self::with_loader(ImageLoader::new())
    }

    fn with_loader(loader: ImageLoader) -> Self {
        Self {
            loader,
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

    #[cfg_attr(not(test), allow(dead_code))]
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
        let mut manager = ImageManager::with_loader(ImageLoader::with_workers(2, Some(gate_rx)));
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
        let mut manager = ImageManager::with_loader(ImageLoader::with_workers(0, None));
        assert_eq!(manager.request(&source("closed")), SourceRequest::Closed);
        // A closed queue is not a decode failure and must not be cached as one.
        assert!(manager.source_image(&source("closed")).is_none());
        assert!(manager.source_cache.is_empty());
    }
}
