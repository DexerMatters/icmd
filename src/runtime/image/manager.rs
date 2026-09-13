use std::{
    collections::{HashMap, HashSet, VecDeque},
    thread,
};

use crossbeam_channel::{Receiver, Sender};

use crate::{Cell, Image, ImageSource, RasterImage, RasterImageError, RasterPlacement};

pub(crate) type LoadResult = (ImageSource, Result<RasterImage, RasterImageError>);

#[derive(Debug)]
struct ImageLoader {
    jobs: Sender<ImageSource>,
    results: Receiver<LoadResult>,
}

impl ImageLoader {
    fn new() -> Self {
        let (jobs, job_rx) = crossbeam_channel::bounded::<ImageSource>(32);
        let (result_tx, results) = crossbeam_channel::unbounded();
        for _ in 0..2 {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            thread::spawn(move || {
                while let Ok(source) = job_rx.recv() {
                    let result = source.load();
                    if result_tx.send((source, result)).is_err() {
                        break;
                    }
                }
            });
        }
        Self { jobs, results }
    }

    fn schedule(&self, source: ImageSource) -> bool {
        self.jobs.try_send(source).is_ok()
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
    Failed,
}

#[derive(Debug)]
pub(crate) struct ImageManager {
    loader: ImageLoader,
    source_cache: HashMap<ImageSource, SourceCacheEntry>,
    source_cache_bytes: usize,
    loading: HashSet<ImageSource>,
    pending_loads: VecDeque<LoadResult>,
    cache_tick: u64,
}

impl ImageManager {
    pub(crate) fn new() -> Self {
        Self {
            loader: ImageLoader::new(),
            source_cache: HashMap::new(),
            source_cache_bytes: 0,
            loading: HashSet::new(),
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
        results
    }

    pub(crate) fn source_image(&self, source: &ImageSource) -> Option<RasterImage> {
        match source {
            ImageSource::Loaded(image) => Some(image.clone()),
            ImageSource::File(_) => {
                self.source_cache
                    .get(source)
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
        let symbol = match self
            .source_cache
            .get(&raster.source)
            .map(|entry| &entry.state)
        {
            Some(SourceState::Ready(_)) => return None,
            Some(SourceState::Failed) => "×",
            Some(SourceState::Loading) | None => "…",
        };
        placeholder(raster.width, raster.height, symbol)
    }

    pub(crate) fn request(&mut self, source: &ImageSource) -> SourceRequest {
        if source.loaded_image().is_some() || self.source_cache.contains_key(source) {
            if let Some(entry) = self.source_cache.get_mut(source) {
                self.cache_tick = self.cache_tick.saturating_add(1);
                entry.used = self.cache_tick;
            }
            return SourceRequest::AlreadyAvailable;
        }
        if !self.loading.insert(source.clone()) {
            return SourceRequest::AlreadyAvailable;
        }
        self.cache_tick = self.cache_tick.saturating_add(1);
        if self.loader.schedule(source.clone()) {
            self.source_cache.insert(
                source.clone(),
                SourceCacheEntry {
                    state: SourceState::Loading,
                    bytes: 0,
                    used: self.cache_tick,
                },
            );
            SourceRequest::Queued
        } else {
            self.loading.remove(source);
            self.source_cache.insert(
                source.clone(),
                SourceCacheEntry {
                    state: SourceState::Failed,
                    bytes: 0,
                    used: self.cache_tick,
                },
            );
            SourceRequest::Failed
        }
    }

    pub(crate) fn remove_cached(&mut self, source: &ImageSource) {
        if let Some(previous) = self.source_cache.remove(source) {
            self.source_cache_bytes = self.source_cache_bytes.saturating_sub(previous.bytes);
        }
    }

    pub(crate) fn store_result(
        &mut self,
        source: ImageSource,
        result: Result<RasterImage, RasterImageError>,
    ) -> bool {
        self.loading.remove(&source);
        let bytes = result
            .as_ref()
            .map(|image| image.rgba8().len())
            .unwrap_or(0);
        self.cache_tick = self.cache_tick.saturating_add(1);
        self.source_cache_bytes = self.source_cache_bytes.saturating_add(bytes);
        let ready = result.is_ok();
        self.source_cache.insert(
            source,
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

    pub(crate) fn oldest_inactive_source(
        &self,
        pinned: &HashSet<ImageSource>,
    ) -> Option<ImageSource> {
        self.source_cache
            .iter()
            .filter(|(source, entry)| entry.bytes > 0 && !pinned.contains(*source))
            .min_by_key(|(_, entry)| entry.used)
            .map(|(source, _)| source.clone())
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
