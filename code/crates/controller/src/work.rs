//! Dispatch queue for Phase 0 matrix work units.
//!
//! The comparison this enables: total round trip observed by the controller
//! minus the compute time reported by the worker is the transfer overhead.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use ai_cluster_protocol::cluster::{
    CapabilityReport, HealthReport, WorkOutput, WorkResult, WorkSnapshot, WorkUnit,
};
use prost::Message;

use crate::placement::{Candidate, placement_cost};

/// Total dispatch attempts allowed per unit: the first try plus two reassignments.
///
/// A unit whose worker keeps dying is handed back to the queue each time. Without a
/// cap that loop never ends: the unit bounces between nodes and `pending()` never
/// reaches zero, so a client polling for completion waits until its own deadline.
/// Past the cap the unit is failed, and so are the units that depend on it.
const MAX_ATTEMPTS: u32 = 3;

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
    /// Output matrix of every completed unit, keyed by work id. A unit's bound
    /// inputs are read from here at dispatch, so this is the controller-side
    /// holding area for intermediate results.
    ///
    /// ponytail: outputs are kept for the process lifetime, so memory grows with
    /// completed work. Drop an output once its dependents are all claimed, or cap
    /// the history, when long-running controllers start to matter.
    outputs: HashMap<String, Vec<f64>>,
    /// Dispatch attempts started per unit, keyed by work id. Bumped in `claim`,
    /// read when a unit is handed back, dropped once the unit is settled.
    attempts: HashMap<String, u32>,
    /// Units that will never complete: their retry budget ran out. Their
    /// dependents are failed with them so the graph terminates.
    failed: HashSet<String>,
}

/// What a requeue pass did. A unit either goes back to the queue for another
/// attempt or, once its budget is spent, is failed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RequeueOutcome {
    pub requeued: usize,
    pub failed: usize,
}

/// Dispatch queue for work units. Optionally backed by a snapshot file, so a
/// controller restart does not lose queued work.
#[derive(Debug, Default)]
pub struct WorkStore {
    inner: Mutex<Inner>,
    sequence: AtomicU64,
    /// Where the snapshot is written. `None` means memory only, which is what the
    /// tests and the benchmark runs use.
    path: Option<PathBuf>,
}

/// A unit may be dispatched when every dependency has completed and every bound
/// input is available. The binding check is defensive: controller-built graphs
/// always bind to declared dependencies, but a unit bound to an id that never
/// completes would otherwise be dispatched with an empty payload.
fn ready(unit: &WorkUnit, outputs: &HashMap<String, Vec<f64>>) -> bool {
    let deps_done = unit.depends_on.iter().all(|dep| outputs.contains_key(dep));
    let inputs_available = [&unit.matrix_a_from, &unit.matrix_b_from]
        .iter()
        .all(|src| src.is_empty() || outputs.contains_key(*src));
    deps_done && inputs_available
}

/// Fails `work_id` and every queued unit that depends on it, directly or through
/// another failed unit. Returns how many units were failed.
///
/// A unit whose source failed can never become ready: readiness waits for the
/// source's *output*, and a failed unit never produces one. Left alone, such a unit
/// would sit in the queue forever and `pending()` would never reach zero. Failing it
/// too makes the graph terminate with an answer instead of a hang.
///
/// Dependents are always still queued when this runs: `ready` withholds a unit until
/// its dependencies have produced output, so nothing downstream of a failure can be
/// in flight yet.
fn fail_cascade(inner: &mut Inner, work_id: &str) -> usize {
    let mut failed = 0;
    let mut pending = vec![work_id.to_string()];
    while let Some(id) = pending.pop() {
        if !inner.failed.insert(id.clone()) {
            continue;
        }
        inner.attempts.remove(&id);
        failed += 1;
        let mut survivors = VecDeque::with_capacity(inner.queue.len());
        for unit in inner.queue.drain(..) {
            if unit.depends_on.iter().any(|dep| dep == &id)
                || unit.matrix_a_from == id
                || unit.matrix_b_from == id
            {
                pending.push(unit.work_id);
            } else {
                survivors.push_back(unit);
            }
        }
        inner.queue = survivors;
    }
    failed
}

