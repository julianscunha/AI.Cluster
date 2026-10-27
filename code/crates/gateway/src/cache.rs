//! Exact response cache (F14, M1). Generation is greedy, so the same prompt on the same weights
//! gives the same tokens: the second ask can be answered from memory instead of running the
//! model again. "Exact" is the point: the key is the whole token sequence, the model, its version
//! and the length asked for, with no similarity involved. Each tenant has its own entries, so a
//! fast answer never tells one tenant what another has asked.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    /// Per tenant, so a hit says nothing about what other tenants asked.
    tenant: String,
    model: &'static str,
    version: Option<String>,
    new_tokens: usize,
    prompt: Vec<u32>,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<Key, Vec<u32>>,
    /// Oldest use first. A hit moves the key to the back.
    order: VecDeque<Key>,
}

/// Bounded by entry count; `0` entries disables it. A response is at most `MAX_NEW_TOKENS`
/// tokens and a prompt at most `MAX_PROMPT_TOKENS`, so the memory bound is entries times a few KiB.
pub struct ResponseCache {
    capacity: usize,
    inner: Mutex<Inner>,
    hits: AtomicU64,
    misses: AtomicU64,
    tokens_avoided: AtomicU64,
}

impl ResponseCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: Mutex::new(Inner::default()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            tokens_avoided: AtomicU64::new(0),
        }
    }

    /// `(hits, misses, tokens avoided)` since the process started (or the restored base).
    pub fn counts(&self) -> (u64, u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
            self.tokens_avoided.load(Ordering::Relaxed),
        )
    }

    /// Starts the counters from a saved base; the entries themselves are not persisted.
    pub fn restore_counts(&self, hits: u64, misses: u64, tokens_avoided: u64) {
        self.hits.store(hits, Ordering::Relaxed);
        self.misses.store(misses, Ordering::Relaxed);
        self.tokens_avoided.store(tokens_avoided, Ordering::Relaxed);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn key(
        tenant: &str,
        model: &'static str,
        version: &Option<String>,
        prompt: &[u32],
        new_tokens: usize,
    ) -> Key {
        Key {
            tenant: tenant.to_string(),
            model,
            version: version.clone(),
            new_tokens,
            prompt: prompt.to_vec(),
        }
    }

    /// The generated tokens for this exact request, if held. Counts the hit or the miss.
    pub fn get(
        &self,
        tenant: &str,
        model: &'static str,
        version: &Option<String>,
        prompt: &[u32],
        new_tokens: usize,
    ) -> Option<Vec<u32>> {
        if self.capacity == 0 {
            return None;
        }
        let key = Self::key(tenant, model, version, prompt, new_tokens);
        let mut inner = self.lock();
        match inner.entries.get(&key).cloned() {
            Some(tokens) => {
                if let Some(position) = inner.order.iter().position(|known| *known == key) {
                    let used = inner.order.remove(position).expect("position is valid");
                    inner.order.push_back(used);
                }
                self.hits.fetch_add(1, Ordering::Relaxed);
                self.tokens_avoided
                    .fetch_add(tokens.len() as u64, Ordering::Relaxed);
                Some(tokens)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Keeps a finished generation, dropping the least recently used one when full.
    pub fn put(
        &self,
        tenant: &str,
        model: &'static str,
        version: &Option<String>,
        prompt: &[u32],
        new_tokens: usize,
        generated: &[u32],
    ) {
        if self.capacity == 0 {
            return;
        }
        let key = Self::key(tenant, model, version, prompt, new_tokens);
        let mut inner = self.lock();
        if inner
            .entries
            .insert(key.clone(), generated.to_vec())
            .is_none()
        {
            inner.order.push_back(key);
            if inner.order.len() > self.capacity
                && let Some(oldest) = inner.order.pop_front()
            {
                inner.entries.remove(&oldest);
            }
        }
    }

    /// Prometheus text for `GET /metrics`.
    pub fn render(&self) -> String {
        let inner = self.lock();
        let mut out = String::new();
        for (name, kind, help, value) in [
            (
                "ai_cluster_cache_hits_total",
                "counter",
                "Requests answered from the exact response cache.",
                self.hits.load(Ordering::Relaxed),
            ),
            (
                "ai_cluster_cache_misses_total",
                "counter",
                "Requests that ran the model.",
                self.misses.load(Ordering::Relaxed),
            ),
            (
                "ai_cluster_cache_tokens_avoided_total",
                "counter",
                "Tokens not generated because of a cache hit.",
                self.tokens_avoided.load(Ordering::Relaxed),
            ),
            (
                "ai_cluster_cache_entries",
                "gauge",
                "Responses held.",
                inner.entries.len() as u64,
            ),
        ] {
            out.push_str(&format!(
                "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}\n"
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_request_hits_and_any_difference_misses() {
        let cache = ResponseCache::new(4);
        let version = Some("v1".to_string());
        assert_eq!(cache.get("t", "gpt2", &version, &[1, 2, 3], 5), None);
        cache.put("t", "gpt2", &version, &[1, 2, 3], 5, &[9, 8, 7]);
        assert_eq!(
            cache.get("t", "gpt2", &version, &[1, 2, 3], 5),
            Some(vec![9, 8, 7])
        );
        // Another length, another prompt, another model or another version is another answer.
        assert_eq!(cache.get("t", "gpt2", &version, &[1, 2, 3], 6), None);
        assert_eq!(cache.get("t", "gpt2", &version, &[1, 2], 5), None);
        assert_eq!(cache.get("t", "smollm2", &version, &[1, 2, 3], 5), None);
        assert_eq!(
            cache.get("t", "gpt2", &Some("v2".into()), &[1, 2, 3], 5),
            None
        );
        assert_eq!(cache.get("t", "gpt2", &None, &[1, 2, 3], 5), None);
    }

    #[test]
    fn the_least_recently_used_goes_first_and_zero_disables() {
        let cache = ResponseCache::new(2);
        let none = None;
        cache.put("t", "m", &none, &[1], 1, &[1]);
        cache.put("t", "m", &none, &[2], 1, &[2]);
        // Touch [1] so [2] is the oldest.
        assert!(cache.get("t", "m", &none, &[1], 1).is_some());
        cache.put("t", "m", &none, &[3], 1, &[3]);
        assert!(cache.get("t", "m", &none, &[2], 1).is_none());
        assert!(cache.get("t", "m", &none, &[1], 1).is_some());
        assert!(cache.get("t", "m", &none, &[3], 1).is_some());

        let off = ResponseCache::new(0);
        off.put("t", "m", &none, &[1], 1, &[1]);
        assert!(off.get("t", "m", &none, &[1], 1).is_none());
    }

    #[test]
    fn a_lookup_costs_microseconds_next_to_a_generation() {
        // The worst key: a full 512-token prompt, in a full cache of such prompts.
        let cache = ResponseCache::new(256);
        for n in 0..256u32 {
            let prompt: Vec<u32> = (0..512).map(|i| i + n).collect();
            cache.put("t", "m", &None, &prompt, 64, &[1; 64]);
        }
        let probe: Vec<u32> = (0..512).map(|i| i + 255).collect();
        let started = std::time::Instant::now();
        for _ in 0..100 {
            assert!(cache.get("t", "m", &None, &probe, 64).is_some());
        }
        let per_lookup = started.elapsed() / 100;
        // A generation takes hundreds of milliseconds; the lookup must be far below one.
        assert!(
            per_lookup < std::time::Duration::from_millis(5),
            "{per_lookup:?}"
        );
    }

    #[test]
    fn hits_and_avoided_tokens_are_counted() {
        let cache = ResponseCache::new(2);
        cache.put("t", "m", &None, &[1], 3, &[4, 5, 6]);
        cache.get("t", "m", &None, &[1], 3);
        cache.get("t", "m", &None, &[1], 3);
        cache.get("t", "m", &None, &[2], 3);
        let text = cache.render();
        assert!(text.contains("ai_cluster_cache_hits_total 2"));
        assert!(text.contains("ai_cluster_cache_misses_total 1"));
        assert!(text.contains("ai_cluster_cache_tokens_avoided_total 6"));
        assert!(text.contains("ai_cluster_cache_entries 1"));
    }
}
