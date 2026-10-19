//! Dispatch queue for Phase 0 matrix work units.
//!
//! The comparison this enables: total round trip observed by the controller
//! minus the compute time reported by the worker is the transfer overhead.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use ai_cluster_protocol::cluster::{WorkResult, WorkUnit};

#[derive(Clone, Debug, PartialEq)]
pub struct CompletedWork {
    pub work_id: String,
    pub node_id: String,
    pub compute_time_us: u64,
    pub total_latency_us: u64,
    pub transfer_time_us: u64,
    pub checksum: f64,
}

#[derive(Debug, Clone)]
struct Dispatched {
    node_id: String,
    dispatched_at_unix_ms: u64,
    /// Kept so a unit can be handed back to the queue when its node leaves or
    /// is believed dead. Without this the unit would be stranded forever.
    unit: WorkUnit,
}

#[derive(Debug, Default)]
struct Inner {
    queue: VecDeque<WorkUnit>,
    dispatched: HashMap<String, Dispatched>,
    completed: Vec<CompletedWork>,
}

/// In-memory dispatch queue. No persistence: a controller restart loses pending
/// work, which is acceptable until the storage phase.
#[derive(Debug, Default)]
pub struct WorkStore {
    inner: Mutex<Inner>,
    sequence: AtomicU64,
}