/// Removes the unit at `index` and records the dispatch against `node_id`, with
/// any bound inputs substituted from their sources' outputs. Shared by `claim`
/// and `claim_placed` so both dispatch the same way.
fn take_locked(inner: &mut Inner, index: usize, node_id: &str, now_unix_ms: u64) -> WorkUnit {
    let mut unit = inner
        .queue
        .remove(index)
        .expect("index comes from a successful position()");
    // Substitute dependency outputs for bound inputs before dispatch.
    if let Some(data) = inner.outputs.get(&unit.matrix_a_from) {
        unit.matrix_a = data.clone();
    }
    if let Some(data) = inner.outputs.get(&unit.matrix_b_from) {
        unit.matrix_b = data.clone();
    }
    // Counting the attempt here rather than at requeue time keeps the number
    // meaningful for a unit that is claimed and never handed back.
    *inner.attempts.entry(unit.work_id.clone()).or_insert(0) += 1;
    inner.dispatched.insert(
        unit.work_id.clone(),
        Dispatched {
            node_id: node_id.to_string(),
            dispatched_at_unix_ms: now_unix_ms,
            unit: unit.clone(),
        },
    );
    unit
}

impl WorkStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a store backed by the snapshot at `path`, loading it when one exists.
    ///
    /// A missing file is not an error: a controller's first run has nothing to load.
    /// An unreadable one is reported and treated as empty rather than taking the
    /// controller down — a corrupt snapshot must not stop the cluster taking work.
    pub fn with_state(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let store = Self {
            path: Some(path.clone()),
            ..Self::default()
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return store,
            Err(err) => {
                println!(
                    "estado {} ilegível ({err}); iniciando vazio",
                    path.display()
                );
                return store;
            }
        };
        match WorkSnapshot::decode(bytes.as_slice()) {
            Ok(snapshot) => {
                let pending = {
                    let mut inner = store.lock();
                    inner.queue = snapshot.pending.into_iter().collect();
                    inner.outputs = snapshot
                        .outputs
                        .into_iter()
                        .map(|output| (output.work_id, output.matrix))
                        .collect();
                    inner.attempts = snapshot.attempts.into_iter().collect();
                    inner.failed = snapshot.failed.into_iter().collect();
                    inner.queue.len()
                };
                store.sequence.store(snapshot.sequence, Ordering::Relaxed);
                println!(
                    "estado carregado de {}: {pending} unidade(s) pendente(s)",
                    path.display()
                );
            }
            Err(err) => println!(
                "estado {} ilegível ({err}); iniciando vazio",
                path.display()
            ),
        }
        store
    }

    /// Writes the whole store to the snapshot path, when it has one.
    ///
    /// Called while the caller still holds the lock, so the snapshot is a consistent
    /// view rather than a mix of before and after. Written to a sibling file first and
    /// renamed into place: a crash mid-write leaves the previous snapshot intact
    /// instead of a truncated one. A failed write is reported, never fatal — the
    /// in-memory state stays authoritative for this run.
    ///
    /// Deliberately not called from `claim`: a unit that is in flight when the
    /// controller dies is still in the previous snapshot's `pending`, so it comes back
    /// on reload without a save per poll.
    ///
    /// ponytail: the whole state is rewritten on every mutation, so the cost grows
    /// with queue size. Fine at benchmark scale; move to an append-only log or a
    /// periodic snapshot when the queue gets large or mutations get frequent.
    fn save_locked(&self, inner: &Inner) {
        let Some(path) = &self.path else {
            return;
        };
        let snapshot = WorkSnapshot {
            pending: inner
                .queue
                .iter()
                .cloned()
                .chain(inner.dispatched.values().map(|record| record.unit.clone()))
                .collect(),
            outputs: inner
                .outputs
                .iter()
                .map(|(work_id, matrix)| WorkOutput {
                    work_id: work_id.clone(),
                    matrix: matrix.clone(),
                })
                .collect(),
            attempts: inner.attempts.clone(),
            failed: inner.failed.iter().cloned().collect(),
            sequence: self.sequence.load(Ordering::Relaxed),
        };
        let temp = path.with_extension("tmp");
        if let Err(err) = std::fs::write(&temp, snapshot.encode_to_vec())
            .and_then(|()| std::fs::rename(&temp, path))
        {
            println!("salvar estado em {} falhou: {err}", path.display());
        }
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
        self.save_locked(&inner);
        count
    }

    /// Hands out the first unit whose dependencies have all completed and whose
    /// minimum-threads requirement the asking node satisfies, with any bound inputs
    /// substituted from the outputs of those dependencies.
    ///
    /// This is the ungated path: the first eligible unit goes to whoever asks
    /// first. `claim_placed` is the placement-aware version the controller uses.
    ///
    /// ponytail: a unit whose requirement no surviving node meets stays pending.
    /// That is the same "cluster cannot run this" case as an empty cluster, so no
    /// separate handling here. The scan is O(n) in queue length — fine at benchmark
    /// scale, split into blocked and ready queues when graphs reach thousands of
    /// units. The unit is cloned into the dispatch table so it can be requeued if
    /// the node leaves; move to an id-only table if unit payloads stop fitting in RAM.
    pub fn claim(&self, node_id: &str, cpu_threads: u32, now_unix_ms: u64) -> Option<WorkUnit> {
        let mut inner = self.lock();
        let index = {
            let outputs = &inner.outputs;
            inner
                .queue
                .iter()
                .position(|unit| ready(unit, outputs) && unit.min_cpu_threads <= cpu_threads)?
        };
        Some(take_locked(&mut inner, index, node_id, now_unix_ms))
    }

    /// Hands the asking node the first unit it could take, but only when that node
    /// is the cheapest **idle** candidate for it. A node that is not the best fit
    /// gets nothing back and polls again, so the unit waits for the node that fits
    /// it better instead of going to whoever asked first. That is what turns
    /// dispatch from first-come-first-served into a placement decision.
    ///
    /// Only idle candidates are ranked. A node with a unit already in flight is not
    /// polling, so it cannot take this unit now and must not hold it up. Ties are
    /// broken by node id, so two equally good nodes cannot both yield to each other
    /// and leave the unit unclaimed.
    ///
    /// ponytail: a registered but idle node that stops polling can hold a unit
    /// indefinitely — it keeps heartbeating, so it is never declared offline. The
    /// window is small because an idle node polls every 200ms; revisit with a
    /// poll-age check or a refusal counter if a stuck node becomes a real case.
    pub fn claim_placed(
        &self,
        node_id: &str,
        cpu_threads: u32,
        now_unix_ms: u64,
        candidates: &[(String, CapabilityReport, HealthReport)],
    ) -> Option<WorkUnit> {
        let mut inner = self.lock();
        let index = {
            let outputs = &inner.outputs;
            inner
                .queue
                .iter()
                .position(|unit| ready(unit, outputs) && unit.min_cpu_threads <= cpu_threads)?
        };
        let best = {
            let unit = &inner.queue[index];
            let mut best: Option<(&str, f64)> = None;
            for (id, capability, health) in candidates {
                let in_flight = inner
                    .dispatched
                    .values()
                    .filter(|record| record.node_id.as_str() == id.as_str())
                    .count() as u32;
                if in_flight > 0 || unit.min_cpu_threads > capability.cpu_threads {
                    continue;
                }
                let cost = placement_cost(
                    unit,
                    &Candidate {
                        capability,
                        health,
                        in_flight,
                    },
                );
                let better = match best {
                    None => true,
                    Some((best_id, best_cost)) => {
                        cost < best_cost || (cost == best_cost && id.as_str() < best_id)
                    }
                };
                if better {
                    best = Some((id.as_str(), cost));
                }
            }
            best.map(|(id, _)| id.to_string())
        };
        // The asking node only takes the unit when it is the cheapest idle fit.
        // Otherwise it yields, and the unit waits for the node that fits it better.
        if best.as_deref() != Some(node_id) {
            return None;
        }
        Some(take_locked(&mut inner, index, node_id, now_unix_ms))
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
        // Recorded so units that depend on this one become claimable, and so a
        // dependent unit can read this unit's output as its own input.
        inner
            .outputs
            .insert(result.work_id.clone(), result.matrix_c);
        // The unit is settled, so its retry budget no longer matters.
        inner.attempts.remove(&result.work_id);
        self.save_locked(&inner);
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

    /// Queued units that cannot be dispatched yet: either a dependency is still
    /// incomplete or a bound input is not available.
    pub fn blocked(&self) -> usize {
        let inner = self.lock();
        inner
            .queue
            .iter()
            .filter(|unit| !ready(unit, &inner.outputs))
            .count()
    }

    /// Hands in-flight units belonging to `node_id` back to the queue and reports
    /// how many moved.
    ///
    /// Called when a node leaves or is declared offline, so the work it held is
    /// offered to the remaining nodes instead of being stranded. A late result
    /// from the old node is rejected because its dispatch record is gone.
    ///
    /// A unit that has already used up its dispatch attempts is not handed back:
    /// it is failed, along with everything downstream of it. That is what keeps a
    /// node that keeps dying from holding the graph open forever.
    pub fn requeue_for_node(&self, node_id: &str) -> RequeueOutcome {
        let mut inner = self.lock();
        let stranded: Vec<String> = inner
            .dispatched
            .iter()
            .filter(|(_, record)| record.node_id == node_id)
            .map(|(work_id, _)| work_id.clone())
            .collect();
        let mut outcome = RequeueOutcome::default();
        for work_id in stranded {
            let Some(record) = inner.dispatched.remove(&work_id) else {
                continue;
            };
            if inner.attempts.get(&work_id).copied().unwrap_or(0) >= MAX_ATTEMPTS {
                outcome.failed += fail_cascade(&mut inner, &work_id);
            } else {
                inner.queue.push_back(record.unit);
                outcome.requeued += 1;
            }
        }
        self.save_locked(&inner);
        outcome
    }

    /// Ids of units that will never complete, in stable order. Includes the units
    /// failed for spending their retry budget and the dependents failed with them.
    pub fn failed_units(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.lock().failed.iter().cloned().collect();
        ids.sort();
        ids
    }

    pub fn failed(&self) -> usize {
        self.lock().failed.len()
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
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
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
        assert!(store.claim("node-a", 8, 1_000).is_none());

        assert_eq!(store.enqueue([unit("w1"), unit("w2")]), 2);
        assert_eq!(store.pending(), 2);

        let claimed = store.claim("node-a", 8, 1_000).expect("queue is not empty");
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
        store.claim("node-a", 8, 1_000);

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
        store.claim("node-a", 8, 1_000);
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
        assert_eq!(store.claim("node-a", 8, 1_000).expect("w1").work_id, "w1");
        assert_eq!(store.pending(), 1);

        // node-a leaves: the unit goes back to the queue and is still outstanding.
        assert_eq!(store.requeue_for_node("node-a").requeued, 1);
        assert_eq!(store.pending(), 1);

        // Its dispatch record is gone, so a late result from node-a is refused.
        assert!(store.complete(result("w1", 1_000), 2_000).is_none());
        assert!(store.completed().is_empty());

        // Another node picks the same unit up and finishes it.
        let taken = store.claim("node-b", 8, 2_000).expect("requeued unit");
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
        store.claim("node-a", 8, 1_000);
        store.claim("node-b", 8, 1_000);
        assert_eq!(store.pending(), 2);

        assert_eq!(store.requeue_for_node("node-c").requeued, 0);
        assert_eq!(store.pending(), 2);

        // Only node-b's unit comes back when node-b leaves.
        assert_eq!(store.requeue_for_node("node-b").requeued, 1);
        assert_eq!(store.pending(), 2);
        // node-a's unit is still owned by node-a and still completable.
        assert!(store.complete(result("w1", 1_000), 2_000).is_some());
    }

    fn dependent(work_id: &str, depends_on: &[&str]) -> WorkUnit {
        let mut unit = unit(work_id);
        unit.depends_on = depends_on.iter().map(|d| d.to_string()).collect();
        unit
    }

    #[test]
    fn blocked_units_are_not_claimed_until_dependencies_complete() {
        let store = WorkStore::new();
        assert_eq!(store.enqueue([unit("w1"), dependent("w2", &["w1"])]), 2);
        assert_eq!(store.pending(), 2);
        assert_eq!(store.blocked(), 1);

        // w2 is behind w1, so claiming in order yields w1 first.
        let first = store.claim("node-a", 8, 1_000).expect("w1 is ready");
        assert_eq!(first.work_id, "w1");

        // w2 stays blocked while its dependency is still in flight.
        assert!(store.claim("node-b", 8, 1_000).is_none());
        assert_eq!(store.blocked(), 1);

        store.complete(result("w1", 1_000), 2_000).expect("w1 done");
        assert_eq!(store.blocked(), 0);

        // With w1 complete, w2 becomes claimable.
        let second = store.claim("node-b", 8, 2_000).expect("w2 is ready");
        assert_eq!(second.work_id, "w2");
        let mut done = result("w2", 1_000);
        done.node_id = "node-b".to_string();
        store.complete(done, 3_000).expect("w2 done");
        assert_eq!(store.pending(), 0);
        assert_eq!(store.completed().len(), 2);
    }

    #[test]
    fn claim_skips_blocked_units_to_reach_a_ready_one() {
        let store = WorkStore::new();
        // The blocked unit sits at the head; a ready unit sits behind it.
        assert_eq!(store.enqueue([dependent("w2", &["w1"]), unit("w1")]), 2);
        assert_eq!(store.blocked(), 1);

        // The head is blocked, so the scan must find w1 further down the queue.
        let claimed = store.claim("node-a", 8, 1_000).expect("w1 is ready");
        assert_eq!(claimed.work_id, "w1");
    }

    #[test]
    fn requeue_keeps_dependencies_intact() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), dependent("w2", &["w1"])]);
        let first = store.claim("node-a", 8, 1_000).expect("w1 is ready");
        assert_eq!(first.work_id, "w1");

        // node-a leaves with w1 in flight; w1 returns to the queue still ready.
        assert_eq!(store.requeue_for_node("node-a").requeued, 1);
        let again = store.claim("node-b", 8, 2_000).expect("w1 requeued");
        assert_eq!(again.work_id, "w1");
        // w2 is still blocked because w1 has not completed.
        assert_eq!(store.blocked(), 1);
    }

    #[test]
    fn dependent_unit_receives_its_dependency_output_as_input() {
        let store = WorkStore::new();
        let mut consumer = dependent("w2", &["w1"]);
        consumer.matrix_a_from = "w1".to_string();
        consumer.matrix_b_from = "w1".to_string();
        // Sourced inputs arrive at dispatch, so the payload is empty for now.
        consumer.matrix_a = Vec::new();
        consumer.matrix_b = Vec::new();
        store.enqueue([unit("w1"), consumer]);

        // w1 runs first and produces a known output.
        let first = store.claim("node-a", 8, 1_000).expect("w1 is ready");
        assert_eq!(first.work_id, "w1");
        let mut done = result("w1", 1_000);
        done.matrix_c = vec![7.0, 8.0, 9.0, 10.0];
        store.complete(done, 2_000).expect("w1 done");

        // w2 is dispatched with w1's output substituted into both inputs.
        let second = store.claim("node-b", 8, 2_000).expect("w2 is ready");
        assert_eq!(second.work_id, "w2");
        assert_eq!(second.matrix_a, vec![7.0, 8.0, 9.0, 10.0]);
        assert_eq!(second.matrix_b, vec![7.0, 8.0, 9.0, 10.0]);
    }

    #[test]
    fn a_binding_to_an_unknown_work_id_keeps_the_unit_blocked() {
        let store = WorkStore::new();
        let mut broken = dependent("w2", &[]);
        broken.matrix_a_from = "ghost".to_string();
        store.enqueue([broken]);
        // Never dispatchable: the bound source never completes.
        assert!(store.claim("node-a", 8, 1_000).is_none());
        assert_eq!(store.blocked(), 1);
    }

    #[test]
    fn a_unit_is_failed_once_its_retry_budget_is_spent() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);

        // The first attempts are handed back for another try.
        for _ in 0..MAX_ATTEMPTS - 1 {
            assert_eq!(store.claim("node-a", 8, 1_000).expect("w1").work_id, "w1");
            let outcome = store.requeue_for_node("node-a");
            assert_eq!(outcome.requeued, 1);
            assert_eq!(outcome.failed, 0);
            assert_eq!(store.pending(), 1);
        }

        // The last attempt spends the budget: the unit is not handed back again.
        assert_eq!(store.claim("node-a", 8, 2_000).expect("w1").work_id, "w1");
        let outcome = store.requeue_for_node("node-a");
        assert_eq!(outcome.requeued, 0);
        assert_eq!(outcome.failed, 1);
        assert_eq!(store.failed_units(), vec!["w1".to_string()]);
        // The graph is no longer outstanding, so a polling client terminates.
        assert_eq!(store.pending(), 0);
        assert!(store.claim("node-b", 8, 3_000).is_none());
        // Its dispatch record is gone, so a late result from the failed unit is refused.
        assert!(store.complete(result("w1", 1_000), 3_500).is_none());
        // A second pass over the same node finds nothing left to move.
        assert_eq!(store.requeue_for_node("node-a"), RequeueOutcome::default());
        assert_eq!(store.failed(), 1);
    }

    #[test]
    fn failing_a_unit_fails_its_dependents_with_it() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), dependent("w2", &["w1"])]);

        // w2 stays blocked behind w1 for the whole retry loop, so w1 is always next.
        for _ in 0..MAX_ATTEMPTS {
            assert_eq!(store.claim("node-a", 8, 1_000).expect("w1").work_id, "w1");
            store.requeue_for_node("node-a");
        }

        // w2 can never run now, so it is failed rather than left blocking the queue.
        assert_eq!(
            store.failed_units(),
            vec!["w1".to_string(), "w2".to_string()]
        );
        assert_eq!(store.pending(), 0);
        assert_eq!(store.blocked(), 0);
        assert!(store.claim("node-b", 8, 2_000).is_none());
    }

    #[test]
    fn a_failed_branch_does_not_fail_unrelated_work() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), dependent("w2", &["w1"])]);
        for _ in 0..MAX_ATTEMPTS {
            assert_eq!(store.claim("node-a", 8, 1_000).expect("w1").work_id, "w1");
            store.requeue_for_node("node-a");
        }
        assert_eq!(
            store.failed_units(),
            vec!["w1".to_string(), "w2".to_string()]
        );

        // The store still accepts and runs work after a branch failed.
        store.enqueue([unit("w3")]);
        assert_eq!(store.pending(), 1);
        assert_eq!(store.claim("node-b", 8, 2_000).expect("w3").work_id, "w3");
    }

    #[test]
    fn a_weak_node_is_skipped_for_a_unit_that_requires_more_threads() {
        let store = WorkStore::new();
        let mut heavy = unit("w1");
        heavy.min_cpu_threads = 4;
        let light = unit("w2");
        store.enqueue([heavy, light]);

        // The weak node polls first but only w2 fits, so w2 is handed over and w1
        // is left for a node that can run it. FCFS would have given it w1.
        assert_eq!(
            store.claim("node-weak", 2, 1_000).expect("w2 fits").work_id,
            "w2"
        );

        // A capable node gets the unit the weak node skipped.
        assert_eq!(
            store
                .claim("node-strong", 8, 2_000)
                .expect("w1 fits")
                .work_id,
            "w1"
        );

        // With only a weak node left, the heavy unit stays pending instead of
        // being handed to a node that cannot run it. A capable node still gets it,
        // so the unit is waiting, not lost.
        let mut another = unit("w3");
        another.min_cpu_threads = 4;
        store.enqueue([another]);
        assert!(store.claim("node-weak", 2, 3_000).is_none());
        assert_eq!(
            store
                .claim("node-strong", 8, 4_000)
                .expect("w3 fits")
                .work_id,
            "w3"
        );
    }

    fn capability(cpu_threads: u32, compute_score: f64) -> CapabilityReport {
        CapabilityReport {
            cpu_threads,
            compute_score,
            ..Default::default()
        }
    }

    fn healthy() -> HealthReport {
        HealthReport {
            health_score: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn placement_gives_a_unit_to_the_cheapest_idle_node() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        let candidates = [
            ("node-weak".to_string(), capability(2, 0.0), healthy()),
            ("node-strong".to_string(), capability(8, 0.0), healthy()),
        ];

        // The weak node polls first, but the strong node is idle and cheaper, so the
        // weak node yields. First-come-first-served would have handed it w1.
        assert!(
            store
                .claim_placed("node-weak", 2, 1_000, &candidates)
                .is_none()
        );
        // The unit is still queued, not lost, and the node that fits it takes it.
        assert_eq!(store.pending(), 1);
        assert_eq!(
            store
                .claim_placed("node-strong", 8, 1_000, &candidates)
                .expect("strong node fits w1")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn a_busy_cheaper_node_does_not_hold_up_an_idle_node() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), unit("w2")]);
        let candidates = [
            ("node-weak".to_string(), capability(2, 0.0), healthy()),
            ("node-strong".to_string(), capability(8, 0.0), healthy()),
        ];

        // With both idle the strong node takes the first unit.
        assert_eq!(
            store
                .claim_placed("node-strong", 8, 1_000, &candidates)
                .expect("strong fits w1")
                .work_id,
            "w1"
        );
        // Now strong is busy, so the weak node is the only idle candidate and takes
        // the next unit instead of leaving it stranded behind a node that is not
        // polling.
        assert_eq!(
            store
                .claim_placed("node-weak", 2, 2_000, &candidates)
                .expect("weak is the only idle candidate")
                .work_id,
            "w2"
        );
    }

    #[test]
    fn the_only_idle_candidate_still_takes_work() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        let candidates = [("node-weak".to_string(), capability(2, 0.0), healthy())];

        // No cheaper node is present, so the weak node is the best fit and takes it.
        assert_eq!(
            store
                .claim_placed("node-weak", 2, 1_000, &candidates)
                .expect("weak is the best fit")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn equally_good_nodes_do_not_both_yield() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        let candidates = [
            ("node-a".to_string(), capability(4, 0.0), healthy()),
            ("node-b".to_string(), capability(4, 0.0), healthy()),
        ];

        // The costs tie, so the tie breaks by id: node-a wins the unit and node-b
        // yields. If both yielded the unit would never be claimed.
        assert!(
            store
                .claim_placed("node-b", 4, 1_000, &candidates)
                .is_none()
        );
        assert_eq!(
            store
                .claim_placed("node-a", 4, 1_000, &candidates)
                .expect("node-a wins the tie")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn a_snapshot_round_trips_pending_work_and_outputs() {
        let dir = std::env::temp_dir().join(format!("aic-work-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("state.bin");

        let store = WorkStore::with_state(&path);
        // Mint an id first so the saved sequence is non-zero and can be checked.
        assert_eq!(store.next_work_id(), "w-0");
        store.enqueue([unit("w1"), dependent("w2", &["w1"])]);
        store.claim("node-a", 8, 1_000).expect("w1 is ready");
        store
            .complete(result("w1", 250), 2_000)
            .expect("w1 was dispatched");

        // A fresh store over the same file: w2 is still queued, and its dependency's
        // output came back with it, so it is ready rather than blocked forever.
        let reloaded = WorkStore::with_state(&path);
        assert_eq!(reloaded.pending(), 1);
        assert_eq!(reloaded.blocked(), 0);
        assert_eq!(
            reloaded
                .claim("node-b", 8, 3_000)
                .expect("w2 is ready")
                .work_id,
            "w2"
        );
        // The id sequence continued instead of restarting, so nothing collides.
        assert_eq!(reloaded.next_work_id(), "w-1");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
