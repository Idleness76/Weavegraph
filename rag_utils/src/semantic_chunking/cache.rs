use std::collections::hash_map::DefaultHasher;
use std::collections::{hash_map::Entry, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard};

/// Snapshot of cache interactions, useful for telemetry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheMetrics {
    pub hits: usize,
    pub misses: usize,
}

/// Shared handle that coordinates cache configuration across chunkers.
#[derive(Clone, Default)]
pub struct CacheHandle {
    inner: Arc<Mutex<Option<EmbeddingCache>>>,
}

impl CacheHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_capacity(capacity: Option<usize>) -> Self {
        let handle = Self::new();
        handle.apply_capacity(capacity);
        handle
    }

    pub fn apply_capacity(&self, capacity: Option<usize>) {
        let mut guard = self.lock();
        match capacity {
            Some(0) => {
                *guard = None;
            }
            Some(limit) => {
                let replace = match guard.as_ref() {
                    Some(existing) => existing.capacity() != Some(limit),
                    None => true,
                };
                if replace {
                    *guard = Some(EmbeddingCache::new(Some(limit)));
                }
            }
            None => {}
        }
    }

    pub fn disable(&self) {
        *self.lock() = None;
    }

    pub fn capacity(&self) -> Option<usize> {
        let guard = self.inner.lock().expect("cache mutex poisoned");
        guard.as_ref().and_then(|cache| cache.capacity())
    }

    pub fn metrics(&self) -> Option<CacheMetrics> {
        let guard = self.inner.lock().expect("cache mutex poisoned");
        guard.as_ref().map(|cache| cache.metrics())
    }

    pub fn inner(&self) -> Arc<Mutex<Option<EmbeddingCache>>> {
        self.inner.clone()
    }

    pub fn lock(&self) -> MutexGuard<'_, Option<EmbeddingCache>> {
        self.inner.lock().expect("cache mutex poisoned")
    }
}

#[derive(Debug)]
pub struct EmbeddingCache {
    capacity: Option<usize>,
    entries: HashMap<u64, Vec<f32>>,
    order: VecDeque<u64>,
    hits: usize,
    misses: usize,
}

impl EmbeddingCache {
    pub fn new(capacity: Option<usize>) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
            hits: 0,
            misses: 0,
        }
    }

    pub fn capacity(&self) -> Option<usize> {
        self.capacity
    }

    pub fn get(&mut self, key: &str) -> Option<Vec<f32>> {
        let hash = hash_text(key);
        if let Some(value) = self.entries.get(&hash) {
            // refresh order for simple LRU behaviour
            refresh(&mut self.order, hash);
            self.hits += 1;
            Some(value.clone())
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn insert(&mut self, key: &str, embedding: Vec<f32>) {
        let hash = hash_text(key);
        if let Entry::Occupied(mut existing) = self.entries.entry(hash) {
            existing.insert(embedding);
            refresh(&mut self.order, hash);
            return;
        }

        if let Some(limit) = self.capacity {
            while self.order.len() >= limit {
                if let Some(evicted) = self.order.pop_front() {
                    self.entries.remove(&evicted);
                } else {
                    break;
                }
            }
        }

        self.order.push_back(hash);
        self.entries.insert(hash, embedding);
    }

    pub fn metrics(&self) -> CacheMetrics {
        CacheMetrics {
            hits: self.hits,
            misses: self.misses,
        }
    }
}

fn refresh(order: &mut VecDeque<u64>, hash: u64) {
    if let Some(pos) = order.iter().position(|value| *value == hash) {
        order.remove(pos);
    }
    order.push_back(hash);
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
