//! Lifecycle event log.
//!
//! The controller already prints these facts to stdout. Recording them makes the
//! same timeline readable by a client, which is what an operator has instead of a
//! container log.

use std::collections::VecDeque;
use std::sync::Mutex;

use ai_cluster_protocol::cluster::Event;

/// Events held before the oldest is dropped.
///
/// ponytail: a bounded ring buffer, not a store. Enough to explain a recent
/// incident; ship events to a sink when history has to outlive the process.
const CAPACITY: usize = 256;

#[derive(Debug)]
pub struct EventLog {
    inner: Mutex<VecDeque<Event>>,
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLog {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::with_capacity(CAPACITY)),
        }
    }

    /// Appends one event, dropping the oldest once the buffer is full.
    pub fn record(&self, at_unix_ms: u64, kind: &str, detail: impl Into<String>) {
        let mut inner = self.lock();
        if inner.len() == CAPACITY {
            inner.pop_front();
        }
        inner.push_back(Event {
            at_unix_ms,
            kind: kind.to_string(),
            detail: detail.into(),
        });
    }

    /// The most recent `limit` events, oldest first. `limit` of 0 means every
    /// event still held.
    pub fn recent(&self, limit: usize) -> Vec<Event> {
        let inner = self.lock();
        let skip = if limit == 0 {
            0
        } else {
            inner.len().saturating_sub(limit)
        };
        inner.iter().skip(skip).cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<Event>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_oldest_once_full_and_keeps_the_newest() {
        let log = EventLog::new();
        for index in 0..(CAPACITY + 5) {
            log.record(index as u64, "test", format!("e{index}"));
        }

        let held = log.recent(0);
        assert_eq!(held.len(), CAPACITY);
        assert_eq!(held[0].detail, "e5");
        assert_eq!(
            held.last().expect("non-empty").detail,
            format!("e{}", CAPACITY + 4)
        );
    }

    #[test]
    fn recent_returns_the_last_n_oldest_first() {
        let log = EventLog::new();
        for index in 0..10 {
            log.record(index as u64, "test", format!("e{index}"));
        }

        let last_three: Vec<String> = log
            .recent(3)
            .iter()
            .map(|event| event.detail.clone())
            .collect();
        assert_eq!(last_three, vec!["e7", "e8", "e9"]);

        // A limit above what is held returns everything rather than padding.
        assert_eq!(log.recent(99).len(), 10);
    }
}