impl WorkStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Monotonic id for controller-generated work units.
    pub fn next_work_id(&self) -> String {
        format!("w-{}", self.sequence.fetch_add(1, Ordering::Relaxed))
    }

    pub fn enqueue(&self, units: impl IntoIterator<Item = WorkUnit>) -> usize {
        let mut inner = self.lock();
        let mut count = 0;
        for unit in units {
            inner.queue.push_back(unit);
            count += 1;
        }
        count
    }

    /// Hands out at most one unit per call so a fast node cannot drain the queue
    /// before its peers get a chance.
    ///
    /// ponytail: the unit is cloned into the dispatch table so it can be requeued
    /// if the node leaves; that doubles payload memory while in flight. Move to an
    /// id-only table if unit payloads stop fitting in RAM.
    pub fn claim(&self, node_id: &str, now_unix_ms: u64) -> Option<WorkUnit> {
        let mut inner = self.lock();
        let unit = inner.queue.pop_front()?;
        inner.dispatched.insert(
            unit.work_id.clone(),
            Dispatched {
                node_id: node_id.to_string(),
                dispatched_at_unix_ms: now_unix_ms,
                unit: unit.clone(),
            },
        );
        Some(unit)
    }

    /// Returns the record only when the unit was dispatched to the same node.
    ///
    /// ponytail: latency derives from millisecond timestamps, so total/transfer
    /// resolution is ~1ms; fine while the workload runs for hundreds of ms.
    pub fn complete(&self, result: WorkResult, now_unix_ms: u64) -> Option<CompletedWork> {
        let mut inner = self.lock();
        let dispatched = inner.dispatched.get(&result.work_id)?;
        // A result must come from the node the unit was handed to.
        if dispatched.node_id != result.node_id {
            return None;
        }
        let dispatched = inner.dispatched.remove(&result.work_id)?;
        let total_latency_us = now_unix_ms
            .saturating_sub(dispatched.dispatched_at_unix_ms)
            .saturating_mul(1_000);
        let completed = CompletedWork {
            work_id: result.work_id.clone(),
            node_id: result.node_id.clone(),
            compute_time_us: result.compute_time_us,
            total_latency_us,
            transfer_time_us: total_latency_us.saturating_sub(result.compute_time_us),
            checksum: result.checksum,
        };
        inner.completed.push(completed.clone());
        Some(completed)
    }

    pub fn completed(&self) -> Vec<CompletedWork> {
        self.lock().completed.clone()
    }

    /// Queued plus in-flight units.
    pub fn pending(&self) -> usize {
        let inner = self.lock();
        inner.queue.len() + inner.dispatched.len()
    }

    /// Hands in-flight units belonging to `node_id` back to the queue and reports
    /// how many moved.
    ///
    /// Called when a node leaves or is declared offline, so the work it held is
    /// offered to the remaining nodes instead of being stranded. A late result
    /// from the old node is rejected because its dispatch record is gone.
    pub fn requeue_for_node(&self, node_id: &str) -> usize {
        let mut inner = self.lock();
        let stranded: Vec<String> = inner
            .dispatched
            .iter()
            .filter(|(_, record)| record.node_id == node_id)
            .map(|(work_id, _)| work_id.clone())
            .collect();
        let mut requeued = 0;
        for work_id in stranded {
            if let Some(record) = inner.dispatched.remove(&work_id) {
                inner.queue.push_back(record.unit);
                requeued += 1;
            }
        }
        requeued
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(work_id: &str) -> WorkUnit {
        WorkUnit {
            work_id: work_id.to_string(),
            rows: 2,
            shared: 2,
            cols: 2,
            matrix_a: vec![1.0, 2.0, 3.0, 4.0],
            matrix_b: vec![5.0, 6.0, 7.0, 8.0],
        }
    }

    fn result(work_id: &str, compute_time_us: u64) -> WorkResult {
        WorkResult {
            work_id: work_id.to_string(),
            node_id: "node-a".to_string(),
            rows: 2,
            cols: 2,
            matrix_c: vec![19.0, 22.0, 43.0, 50.0],
            checksum: 134.0,
            compute_time_us,
        }
    }

    #[test]
    fn enqueue_claim_complete_round_trip() {
        let store = WorkStore::new();
        assert_eq!(store.pending(), 0);
        assert!(store.claim("node-a", 1_000).is_none());

        assert_eq!(store.enqueue([unit("w1"), unit("w2")]), 2);
        assert_eq!(store.pending(), 2);

        let claimed = store.claim("node-a", 1_000).expect("queue is not empty");
        assert_eq!(claimed.work_id, "w1");
        // One queued plus one in flight still counts as two outstanding.
        assert_eq!(store.pending(), 2);

        let completed = store
            .complete(result("w1", 250_000), 1_500)
            .expect("w1 was dispatched");
        assert_eq!(completed.total_latency_us, 500_000);
        assert_eq!(completed.compute_time_us, 250_000);
        assert_eq!(completed.transfer_time_us, 250_000);
        assert_eq!(completed.node_id, "node-a");
        assert_eq!(store.completed().len(), 1);
        assert_eq!(store.pending(), 1);
    }

    #[test]
    fn completing_undispatched_work_is_rejected() {
        let store = WorkStore::new();
        assert!(store.complete(result("ghost", 1), 1_000).is_none());
        assert!(store.completed().is_empty());
    }

    #[test]
    fn result_from_a_different_node_is_rejected_and_kept_in_flight() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        store.claim("node-a", 1_000);

        // "node-a" is the dispatchee, so "node-b" cannot complete its work.
        let mut wrong = result("w1", 1_000);
        wrong.node_id = "node-b".to_string();
        assert!(store.complete(wrong, 1_500).is_none());
        assert!(store.completed().is_empty());
        assert_eq!(store.pending(), 1);

        // The real owner can still complete it.
        assert!(store.complete(result("w1", 1_000), 2_000).is_some());
        assert_eq!(store.pending(), 0);
        assert_eq!(store.completed().len(), 1);
    }

    #[test]
    fn transfer_time_never_underflows() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        store.claim("node-a", 1_000);
        // Worker reports more compute than the controller observed.
        let completed = store
            .complete(result("w1", 9_000_000), 1_100)
            .expect("w1 was dispatched");
        assert_eq!(completed.total_latency_us, 100_000);
        assert_eq!(completed.transfer_time_us, 0);
    }

    #[test]
    fn requeue_for_node_returns_in_flight_units_to_the_queue() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        assert_eq!(store.claim("node-a", 1_000).expect("w1").work_id, "w1");
        assert_eq!(store.pending(), 1);

        // node-a leaves: the unit goes back to the queue and is still outstanding.
        assert_eq!(store.requeue_for_node("node-a"), 1);
        assert_eq!(store.pending(), 1);

        // Its dispatch record is gone, so a late result from node-a is refused.
        assert!(store.complete(result("w1", 1_000), 2_000).is_none());
        assert!(store.completed().is_empty());

        // Another node picks the same unit up and finishes it.
        let taken = store.claim("node-b", 2_000).expect("requeued unit");
        assert_eq!(taken.work_id, "w1");
        assert_eq!(taken.matrix_a, vec![1.0, 2.0, 3.0, 4.0]);
        let mut done = result("w1", 1_000);
        done.node_id = "node-b".to_string();
        assert!(store.complete(done, 3_000).is_some());
        assert_eq!(store.completed().len(), 1);
        assert_eq!(store.pending(), 0);
    }

    #[test]
    fn requeue_leaves_other_nodes_units_alone() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), unit("w2")]);
        store.claim("node-a", 1_000);
        store.claim("node-b", 1_000);
        assert_eq!(store.pending(), 2);

        assert_eq!(store.requeue_for_node("node-c"), 0);
        assert_eq!(store.pending(), 2);

        // Only node-b's unit comes back when node-b leaves.
        assert_eq!(store.requeue_for_node("node-b"), 1);
        assert_eq!(store.pending(), 2);
        // node-a's unit is still owned by node-a and still completable.
        assert!(store.complete(result("w1", 1_000), 2_000).is_some());
    }
}
