//! Dispatch queue for Phase 0 matrix work units.
//!
//! The comparison this enables: total round trip observed by the controller
//! minus the compute time reported by the worker is the transfer overhead.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ai_cluster_protocol::cluster::{
    CapabilityReport, HealthReport, StateReplica, TraceEntry, WorkLease, WorkOutput, WorkResult,
    WorkSnapshot, WorkUnit,
};
use prost::Message;

use crate::placement::{decide, fits, predicted_compute_us};
use crate::reservations::{Reservation, Reservations};

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
    /// The client request this unit was enqueued under (H3), copied from the unit at
    /// dispatch. Lets a trace find a request's completed units and their timings.
    pub request_id: String,
    pub compute_time_us: u64,
    pub total_latency_us: u64,
    pub transfer_time_us: u64,
    /// Compute time the cost model predicted for this unit on the node it went to (H5),
    /// captured at dispatch. Zero when the node reported no compute score. Compared
    /// against `compute_time_us`, the worker's measurement, to expose the model's error.
    pub predicted_compute_us: u64,
    pub checksum: f64,
}

#[derive(Debug, Clone)]
struct Dispatched {
    node_id: String,
    dispatched_at_unix_ms: u64,
    /// Prediction carried from claim time to completion (H5). Held here because the
    /// node's capability is only in hand at claim time, not when the result comes back.
    predicted_compute_us: u64,
    /// The cost model's prediction before the per-node correction, so completion can learn how
    /// far the model is off. Zero when unknown (a lease restored from disk).
    raw_predicted_us: u64,
    /// Kept so a unit can be handed back to the queue when its node leaves or
    /// is believed dead. Without this the unit would be stranded forever.
    unit: WorkUnit,
}

/// How many kept results the store holds (J1). A result is read once, right after its request
/// ends, so a short window is enough and bounds the memory a client can pin.
const MAX_KEPT_RESULTS: usize = 64;

/// The output of a request's final unit, held for `GetOutput` after the intermediate-output
/// sweep would have dropped it.
#[derive(Debug)]
struct KeptResult {
    request_id: String,
    rows: u32,
    cols: u32,
    data: Vec<f64>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Work ids whose output a client asked to keep (J1). Not persisted: a restart loses the
    /// result, and the client sees `found: false`.
    ///
    /// ponytail: an id whose unit fails stays here; a few bytes per failed request. Prune on
    /// failure if gateways ever churn through many failing requests.
    keep: HashSet<String>,
    results: VecDeque<KeptResult>,
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
    /// dependents are failed with them so the graph terminates. Mapped to the request
    /// that produced them, so a trace can list a request's failed units (H3).
    ///
    /// ponytail: only the ids survive a reload, so a failed unit's request id is empty
    /// after a restart. The same ceiling as completed units, which are not persisted at
    /// all; persist the correlation when traces need to outlive a restart.
    failed: HashMap<String, String>,
    /// Dispatches that went to another unit while a queued unit waited (G4 aging). Not
    /// persisted: a restart resets the age, which only delays an overtake by one span.
    passed: HashMap<String, u32>,
    /// Per node, how many times slower (or faster) than the cost model it really computes: the
    /// moving average of actual over predicted compute time (P1). Not persisted; a restart
    /// relearns it from the first few units.
    learned: HashMap<String, f64>,
}

/// Weight of the newest observation in the learned ratio.
const LEARNING_RATE: f64 = 0.3;

/// A ratio outside this band is a stall or a clock glitch, not the node's speed.
const RATIO_BAND: (f64, f64) = (0.05, 20.0);

/// The prediction after the node's learned correction. No ratio yet means the model as it is.
fn corrected(ratio: Option<f64>, raw_us: u64) -> u64 {
    ratio.map_or(raw_us, |ratio| (raw_us as f64 * ratio).round() as u64)
}

/// The ratio after one more observation: the first sets it, later ones move it by
/// `LEARNING_RATE` of the way. Nothing is learned from an unknown prediction or an empty timing.
fn learn(previous: Option<f64>, raw_us: u64, actual_us: u64) -> Option<f64> {
    if raw_us == 0 || actual_us == 0 {
        return previous;
    }
    let seen = (actual_us as f64 / raw_us as f64).clamp(RATIO_BAND.0, RATIO_BAND.1);
    Some(previous.map_or(seen, |ratio| ratio + LEARNING_RATE * (seen - ratio)))
}

/// One row of a request trace (H3). A unit only has timings once it has completed; the
/// other states report zero, so a trace shows cost where there is cost and nothing where
/// there is not, instead of a misleading zero.
fn trace_row(
    work_id: &str,
    node_id: &str,
    status: &str,
    times: Option<&CompletedWork>,
) -> TraceEntry {
    let (compute_time_us, transfer_time_us, total_latency_us, predicted_compute_us) =
        times.map_or((0, 0, 0, 0), |record| {
            (
                record.compute_time_us,
                record.transfer_time_us,
                record.total_latency_us,
                record.predicted_compute_us,
            )
        });
    TraceEntry {
        work_id: work_id.to_string(),
        node_id: node_id.to_string(),
        status: status.to_string(),
        compute_time_us,
        transfer_time_us,
        total_latency_us,
        predicted_compute_us,
    }
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
    /// Separate sequence for client request ids (H3). Kept apart from the work-id sequence
    /// so handing out a request id does not shift the work-id numbering that tests and
    /// scripts read.
    request_sequence: AtomicU64,
    /// Nós retidos para requisições futuras (G6). Lido dentro de `claim_placed`, então mora
    /// aqui junto da decisão de placement, e não no registry, que só conhece estado dos nós.
    reservations: Reservations,
    /// Where the snapshot is written. `None` means memory only, which is what the
    /// tests and the benchmark runs use.
    path: Option<PathBuf>,
    /// The snapshot exactly as last written, with the epoch that dates it (I4).
    ///
    /// Held so a heartbeat can hand a copy to a node without re-encoding the state, and so
    /// this controller can tell its own copy from one a node offers back. `None` when the
    /// store has no file: without a state path there is nothing to replicate, and nothing a
    /// copy could restore — a memory-only controller is a benchmark, not a cluster.
    ///
    /// An `Arc` because the heartbeat reads it far more often than it changes: handing the
    /// blob out clones a pointer, and only the node that is actually behind pays for a copy.
    replica: Mutex<Option<Arc<StateReplica>>>,
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

/// Drops every intermediate result no unfinished unit can read.
///
/// An output is read in exactly two places: `ready` asks whether a dependency's output is
/// there, and `take_locked` substitutes a bound input from it. Both read it on behalf of a
/// unit that has not settled — queued now, or in flight and possibly handed back by
/// `requeue_for_node` — so those are the only units that pin an output. Anything else is a
/// matrix held for nobody, and it is the bulk of what the snapshot rewrites on every save.
///
/// Run from `save_locked`, the one choke point every mutation passes through, so the
/// invariant cannot be forgotten at a call site. Also a memory bound: the map used to grow
/// with all work ever completed.
///
/// ponytail: the live set is still rewritten whole per mutation, so the checkpoint costs
/// O(work not yet done) instead of O(all work ever). A delta journal is the next step if even
/// that gets expensive; at 8 units of 256³ the snapshot went from 4.19 MB to nothing.
fn drop_unreferenced_outputs(inner: &mut Inner) {
    let mut needed: HashSet<String> = HashSet::new();
    for unit in inner
        .queue
        .iter()
        .chain(inner.dispatched.values().map(|record| &record.unit))
    {
        needed.extend(unit.depends_on.iter().cloned());
        if !unit.matrix_a_from.is_empty() {
            needed.insert(unit.matrix_a_from.clone());
        }
        if !unit.matrix_b_from.is_empty() {
            needed.insert(unit.matrix_b_from.clone());
        }
    }
    inner.outputs.retain(|work_id, _| needed.contains(work_id));
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
/// in flight yet. They inherit the head unit's request id, since a graph's units are
/// all enqueued under one request — so a trace can attribute the whole cascade to it.
fn fail_cascade(inner: &mut Inner, work_id: &str, request_id: &str) -> usize {
    let mut failed = 0;
    let mut pending = vec![work_id.to_string()];
    while let Some(id) = pending.pop() {
        if inner
            .failed
            .insert(id.clone(), request_id.to_string())
            .is_some()
        {
            continue;
        }
        inner.attempts.remove(&id);
        inner.passed.remove(&id);
        // A failed unit will never complete, so a kept result for it would wait forever (J1).
        inner.keep.remove(&id);
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

/// How many passes-over one priority class is worth (G4 aging). A unit's rank is
/// `priority * PRIORITY_SPAN + passed`, where `passed` counts the dispatches that went to
/// another unit while it waited. A low-class unit therefore overtakes a stream of higher-class
/// ones after `PRIORITY_SPAN` passes and cannot starve. With every priority equal the oldest
/// unit ranks highest (ties go to the lower index), which is the FIFO order the queue had.
const PRIORITY_SPAN: u32 = 64;

/// Indices of the units that pass `eligible`, best first: highest rank, FIFO among equal ranks.
fn ranked_positions(
    queue: &VecDeque<WorkUnit>,
    passed: &HashMap<String, u32>,
    eligible: impl Fn(&WorkUnit) -> bool,
) -> Vec<usize> {
    let mut ranked: Vec<(u64, usize)> = queue
        .iter()
        .enumerate()
        .filter(|(_, unit)| eligible(unit))
        .map(|(index, unit)| {
            let rank = u64::from(unit.priority) * u64::from(PRIORITY_SPAN)
                + u64::from(passed.get(&unit.work_id).copied().unwrap_or(0));
            (rank, index)
        })
        .collect();
    ranked.sort_by_key(|&(rank, index)| (std::cmp::Reverse(rank), index));
    ranked.into_iter().map(|(_, index)| index).collect()
}

/// Removes the unit at `index` and records the dispatch against `node_id`, with
/// any bound inputs substituted from their sources' outputs. Shared by `claim`
/// and `claim_placed` so both dispatch the same way.
fn take_locked(
    inner: &mut Inner,
    index: usize,
    node_id: &str,
    now_unix_ms: u64,
    raw_predicted_us: u64,
) -> WorkUnit {
    // P1: what this node has taught the model so far scales the prediction.
    let predicted_compute_us = corrected(inner.learned.get(node_id).copied(), raw_predicted_us);
    let mut unit = inner
        .queue
        .remove(index)
        .expect("index comes from a successful position()");
    // Every unit left behind ages, so a low class eventually outranks a stream of high ones.
    for waiting in &inner.queue {
        match inner.passed.get_mut(&waiting.work_id) {
            Some(count) => *count += 1,
            None => {
                inner.passed.insert(waiting.work_id.clone(), 1);
            }
        }
    }
    inner.passed.remove(&unit.work_id);
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
            predicted_compute_us,
            raw_predicted_us,
            unit: unit.clone(),
        },
    );
    unit
}

/// The redundant copy that sits next to the primary snapshot.
///
/// Named by appending rather than replacing the extension, so `state.bin` gets
/// `state.bin.replica` and cannot collide with a file already called `state.replica`.
fn replica_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".replica");
    PathBuf::from(name)
}

impl WorkStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a store backed by the snapshot at `path`, loading it when one exists.
    ///
    /// A missing file is not an error: a controller's first run has nothing to load. An
    /// unreadable one falls back to the redundant copy beside it before giving up, and only
    /// when neither decodes does the store start empty — a corrupt snapshot must not stop the
    /// cluster taking work. The copy is the second half of the state directory: losing one
    /// file should cost at most the mutations since the last save, not the whole queue.
    pub fn with_state(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let store = Self {
            path: Some(path.clone()),
            ..Self::default()
        };
        let mut reported = false;
        for candidate in [path.clone(), replica_path(&path)] {
            let bytes = match std::fs::read(&candidate) {
                Ok(bytes) => bytes,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    println!("estado {} ilegível ({err})", candidate.display());
                    reported = true;
                    continue;
                }
            };
            // An empty file decodes as a default snapshot, so it would pass for "nothing to do"
            // even when the queue it held is gone. Nothing here to load — try the copy instead.
            if bytes.is_empty() {
                continue;
            }
            let Ok(snapshot) = WorkSnapshot::decode(bytes.as_slice()) else {
                println!("estado {} ilegível (não decodifica)", candidate.display());
                reported = true;
                continue;
            };
            let epoch = snapshot.epoch_unix_ms;
            let (pending, dispatched) = {
                let mut inner = store.lock();
                store.install_locked(&mut inner, snapshot)
            };
            // A cópia em memória sai do arquivo que carregou, com a época que ele traz: sem
            // isso um controller que voltou de um reload pareceria época zero, e tomaria a
            // cópia de um nó por mais nova que a sua.
            *store
                .replica
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(StateReplica {
                epoch_unix_ms: epoch,
                snapshot: bytes,
            }));
            println!(
                "estado carregado de {}: {pending} pendente(s), {dispatched} em voo",
                candidate.display()
            );
            return store;
        }
        if reported {
            println!("nenhum estado carregado; iniciando vazio");
        }
        store
    }

    /// Copies a decoded snapshot into the store: queue, outputs, attempts, failed units and
    /// the in-flight leases.
    ///
    /// Shared by the file loader in `with_state` and the copy adopted from a node (I4), so a
    /// field added for one cannot be missed by the other. The in-flight units keep their owner
    /// instead of going back to the queue, so the node that survived the restart has its
    /// result accepted rather than refused; an owner that does not come back is handed back
    /// once the grace passes — see `unknown_owners`.
    fn install_locked(&self, inner: &mut Inner, snapshot: WorkSnapshot) -> (usize, usize) {
        inner.queue = snapshot.pending.into_iter().collect();
        inner.outputs = snapshot
            .outputs
            .into_iter()
            .map(|output| (output.work_id, output.matrix))
            .collect();
        inner.attempts = snapshot.attempts.into_iter().collect();
        inner.failed = snapshot
            .failed
            .into_iter()
            .map(|id| (id, String::new()))
            .collect();
        inner.dispatched = snapshot
            .dispatched
            .into_iter()
            .filter_map(|lease| {
                Some((
                    lease.work_id.clone(),
                    Dispatched {
                        node_id: lease.node_id,
                        dispatched_at_unix_ms: lease.dispatched_at_unix_ms,
                        predicted_compute_us: lease.predicted_compute_us,
                        raw_predicted_us: 0,
                        unit: lease.unit?,
                    },
                ))
            })
            .collect();
        self.sequence.store(snapshot.sequence, Ordering::Relaxed);
        (inner.queue.len(), inner.dispatched.len())
    }

    /// Writes the whole store to the snapshot path and its redundant copy, when it has one.
    ///
    /// Called while the caller still holds the lock, so the snapshot is a consistent
    /// view rather than a mix of before and after. Each file is written to a sibling
    /// first and renamed into place: a crash mid-write leaves the previous snapshot
    /// intact instead of a truncated one. A failed write is reported, never fatal — the
    /// in-memory state stays authoritative for this run. With two files a crash leaves at
    /// least one complete copy: the two may differ in age, never in validity.
    ///
    /// The in-flight units go to `dispatched` with their owner rather than back into
    /// `pending`, so a reload knows who held what instead of offering the unit to a second
    /// node while the first is still working on it.
    ///
    /// Deliberately not called from `claim`: a unit claimed but not yet saved is still in
    /// the previous snapshot's `pending`, so it comes back on reload without a save per
    /// poll. The cost is that such a unit loses its owner — the same double dispatch the
    /// old code did for every in-flight unit.
    ///
    /// Also drops the intermediate results nobody will read (`drop_unreferenced_outputs`),
    /// which is what keeps the snapshot proportional to the work still to do. It used to carry
    /// every matrix ever completed and rewrite them all on every mutation — quadratic in
    /// finished work. Measured: 8 units of 256³ left a 4.19 MB snapshot before, nothing now.
    ///
    /// ponytail: the live set is still rewritten whole on every mutation, and twice — the copy
    /// doubles the write cost to buy redundancy. Fine at benchmark scale; a delta journal or a
    /// periodic snapshot when the live set gets large or mutations get frequent.
    fn save_locked(&self, inner: &mut Inner) {
        // The checkpoint is what a restart comes back to, so it carries no matrix nobody can
        // read. Done before the early return: a memory-only store has the same ceiling, and
        // this is what keeps the map from growing with all work ever completed.
        drop_unreferenced_outputs(inner);
        let Some(path) = &self.path else {
            return;
        };
        let epoch = self.next_epoch();
        let snapshot = WorkSnapshot {
            pending: inner.queue.iter().cloned().collect(),
            outputs: inner
                .outputs
                .iter()
                .map(|(work_id, matrix)| WorkOutput {
                    work_id: work_id.clone(),
                    matrix: matrix.clone(),
                })
                .collect(),
            attempts: inner.attempts.clone(),
            failed: inner.failed.keys().cloned().collect(),
            sequence: self.sequence.load(Ordering::Relaxed),
            dispatched: inner
                .dispatched
                .iter()
                .map(|(work_id, record)| WorkLease {
                    work_id: work_id.clone(),
                    node_id: record.node_id.clone(),
                    dispatched_at_unix_ms: record.dispatched_at_unix_ms,
                    predicted_compute_us: record.predicted_compute_us,
                    unit: Some(record.unit.clone()),
                })
                .collect(),
            epoch_unix_ms: epoch,
        };
        // Encoded once, written twice: serialisation is the expensive part and both files
        // hold the same bytes. Each rename is atomic on its own, so whichever write is
        // interrupted leaves the other file complete.
        let bytes = snapshot.encode_to_vec();
        for target in [path.clone(), replica_path(path)] {
            let temp = target.with_extension("tmp");
            if let Err(err) =
                std::fs::write(&temp, &bytes).and_then(|()| std::fs::rename(&temp, &target))
            {
                println!("salvar estado em {} falhou: {err}", target.display());
            }
        }
        // The very bytes the files just got, kept so a heartbeat can hand a node a copy
        // without re-encoding the state (I4).
        *self
            .replica
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(StateReplica {
            epoch_unix_ms: epoch,
            snapshot: bytes,
        }));
    }

    /// Epoch for the next save: the clock, but never at or below the last one written.
    ///
    /// A node compares the copy it holds against this number, so a clock that jumps backwards
    /// — a VM restored from a snapshot — must not make the live state look older than a copy
    /// already in the field. Bumping past the previous value keeps the comparison meaningful
    /// under a skewed or rewound clock, which a bare timestamp does not.
    fn next_epoch(&self) -> u64 {
        let previous = self.replica_epoch();
        crate::registry::unix_ms().max(previous.saturating_add(1))
    }

    /// Epoch of the copy in memory. Zero when there is none.
    fn replica_epoch(&self) -> u64 {
        self.replica
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map_or(0, |replica| replica.epoch_unix_ms)
    }

    /// The copy of the state this controller last wrote, for a node to keep beside its own
    /// work (I4).
    ///
    /// `None` for a store with no snapshot file: without a state path there is nothing to
    /// replicate, and nothing a copy could restore. The blob is behind an `Arc` because a
    /// heartbeat asks for this on every beat and only the node that is actually behind pays
    /// for a full copy.
    pub fn state_replica(&self) -> Option<Arc<StateReplica>> {
        self.replica
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Adopts a copy of the state a node held, and returns whether it was taken.
    ///
    /// This is the promotion path (I4): a controller that lost both files has no queue, and a
    /// node that kept a copy is the only surviving source. A copy is taken only when this
    /// controller holds no work at all and the copy is newer than its own — the point is to
    /// recover a controller that lost its files, not to let a node overwrite a queue this one
    /// is already serving. A memory-only store never takes one: without a state path there is
    /// nothing to recover, and a run with no file is a benchmark, not a cluster.
    ///
    /// The adopted state is written to the local files straight away, so the copy comes home:
    /// the next restart loads normally instead of having to ask a node again.
    pub fn restore(&self, replica: &StateReplica) -> bool {
        if self.path.is_none() {
            return false;
        }
        let Ok(snapshot) = WorkSnapshot::decode(replica.snapshot.as_slice()) else {
            println!("réplica de estado oferecida por um nó não decodifica; ignorada");
            return false;
        };
        let mut inner = self.lock();
        if !inner.queue.is_empty()
            || !inner.dispatched.is_empty()
            || !inner.outputs.is_empty()
            || !inner.completed.is_empty()
        {
            return false;
        }
        if replica.epoch_unix_ms <= self.replica_epoch() {
            return false;
        }
        let (pending, dispatched) = self.install_locked(&mut inner, snapshot);
        *self
            .replica
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(replica.clone()));
        self.save_locked(&mut inner);
        println!(
            "estado restaurado da réplica de um nó: {pending} pendente(s), {dispatched} em voo, época {}",
            replica.epoch_unix_ms
        );
        true
    }

    /// Monotonic id for controller-generated work units.
    pub fn next_work_id(&self) -> String {
        format!("w-{}", self.sequence.fetch_add(1, Ordering::Relaxed))
    }

    /// Monotonic id for a client request (H3), used when the client did not supply one.
    pub fn next_request_id(&self) -> String {
        format!(
            "req-{}",
            self.request_sequence.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// Retém um nó para uma requisição futura (G6) até `ttl_ms` a partir de `now`.
    /// TTL zero usa o padrão de [`crate::reservations::DEFAULT_TTL_MS`].
    pub fn reserve(&self, node_id: &str, request_id: &str, ttl_ms: u64, now: u64) -> Reservation {
        self.reservations.reserve(node_id, request_id, ttl_ms, now)
    }

    /// Libera a reserva de um nó. False quando não havia reserva ativa.
    pub fn release_reservation(&self, node_id: &str) -> bool {
        self.reservations.release(node_id)
    }

    /// Reservas ativas em `now`, ordenadas por nó. As vencidas são descartadas na leitura.
    pub fn reservations(&self, now: u64) -> Vec<(String, Reservation)> {
        self.reservations.list(now)
    }

    pub fn enqueue(&self, units: impl IntoIterator<Item = WorkUnit>) -> usize {
        let mut inner = self.lock();
        let mut count = 0;
        for unit in units {
            inner.queue.push_back(unit);
            count += 1;
        }
        self.save_locked(&mut inner);
        count
    }

    /// Hands out the best-ranked unit (priority, then FIFO) whose dependencies have all completed and whose
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
    pub fn claim(
        &self,
        node_id: &str,
        capability: &CapabilityReport,
        now_unix_ms: u64,
    ) -> Option<WorkUnit> {
        let mut inner = self.lock();
        // G6: a node held for a request takes only that request's units, even on this
        // ungated path. The hold is a property of the node, not of the placement policy,
        // so it must survive `AI_CLUSTER_PLACEMENT=off`.
        let held = self.reservations.held_for(node_id, now_unix_ms);
        let index = {
            let outputs = &inner.outputs;
            *ranked_positions(&inner.queue, &inner.passed, |unit| {
                ready(unit, outputs)
                    && fits(unit, capability)
                    && held
                        .as_deref()
                        .is_none_or(|request| request == unit.request_id)
            })
            .first()?
        };
        let raw = predicted_compute_us(&inner.queue[index], capability);
        Some(take_locked(&mut inner, index, node_id, now_unix_ms, raw))
    }

    /// Hands the asking node the first unit it could take, but only when that node is
    /// cheap enough for it. A node that is not gets nothing back and polls again, so
    /// the unit waits for the node that fits it better instead of going to whoever
    /// asked first. That is what turns dispatch from first-come-first-served into a
    /// placement decision.
    ///
    /// Only idle candidates are ranked. A node with a unit already in flight is not
    /// polling, so it cannot take this unit now and must not hold it up.
    ///
    /// Hysteresis: the asking node yields only when another candidate is cheaper by
    /// more than `tolerance`, a fraction of that candidate's cost. Yielding is not
    /// free — the unit waits up to a poll interval for the other node to ask — so
    /// refusing to yield over a rounding-level difference trades a certain idle gap
    /// for a saving that may not exist. The band also subsumes the old id tie-break:
    /// near-equal nodes no longer refuse each other, so a unit cannot be stranded
    /// between them. A zero band means "strictly cheapest".
    ///
    /// Reservations (G6): a node held for a request is the only home for that request's
    /// units, and it takes no other request's units. The hold outranks the cost ranking —
    /// the client held the node precisely so the work lands there — so a reserved unit
    /// yields to the held node instead of going to a cheaper stranger. If the held node
    /// never polls, the TTL releases the hold and normal placement resumes.
    ///
    /// ponytail: a registered but idle node that stops polling can hold a unit
    /// indefinitely — it keeps heartbeating, so it is never declared offline. The
    /// window is small because an idle node polls every 200ms; revisit with a
    /// poll-age check or a refusal counter if a stuck node becomes a real case.
    pub fn claim_placed(
        &self,
        node_id: &str,
        capability: &CapabilityReport,
        now_unix_ms: u64,
        candidates: &[(String, CapabilityReport, HealthReport)],
        tolerance: f64,
    ) -> Option<WorkUnit> {
        let mut inner = self.lock();
        // G6: a node held for a request takes only that request's units.
        let held = self.reservations.held_for(node_id, now_unix_ms);
        let ranked = {
            let outputs = &inner.outputs;
            ranked_positions(&inner.queue, &inner.passed, |unit| {
                ready(unit, outputs)
                    && fits(unit, capability)
                    && held
                        .as_deref()
                        .is_none_or(|request| request == unit.request_id.as_str())
            })
        };
        // Best rank first, but a unit this node is not the right home for (a better node, or
        // a hold for another node) must not block a lower-ranked unit it would take.
        for index in ranked {
            let raw = predicted_compute_us(&inner.queue[index], capability);
            let takes = decide(
                &inner.queue[index],
                node_id,
                candidates,
                |id| {
                    inner
                        .dispatched
                        .values()
                        .filter(|record| record.node_id.as_str() == id)
                        .count() as u32
                },
                |id| self.reservations.held_for(id, now_unix_ms),
                tolerance,
            );
            if takes {
                return Some(take_locked(&mut inner, index, node_id, now_unix_ms, raw));
            }
        }
        None
    }

    /// Whether `result` is a plausible answer to the unit dispatched under its id: same shape,
    /// a payload of that size, only finite numbers, and the checksum the worker would have
    /// computed (same sum, same order). `None` when no such unit is in flight.
    pub fn result_is_valid(&self, result: &WorkResult) -> Option<bool> {
        let inner = self.lock();
        let unit = &inner.dispatched.get(&result.work_id)?.unit;
        let size = (unit.rows as usize).checked_mul(unit.cols as usize);
        Some(
            result.rows == unit.rows
                && result.cols == unit.cols
                && size == Some(result.matrix_c.len())
                && result.matrix_c.iter().all(|value| value.is_finite())
                && result.checksum == result.matrix_c.iter().sum::<f64>(),
        )
    }

    /// Takes an in-flight unit back after its node answered wrongly. The blame is the node's,
    /// so the dispatch does not count against the unit's retry budget: a lying node must not
    /// be able to fail work an honest one would finish. The loop is bounded by the node's
    /// quarantine. False when the unit was not in flight on that node.
    pub fn reject(&self, work_id: &str, node_id: &str) -> bool {
        let mut inner = self.lock();
        if inner
            .dispatched
            .get(work_id)
            .is_none_or(|record| record.node_id != node_id)
        {
            return false;
        }
        let record = inner.dispatched.remove(work_id).expect("checked above");
        if let Some(attempts) = inner.attempts.get_mut(work_id) {
            *attempts = attempts.saturating_sub(1);
        }
        inner.queue.push_back(record.unit);
        self.save_locked(&mut inner);
        true
    }

    /// Returns the record only when the unit was dispatched to the same node.
    ///
    /// ponytail: latency derives from millisecond timestamps, so total/transfer
    /// resolution is ~1ms; fine while the workload runs for hundreds of ms. Move to
    /// microsecond timestamps when units shorter than ~10ms matter.
    pub fn complete(&self, result: WorkResult, now_unix_ms: u64) -> Option<CompletedWork> {
        let mut inner = self.lock();
        let dispatched = inner.dispatched.get(&result.work_id)?;
        // A result must come from the node the unit was handed to.
        if dispatched.node_id != result.node_id {
            return None;
        }
        let dispatched = inner.dispatched.remove(&result.work_id)?;
        let previous = inner.learned.get(&result.node_id).copied();
        if let Some(ratio) = learn(
            previous,
            dispatched.raw_predicted_us,
            result.compute_time_us,
        ) {
            inner.learned.insert(result.node_id.clone(), ratio);
        }
        let total_latency_us = now_unix_ms
            .saturating_sub(dispatched.dispatched_at_unix_ms)
            .saturating_mul(1_000);
        let completed = CompletedWork {
            work_id: result.work_id.clone(),
            node_id: result.node_id.clone(),
            request_id: dispatched.unit.request_id.clone(),
            compute_time_us: result.compute_time_us,
            total_latency_us,
            transfer_time_us: total_latency_us.saturating_sub(result.compute_time_us),
            predicted_compute_us: dispatched.predicted_compute_us,
            checksum: result.checksum,
        };
        inner.completed.push(completed.clone());
        if inner.keep.remove(&result.work_id) {
            if inner.results.len() == MAX_KEPT_RESULTS {
                inner.results.pop_front();
            }
            inner.results.push_back(KeptResult {
                request_id: completed.request_id.clone(),
                rows: result.rows,
                cols: result.cols,
                data: result.matrix_c.clone(),
            });
        }
        // Recorded so units that depend on this one become claimable, and so a
        // dependent unit can read this unit's output as its own input.
        inner
            .outputs
            .insert(result.work_id.clone(), result.matrix_c);
        // The unit is settled, so its retry budget no longer matters.
        inner.attempts.remove(&result.work_id);
        self.save_locked(&mut inner);
        Some(completed)
    }

    /// Cancels a request (F11): every unit of it still in the queue is dropped, whatever it waits
    /// for. Units already on a worker run to the end (the worker has no way to be told), and their
    /// results are discarded when they come in. Returns `(removed, in_flight)`.
    pub fn cancel_request(&self, request_id: &str) -> (usize, usize) {
        let mut inner = self.lock();
        let before = inner.queue.len();
        let mut dropped = Vec::new();
        inner.queue.retain(|unit| {
            let keep = unit.request_id != request_id;
            if !keep {
                dropped.push(unit.work_id.clone());
            }
            keep
        });
        for work_id in &dropped {
            inner.passed.remove(work_id);
            inner.attempts.remove(work_id);
            inner.keep.remove(work_id);
        }
        let in_flight = inner
            .dispatched
            .values()
            .filter(|record| record.unit.request_id == request_id)
            .count();
        let removed = before - inner.queue.len();
        if removed > 0 {
            // A kept intermediate output nothing can read any more is dead weight.
            drop_unreferenced_outputs(&mut inner);
            self.save_locked(&mut inner);
        }
        (removed, in_flight)
    }

    /// Asks the store to keep the output of `work_id` for `kept_output` (J1).
    pub fn keep_output(&self, work_id: &str) {
        self.lock().keep.insert(work_id.to_string());
    }

    /// The kept output of a request as `(rows, cols, data)`, if it finished and is still held.
    pub fn kept_output(&self, request_id: &str) -> Option<(u32, u32, Vec<f64>)> {
        self.lock()
            .results
            .iter()
            .rev()
            .find(|kept| kept.request_id == request_id)
            .map(|kept| (kept.rows, kept.cols, kept.data.clone()))
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

    /// Intermediate results still held for units that have not settled.
    ///
    /// Zero once a graph drains: an output is dropped as soon as nothing queued or in flight
    /// can read it (`drop_unreferenced_outputs`).
    pub fn outputs_held(&self) -> usize {
        self.lock().outputs.len()
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
                outcome.failed += fail_cascade(&mut inner, &work_id, &record.unit.request_id);
            } else {
                inner.queue.push_back(record.unit);
                outcome.requeued += 1;
            }
        }
        self.save_locked(&mut inner);
        outcome
    }

    /// Owners of in-flight units whose node is not in `known`, in stable order.
    ///
    /// Called by the controller's sweep once the heartbeat grace has passed since startup.
    /// After a restart the registry is empty while the snapshot still names the previous
    /// owners, so the sweep cannot tell "the owner is coming back" from "the owner is gone"
    /// until the grace window closes. This is the list to hand to `requeue_for_node`.
    pub fn unknown_owners(&self, known: &[String]) -> Vec<String> {
        let inner = self.lock();
        let mut owners: Vec<String> = inner
            .dispatched
            .values()
            .map(|record| record.node_id.clone())
            .filter(|node_id| !known.iter().any(|known| known == node_id))
            .collect();
        owners.sort();
        owners.dedup();
        owners
    }

    /// Ids of units that will never complete, in stable order. Includes the units
    /// failed for spending their retry budget and the dependents failed with them.
    pub fn failed_units(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.lock().failed.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn failed(&self) -> usize {
        self.lock().failed.len()
    }

    /// The path a client request took: every unit it produced, where each one is in its
    /// life, and what it cost (H3).
    ///
    /// Units come grouped by state — queued, dispatched, completed, failed — in the order
    /// the controller moved them within each group. For a graph, whose units are enqueued
    /// in execution order and settle in that order, the completed group reads as the
    /// execution path. An unknown id yields an empty trace rather than an error: a trace
    /// lost to a restart is not a client mistake.
    ///
    /// ponytail: completed units are not persisted and a failed unit's request id is lost
    /// on reload, so a trace only covers the current run. Enough to explain a request's
    /// latency while it is in flight or just finished, which is the point; persist the
    /// correlation when traces need to outlive a restart.
    pub fn trace(&self, request_id: &str) -> Vec<TraceEntry> {
        let inner = self.lock();
        let mut units = Vec::new();
        for unit in &inner.queue {
            if unit.request_id == request_id {
                units.push(trace_row(&unit.work_id, "", "queued", None));
            }
        }
        for record in inner.dispatched.values() {
            if record.unit.request_id == request_id {
                units.push(trace_row(
                    &record.unit.work_id,
                    &record.node_id,
                    "dispatched",
                    None,
                ));
            }
        }
        for record in &inner.completed {
            if record.request_id == request_id {
                units.push(trace_row(
                    &record.work_id,
                    &record.node_id,
                    "completed",
                    Some(record),
                ));
            }
        }
        for (work_id, failed_request) in &inner.failed {
            if failed_request == request_id {
                units.push(trace_row(work_id, "", "failed", None));
            }
        }
        units
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

    /// A unit large enough that compute dominates the fixed transfer term.
    ///
    /// The 2x2x2 helper is too small to test placement on: its footprint is 96 bytes
    /// against 16 flops, so the thread-independent transfer term dwarfs compute and the
    /// cost gap between a 2-thread and an 8-thread node is only ~6%. 64x64x64 gives
    /// 98_304 bytes against 524_288 flops, so threads move the cost by ~2.2x — a real
    /// difference rather than noise.
    fn big_unit(work_id: &str) -> WorkUnit {
        WorkUnit {
            work_id: work_id.to_string(),
            rows: 64,
            shared: 64,
            cols: 64,
            matrix_a: vec![0.0; 64 * 64],
            matrix_b: vec![0.0; 64 * 64],
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
            request_id: "req-test".to_string(),
            priority: 0,
        }
    }

    fn with_priority(work_id: &str, priority: u32) -> WorkUnit {
        WorkUnit {
            priority,
            ..unit(work_id)
        }
    }

    fn answer(work_id: &str, node_id: &str, data: Vec<f64>, checksum: f64) -> WorkResult {
        WorkResult {
            work_id: work_id.to_string(),
            node_id: node_id.to_string(),
            rows: 2,
            cols: 2,
            matrix_c: data,
            checksum,
            compute_time_us: 1,
        }
    }

    #[test]
    fn a_result_is_valid_only_with_the_right_shape_finite_numbers_and_checksum() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        store.claim("n", &CapabilityReport::default(), 1).unwrap();
        let good = vec![1.0, 2.0, 3.0, 4.0];
        assert_eq!(
            store.result_is_valid(&answer("w1", "n", good.clone(), 10.0)),
            Some(true)
        );
        assert_eq!(
            store.result_is_valid(&answer("w1", "n", good.clone(), 11.0)),
            Some(false)
        );
        assert_eq!(
            store.result_is_valid(&answer("w1", "n", vec![1.0], 1.0)),
            Some(false)
        );
        let nan = vec![f64::NAN, 0.0, 0.0, 0.0];
        assert_eq!(
            store.result_is_valid(&answer("w1", "n", nan, f64::NAN)),
            Some(false)
        );
        assert_eq!(
            store.result_is_valid(&answer("ghost", "n", good, 10.0)),
            None
        );
    }

    #[test]
    fn a_rejected_unit_goes_back_to_the_queue_without_spending_its_attempts() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        for _ in 0..MAX_ATTEMPTS * 2 {
            store.claim("n", &CapabilityReport::default(), 1).unwrap();
            assert!(!store.reject("w1", "someone-else"));
            assert!(store.reject("w1", "n"));
        }
        assert_eq!(store.pending(), 1);
        assert!(store.failed_units().is_empty());
    }

    #[test]
    fn cancelling_a_request_drops_its_queued_units_and_leaves_others() {
        let store = WorkStore::new();
        store.enqueue([
            unit_for("a1", "req-a"),
            unit_for("b1", "req-b"),
            unit_for("a2", "req-a"),
        ]);
        // One unit of req-a is already on a worker.
        let claimed = store.claim("n", &CapabilityReport::default(), 1).unwrap();
        assert_eq!(claimed.work_id, "a1");

        let (removed, in_flight) = store.cancel_request("req-a");
        assert_eq!((removed, in_flight), (1, 1));
        assert_eq!(store.cancel_request("req-a"), (0, 1));
        assert_eq!(store.cancel_request("ghost"), (0, 0));
        // Only req-b's unit is left to claim.
        let next = store.claim("n", &CapabilityReport::default(), 2).unwrap();
        assert_eq!(next.work_id, "b1");
        assert!(store.claim("n", &CapabilityReport::default(), 3).is_none());
    }

    #[test]
    fn the_learned_ratio_moves_toward_what_the_node_really_takes() {
        // The model says 100 us and the node keeps taking 400: the ratio walks to 4.
        let mut ratio = None;
        for _ in 0..30 {
            ratio = learn(ratio, 100, 400);
        }
        assert!((ratio.unwrap() - 4.0).abs() < 0.01);
        assert_eq!(corrected(ratio, 100), 400);
        // Unknown predictions and empty timings teach nothing; outliers are clamped.
        assert_eq!(learn(Some(2.0), 0, 500), Some(2.0));
        assert_eq!(learn(Some(2.0), 100, 0), Some(2.0));
        assert_eq!(learn(None, 1, 1_000_000), Some(RATIO_BAND.1));
        assert_eq!(corrected(None, 100), 100);
    }

    #[test]
    fn a_completed_unit_corrects_the_next_prediction_for_that_node() {
        let store = WorkStore::new();
        let capability = CapabilityReport {
            cpu_threads: 2,
            compute_score: 1.0,
            ..Default::default()
        };
        store.enqueue([big_unit("a"), big_unit("b")]);
        store.claim("node-a", &capability, 1).unwrap();
        let raw = predicted_compute_us(&big_unit("a"), &capability);
        assert!(raw > 0);
        // The node took four times what the model said.
        store.complete(result("a", raw * 4), 2).unwrap();
        store.claim("node-a", &capability, 3).unwrap();
        let completed = store.complete(result("b", raw * 4), 4).unwrap();
        assert_eq!(completed.predicted_compute_us, raw * 4);
    }

    #[test]
    fn a_higher_priority_unit_is_claimed_before_an_older_lower_one() {
        let store = WorkStore::new();
        store.enqueue([with_priority("low", 0), with_priority("high", 2)]);
        let first = store.claim("n", &CapabilityReport::default(), 1).unwrap();
        assert_eq!(first.work_id, "high");
        let second = store.claim("n", &CapabilityReport::default(), 2).unwrap();
        assert_eq!(second.work_id, "low");
    }

    #[test]
    fn equal_priority_stays_fifo() {
        let store = WorkStore::new();
        store.enqueue([unit("a"), unit("b"), unit("c")]);
        let order: Vec<String> = (0..3)
            .map(|t| {
                store
                    .claim("n", &CapabilityReport::default(), t)
                    .unwrap()
                    .work_id
            })
            .collect();
        assert_eq!(order, ["a", "b", "c"]);
    }

    #[test]
    fn aging_lets_an_old_low_priority_unit_overtake_a_stream_of_high_ones() {
        let store = WorkStore::new();
        store.enqueue([with_priority("old-low", 0)]);
        // A fresh high-priority unit arrives before every claim, so without aging the low
        // one would wait forever. Each pass-over is worth one point against PRIORITY_SPAN.
        let mut claimed = Vec::new();
        for i in 0..=PRIORITY_SPAN {
            store.enqueue([with_priority(&format!("h{i}"), 1)]);
            claimed.push(
                store
                    .claim("n", &CapabilityReport::default(), 0)
                    .unwrap()
                    .work_id,
            );
        }
        let at = claimed.iter().position(|id| id == "old-low");
        assert!(
            at.is_some_and(|at| at > 0 && at <= PRIORITY_SPAN as usize),
            "{at:?}"
        );
    }

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
            request_id: "req-test".to_string(),
            priority: 0,
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
    fn a_kept_output_outlives_the_sweep_and_the_window_is_bounded() {
        let store = WorkStore::new();
        store.keep_output("w1");
        store.enqueue([unit("w1"), unit("w2")]);
        store.claim("node-a", &capability(8, 0.0), 1_000).unwrap();
        store.claim("node-a", &capability(8, 0.0), 1_000).unwrap();
        store.complete(result("w1", 10), 2_000).unwrap();
        store.complete(result("w2", 10), 2_000).unwrap();

        // w2 was not kept, w1 was: nothing references either output, yet w1's survives.
        assert_eq!(store.outputs_held(), 0);
        let (rows, cols, data) = store.kept_output("req-test").expect("w1 was kept");
        assert_eq!((rows, cols), (2, 2));
        assert_eq!(data, vec![19.0, 22.0, 43.0, 50.0]);
        assert!(store.kept_output("req-other").is_none());

        // Only the newest MAX_KEPT_RESULTS are held.
        for n in 0..MAX_KEPT_RESULTS + 1 {
            let mut one = unit(&format!("k{n}"));
            one.request_id = format!("req-k{n}");
            store.keep_output(&one.work_id);
            store.enqueue([one]);
            store.claim("node-a", &capability(8, 0.0), 1_000).unwrap();
            store.complete(result(&format!("k{n}"), 10), 2_000).unwrap();
        }
        assert!(store.kept_output("req-test").is_none());
        assert!(store.kept_output("req-k0").is_none());
        assert!(
            store
                .kept_output(&format!("req-k{MAX_KEPT_RESULTS}"))
                .is_some()
        );
    }

    #[test]
    fn enqueue_claim_complete_round_trip() {
        let store = WorkStore::new();
        assert_eq!(store.pending(), 0);
        assert!(store.claim("node-a", &capability(8, 0.0), 1_000).is_none());

        assert_eq!(store.enqueue([unit("w1"), unit("w2")]), 2);
        assert_eq!(store.pending(), 2);

        let claimed = store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("queue is not empty");
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
        store.claim("node-a", &capability(8, 0.0), 1_000);

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
        store.claim("node-a", &capability(8, 0.0), 1_000);
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
        assert_eq!(
            store
                .claim("node-a", &capability(8, 0.0), 1_000)
                .expect("w1")
                .work_id,
            "w1"
        );
        assert_eq!(store.pending(), 1);

        // node-a leaves: the unit goes back to the queue and is still outstanding.
        assert_eq!(store.requeue_for_node("node-a").requeued, 1);
        assert_eq!(store.pending(), 1);

        // Its dispatch record is gone, so a late result from node-a is refused.
        assert!(store.complete(result("w1", 1_000), 2_000).is_none());
        assert!(store.completed().is_empty());

        // Another node picks the same unit up and finishes it.
        let taken = store
            .claim("node-b", &capability(8, 0.0), 2_000)
            .expect("requeued unit");
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
        store.claim("node-a", &capability(8, 0.0), 1_000);
        store.claim("node-b", &capability(8, 0.0), 1_000);
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
        let first = store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
        assert_eq!(first.work_id, "w1");

        // w2 stays blocked while its dependency is still in flight.
        assert!(store.claim("node-b", &capability(8, 0.0), 1_000).is_none());
        assert_eq!(store.blocked(), 1);

        store.complete(result("w1", 1_000), 2_000).expect("w1 done");
        assert_eq!(store.blocked(), 0);

        // With w1 complete, w2 becomes claimable.
        let second = store
            .claim("node-b", &capability(8, 0.0), 2_000)
            .expect("w2 is ready");
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
        let claimed = store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
        assert_eq!(claimed.work_id, "w1");
    }

    #[test]
    fn requeue_keeps_dependencies_intact() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), dependent("w2", &["w1"])]);
        let first = store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
        assert_eq!(first.work_id, "w1");

        // node-a leaves with w1 in flight; w1 returns to the queue still ready.
        assert_eq!(store.requeue_for_node("node-a").requeued, 1);
        let again = store
            .claim("node-b", &capability(8, 0.0), 2_000)
            .expect("w1 requeued");
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
        let first = store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
        assert_eq!(first.work_id, "w1");
        let mut done = result("w1", 1_000);
        done.matrix_c = vec![7.0, 8.0, 9.0, 10.0];
        store.complete(done, 2_000).expect("w1 done");

        // w2 is dispatched with w1's output substituted into both inputs.
        let second = store
            .claim("node-b", &capability(8, 0.0), 2_000)
            .expect("w2 is ready");
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
        assert!(store.claim("node-a", &capability(8, 0.0), 1_000).is_none());
        assert_eq!(store.blocked(), 1);
    }

    #[test]
    fn a_unit_is_failed_once_its_retry_budget_is_spent() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);

        // The first attempts are handed back for another try.
        for _ in 0..MAX_ATTEMPTS - 1 {
            assert_eq!(
                store
                    .claim("node-a", &capability(8, 0.0), 1_000)
                    .expect("w1")
                    .work_id,
                "w1"
            );
            let outcome = store.requeue_for_node("node-a");
            assert_eq!(outcome.requeued, 1);
            assert_eq!(outcome.failed, 0);
            assert_eq!(store.pending(), 1);
        }

        // The last attempt spends the budget: the unit is not handed back again.
        assert_eq!(
            store
                .claim("node-a", &capability(8, 0.0), 2_000)
                .expect("w1")
                .work_id,
            "w1"
        );
        let outcome = store.requeue_for_node("node-a");
        assert_eq!(outcome.requeued, 0);
        assert_eq!(outcome.failed, 1);
        assert_eq!(store.failed_units(), vec!["w1".to_string()]);
        // The graph is no longer outstanding, so a polling client terminates.
        assert_eq!(store.pending(), 0);
        assert!(store.claim("node-b", &capability(8, 0.0), 3_000).is_none());
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
            assert_eq!(
                store
                    .claim("node-a", &capability(8, 0.0), 1_000)
                    .expect("w1")
                    .work_id,
                "w1"
            );
            store.requeue_for_node("node-a");
        }

        // w2 can never run now, so it is failed rather than left blocking the queue.
        assert_eq!(
            store.failed_units(),
            vec!["w1".to_string(), "w2".to_string()]
        );
        assert_eq!(store.pending(), 0);
        assert_eq!(store.blocked(), 0);
        assert!(store.claim("node-b", &capability(8, 0.0), 2_000).is_none());
    }

    #[test]
    fn a_failed_branch_does_not_fail_unrelated_work() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), dependent("w2", &["w1"])]);
        for _ in 0..MAX_ATTEMPTS {
            assert_eq!(
                store
                    .claim("node-a", &capability(8, 0.0), 1_000)
                    .expect("w1")
                    .work_id,
                "w1"
            );
            store.requeue_for_node("node-a");
        }
        assert_eq!(
            store.failed_units(),
            vec!["w1".to_string(), "w2".to_string()]
        );

        // The store still accepts and runs work after a branch failed.
        store.enqueue([unit("w3")]);
        assert_eq!(store.pending(), 1);
        assert_eq!(
            store
                .claim("node-b", &capability(8, 0.0), 2_000)
                .expect("w3")
                .work_id,
            "w3"
        );
    }

    #[test]
    fn a_trace_follows_a_unit_from_queue_to_completion() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);

        // Queued: the unit is there, owned by no node, with no timings yet.
        let queued = store.trace("req-test");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].work_id, "w1");
        assert_eq!(queued[0].status, "queued");
        assert_eq!(queued[0].node_id, "");
        assert_eq!(queued[0].compute_time_us, 0);

        // Dispatched: the node is named, still no timings.
        store.claim("node-a", &capability(8, 0.0), 1_000);
        let dispatched = store.trace("req-test");
        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].status, "dispatched");
        assert_eq!(dispatched[0].node_id, "node-a");
        assert_eq!(dispatched[0].total_latency_us, 0);

        // Completed: the round trip splits into compute and transfer.
        store.complete(result("w1", 250_000), 1_500);
        let completed = store.trace("req-test");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].status, "completed");
        assert_eq!(completed[0].node_id, "node-a");
        assert_eq!(completed[0].compute_time_us, 250_000);
        assert_eq!(completed[0].transfer_time_us, 250_000);
        assert_eq!(completed[0].total_latency_us, 500_000);
    }

    #[test]
    fn a_trace_lists_every_unit_a_request_produced() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), dependent("w2", &["w1"])]);

        // Both units belong to the request, even though only w1 can run so far.
        let trace = store.trace("req-test");
        assert_eq!(trace.len(), 2);
        let w2 = trace.iter().find(|u| u.work_id == "w2").unwrap();
        // A unit waiting on its dependency reads as queued, with no cost to report yet.
        assert_eq!(w2.status, "queued");
        assert_eq!(w2.total_latency_us, 0);
    }

    #[test]
    fn a_trace_shows_a_failed_unit() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        for _ in 0..MAX_ATTEMPTS {
            store.claim("node-a", &capability(8, 0.0), 1_000);
            store.requeue_for_node("node-a");
        }

        let trace = store.trace("req-test");
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].work_id, "w1");
        assert_eq!(trace[0].status, "failed");
        assert_eq!(trace[0].node_id, "");
    }

    #[test]
    fn a_trace_keeps_other_requests_out() {
        let store = WorkStore::new();
        let mut other = unit("w2");
        other.request_id = "req-outro".to_string();
        store.enqueue([unit("w1"), other]);

        let trace = store.trace("req-test");
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].work_id, "w1");
    }

    #[test]
    fn a_trace_of_an_unknown_request_is_empty() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        assert!(store.trace("nao-existe").is_empty());
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
            store
                .claim("node-weak", &capability(2, 0.0), 1_000)
                .expect("w2 fits")
                .work_id,
            "w2"
        );

        // A capable node gets the unit the weak node skipped.
        assert_eq!(
            store
                .claim("node-strong", &capability(8, 0.0), 2_000)
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
        assert!(
            store
                .claim("node-weak", &capability(2, 0.0), 3_000)
                .is_none()
        );
        assert_eq!(
            store
                .claim("node-strong", &capability(8, 0.0), 4_000)
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
                .claim_placed("node-weak", &capability(2, 0.0), 1_000, &candidates, 0.0)
                .is_none()
        );
        // The unit is still queued, not lost, and the node that fits it takes it.
        assert_eq!(store.pending(), 1);
        assert_eq!(
            store
                .claim_placed("node-strong", &capability(8, 0.0), 1_000, &candidates, 0.0)
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
                .claim_placed("node-strong", &capability(8, 0.0), 1_000, &candidates, 0.0)
                .expect("strong fits w1")
                .work_id,
            "w1"
        );
        // Now strong is busy, so the weak node is the only idle candidate and takes
        // the next unit instead of leaving it stranded behind a node that is not
        // polling.
        assert_eq!(
            store
                .claim_placed("node-weak", &capability(2, 0.0), 2_000, &candidates, 0.0)
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
                .claim_placed("node-weak", &capability(2, 0.0), 1_000, &candidates, 0.0)
                .expect("weak is the best fit")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn a_near_equal_node_takes_the_unit_instead_of_yielding() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        let candidates = [
            ("node-a".to_string(), capability(4, 0.0), healthy()),
            ("node-b".to_string(), capability(4, 0.0), healthy()),
        ];

        // Hysteresis: node-b is as good as node-a, so it takes the unit instead of
        // yielding and waiting a poll interval for a saving that would not exist. The
        // band replaces the old id tie-break — near-equal nodes cannot strand a unit
        // between them, because either one may take it.
        assert_eq!(
            store
                .claim_placed("node-b", &capability(4, 0.0), 1_000, &candidates, 0.10)
                .expect("node-b is within the band")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn a_clearly_worse_node_still_yields_with_the_band_on() {
        let store = WorkStore::new();
        // A big unit: on the 2x2x2 helper the thread-independent transfer term dwarfs
        // compute, so the gap between 2 and 8 threads is only ~6% and the band swallows
        // it. Here the gap is ~2.2x, so "clearly worse" is actually true.
        store.enqueue([big_unit("w1")]);
        let candidates = [
            ("node-weak".to_string(), capability(2, 0.0), healthy()),
            ("node-strong".to_string(), capability(8, 0.0), healthy()),
        ];

        // The band must not turn placement back into first-come-first-served: a node
        // whose cost is far above the cheapest still yields, band or no band.
        assert!(
            store
                .claim_placed("node-weak", &capability(2, 0.0), 1_000, &candidates, 0.10)
                .is_none()
        );
        assert_eq!(
            store
                .claim_placed("node-strong", &capability(8, 0.0), 1_000, &candidates, 0.10)
                .expect("strong is the cheapest fit")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn a_zero_band_means_strictly_cheapest() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        let candidates = [
            ("node-weak".to_string(), capability(2, 0.0), healthy()),
            ("node-strong".to_string(), capability(8, 0.0), healthy()),
        ];

        // The pre-G5 policy, kept so the band's effect can be measured against it.
        assert!(
            store
                .claim_placed("node-weak", &capability(2, 0.0), 1_000, &candidates, 0.0)
                .is_none()
        );
    }

    #[test]
    fn a_wide_band_lets_a_worse_node_take_the_unit() {
        let store = WorkStore::new();
        store.enqueue([unit("w1")]);
        let candidates = [
            ("node-weak".to_string(), capability(2, 0.0), healthy()),
            ("node-strong".to_string(), capability(8, 0.0), healthy()),
        ];

        // A band wide enough to cover the gap admits the weak node — the knob is what
        // decides how much worse a candidate may be and still take the unit. This is
        // the dial between "cheapest wins" and "first to ask wins".
        assert_eq!(
            store
                .claim_placed("node-weak", &capability(2, 0.0), 1_000, &candidates, 10.0)
                .expect("a wide band admits the weak node")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn a_node_that_cannot_hold_the_unit_is_skipped() {
        let store = WorkStore::new();
        // The test unit is 2x2x2: (4 + 4 + 4) elements * 8 bytes = 96 bytes resident.
        store.enqueue([unit("w1")]);

        // Reported memory below the footprint: the unit is not handed over.
        let mut tight = capability(8, 0.0);
        tight.memory_bytes = 95;
        assert!(store.claim("node-tight", &tight, 1_000).is_none());
        assert_eq!(store.pending(), 1);

        // A node with exactly enough room takes it.
        let mut exact = capability(2, 0.0);
        exact.memory_bytes = 96;
        assert_eq!(
            store
                .claim("node-exact", &exact, 2_000)
                .expect("w1 fits exactly")
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
        store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
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
                .claim("node-b", &capability(8, 0.0), 3_000)
                .expect("w2 is ready")
                .work_id,
            "w2"
        );
        // The id sequence continued instead of restarting, so nothing collides.
        assert_eq!(reloaded.next_work_id(), "w-1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The point of persisting ownership: a unit in flight when the controller dies comes
    /// back owned by the same node, so the result that node eventually sends is accepted.
    /// Before the lease was saved the unit came back only in the queue, and the reloaded
    /// controller rejected the very result the worker was already computing.
    #[test]
    fn an_in_flight_unit_keeps_its_owner_across_a_restart() {
        let dir = std::env::temp_dir().join(format!("aic-work-owner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("state.bin");

        let store = WorkStore::with_state(&path);
        store.enqueue([unit("w1"), unit("w2")]);
        store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
        store
            .claim("node-b", &capability(8, 0.0), 1_000)
            .expect("w2 is ready");
        // Completing w2 writes the snapshot, and that is where w1's lease gets persisted:
        // `claim` deliberately does not save, so only a later mutation carries it to disk.
        let mut w2_result = result("w2", 250);
        w2_result.node_id = "node-b".to_string();
        store.complete(w2_result, 2_000).expect("w2 was dispatched");

        let reloaded = WorkStore::with_state(&path);
        // w1 is still in flight, not back in the queue, and still owned by node-a.
        assert_eq!(reloaded.pending(), 1);
        assert_eq!(reloaded.blocked(), 0);
        assert_eq!(
            reloaded.unknown_owners(&["node-a".to_string()]),
            Vec::<String>::new()
        );
        assert_eq!(
            reloaded.unknown_owners(&["node-b".to_string()]),
            vec!["node-a".to_string()]
        );
        reloaded
            .complete(result("w1", 250), 3_000)
            .expect("w1's owner survived the restart, so its result is accepted");
        assert_eq!(reloaded.pending(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The redundant copy is what makes a single damaged file survivable. Only when both
    /// copies are unreadable does the store start empty, and even then it starts.
    #[test]
    fn a_corrupt_primary_snapshot_falls_back_to_the_replica() {
        let dir = std::env::temp_dir().join(format!("aic-work-replica-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("state.bin");

        let store = WorkStore::with_state(&path);
        store.enqueue([unit("w1"), unit("w2")]);
        drop(store);
        // The save wrote both files; clobber only the primary.
        std::fs::write(&path, b"truncated").expect("corrupt the primary");
        assert_eq!(WorkStore::with_state(&path).pending(), 2);

        std::fs::write(&path, b"truncated").expect("corrupt the primary");
        std::fs::write(replica_path(&path), b"truncated").expect("corrupt the replica");
        assert_eq!(
            WorkStore::with_state(&path).pending(),
            0,
            "both copies gone means an empty start, not a failure to start"
        );

        // A file truncated to zero bytes decodes as a default snapshot, so it must not be
        // mistaken for "nothing to do" while the copy still holds the queue.
        std::fs::write(&path, b"").expect("truncate the primary");
        assert_eq!(
            WorkStore::with_state(&path).pending(),
            0,
            "both copies empty is a legitimate empty start"
        );
        // Rebuild the copy, then truncate only the primary again: the copy has to win.
        let store = WorkStore::with_state(&path);
        store.enqueue([unit("w1"), unit("w2")]);
        drop(store);
        std::fs::write(&path, b"").expect("truncate the primary");
        assert_eq!(WorkStore::with_state(&path).pending(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A lease whose owner never came back is listed, so the controller's sweep can hand its
    /// work to the nodes that are actually there.
    #[test]
    fn an_unknown_owner_is_listed_until_its_node_registers() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), unit("w2")]);
        store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
        store
            .claim("node-b", &capability(8, 0.0), 1_000)
            .expect("w2 is ready");

        assert_eq!(
            store.unknown_owners(&["node-a".to_string(), "node-b".to_string()]),
            Vec::<String>::new(),
            "a node that is present owns its work, so nothing is reported"
        );
        assert_eq!(
            store.unknown_owners(&["node-b".to_string()]),
            vec!["node-a".to_string()]
        );

        // Handing the missing owner's work back clears it from the list.
        store.requeue_for_node("node-a");
        assert_eq!(store.pending(), 2);
        assert_eq!(
            store.unknown_owners(&["node-b".to_string()]),
            Vec::<String>::new()
        );
    }

    /// A matrix nobody reads is neither checkpointed nor kept in memory. The map used to hold
    /// every output for the process lifetime, and the snapshot rewrote them on every save.
    #[test]
    fn an_output_is_dropped_once_nothing_can_read_it() {
        let store = WorkStore::new();
        store.enqueue([unit("w1"), unit("w2")]);
        store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
        store
            .complete(result("w1", 250), 2_000)
            .expect("w1 was dispatched");
        // w2 is independent, so w1's output is a matrix kept for nobody.
        assert_eq!(store.outputs_held(), 0);

        store
            .claim("node-b", &capability(8, 0.0), 3_000)
            .expect("w2 is ready");
        let mut w2_result = result("w2", 250);
        w2_result.node_id = "node-b".to_string();
        store.complete(w2_result, 4_000).expect("w2 was dispatched");
        assert_eq!(store.outputs_held(), 0);
    }

    /// Both ways an output is read — a dependency reference and a bound input — pin it while an
    /// unfinished unit still needs it. Dropping either would block the graph forever.
    #[test]
    fn an_output_an_unfinished_unit_still_reads_is_kept() {
        // A dependency reference: w2 waits on w1's output to become ready.
        let store = WorkStore::new();
        store.enqueue([unit("w1"), dependent("w2", &["w1"])]);
        store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
        store
            .complete(result("w1", 250), 2_000)
            .expect("w1 was dispatched");
        assert_eq!(store.outputs_held(), 1);
        assert_eq!(store.blocked(), 0, "w2 readies off w1's output");
        assert_eq!(
            store
                .claim("node-b", &capability(8, 0.0), 3_000)
                .expect("w2 is ready")
                .work_id,
            "w2"
        );
        let mut w2_result = result("w2", 250);
        w2_result.node_id = "node-b".to_string();
        store.complete(w2_result, 4_000).expect("w2 was dispatched");
        assert_eq!(
            store.outputs_held(),
            0,
            "the graph is done, so nothing reads either output"
        );

        // A bound input: w3 takes its A matrix from w1 without depending on it.
        let store = WorkStore::new();
        let mut bound = unit("w3");
        bound.matrix_a_from = "w1".to_string();
        store.enqueue([unit("w1"), bound]);
        store
            .claim("node-a", &capability(8, 0.0), 1_000)
            .expect("w1 is ready");
        store
            .complete(result("w1", 250), 2_000)
            .expect("w1 was dispatched");
        assert_eq!(store.outputs_held(), 1);
    }

    /// A unit under a given client request, so a reservation can be matched to it (G6).
    fn unit_for(work_id: &str, request_id: &str) -> WorkUnit {
        WorkUnit {
            request_id: request_id.to_string(),
            ..unit(work_id)
        }
    }

    #[test]
    fn a_reserved_node_takes_its_requests_unit_instead_of_a_cheaper_stranger() {
        let store = WorkStore::new();
        store.enqueue([unit_for("w1", "req-x")]);
        store.reserve("node-weak", "req-x", 0, 1_000);
        let candidates = [
            ("node-weak".to_string(), capability(2, 0.0), healthy()),
            ("node-strong".to_string(), capability(8, 0.0), healthy()),
        ];

        // The hold outranks cost: the cheaper node yields so the unit can land on the
        // node its request reserved, which is what reserving one is for.
        assert!(
            store
                .claim_placed("node-strong", &capability(8, 0.0), 1_001, &candidates, 0.0)
                .is_none()
        );
        assert_eq!(
            store
                .claim_placed("node-weak", &capability(2, 0.0), 1_002, &candidates, 0.0)
                .expect("the held node takes its request's unit")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn a_high_priority_unit_that_yields_does_not_block_a_lower_one() {
        let store = WorkStore::new();
        store.enqueue([
            WorkUnit {
                priority: 3,
                ..unit_for("high", "req-x")
            },
            unit_for("low", "req-y"),
        ]);
        store.reserve("node-weak", "req-x", 0, 1_000);
        let candidates = [
            ("node-weak".to_string(), capability(2, 0.0), healthy()),
            ("node-strong".to_string(), capability(8, 0.0), healthy()),
        ];

        // `high` waits for its held node, so node-strong goes on to take `low`.
        let taken = store
            .claim_placed("node-strong", &capability(8, 0.0), 1_001, &candidates, 0.0)
            .expect("the free node takes the lower-ranked unit");
        assert_eq!(taken.work_id, "low");
    }

    #[test]
    fn a_node_reserved_for_another_request_takes_no_work() {
        let store = WorkStore::new();
        store.enqueue([unit_for("w1", "req-x")]);
        store.reserve("node-a", "req-other", 0, 1_000);
        let candidates = [("node-a".to_string(), capability(8, 0.0), healthy())];

        // node-a is the only candidate, but it is held for someone else: the unit waits
        // instead of being placed on a node another request is holding.
        assert!(
            store
                .claim_placed("node-a", &capability(8, 0.0), 1_001, &candidates, 0.0)
                .is_none()
        );
        assert_eq!(store.pending(), 1);
    }

    #[test]
    fn an_expired_reservation_stops_holding_the_node() {
        let store = WorkStore::new();
        store.enqueue([unit_for("w1", "req-x")]);
        store.reserve("node-a", "req-other", 5_000, 1_000);
        let candidates = [("node-a".to_string(), capability(8, 0.0), healthy())];

        // Inside the window the hold blocks the unit...
        assert!(
            store
                .claim_placed("node-a", &capability(8, 0.0), 2_000, &candidates, 0.0)
                .is_none()
        );
        // ...and once it lapses the node is a normal candidate again, with no timer.
        assert_eq!(
            store
                .claim_placed("node-a", &capability(8, 0.0), 6_000, &candidates, 0.0)
                .expect("the hold lapsed")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn an_ungated_claim_honours_a_reservation_too() {
        let store = WorkStore::new();
        store.enqueue([unit_for("w1", "req-x"), unit_for("w2", "req-other")]);
        store.reserve("node-a", "req-other", 0, 1_000);

        // `claim` is the first-come-first-served path, but a hold is a property of the
        // node, not of the placement policy, so it must survive AI_CLUSTER_PLACEMENT=off:
        // the held node skips the unit it does not own and takes its own.
        assert_eq!(
            store
                .claim("node-a", &capability(8, 0.0), 1_001)
                .expect("the held node takes its own request's unit")
                .work_id,
            "w2"
        );
        // w1 belongs to another request and is still queued: the held node may not take
        // it, but a node nobody holds can.
        assert_eq!(
            store
                .claim("node-b", &capability(8, 0.0), 1_002)
                .expect("w1 is ready for a node that is not held")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn releasing_a_reservation_frees_the_node_for_other_requests() {
        let store = WorkStore::new();
        store.enqueue([unit_for("w1", "req-x")]);
        store.reserve("node-a", "req-other", 0, 1_000);
        let candidates = [("node-a".to_string(), capability(8, 0.0), healthy())];

        assert!(
            store
                .claim_placed("node-a", &capability(8, 0.0), 1_001, &candidates, 0.0)
                .is_none()
        );
        assert!(store.release_reservation("node-a"));
        assert_eq!(
            store
                .claim_placed("node-a", &capability(8, 0.0), 1_002, &candidates, 0.0)
                .expect("the node is free again")
                .work_id,
            "w1"
        );
    }

    #[test]
    fn reservations_lists_active_holds_and_drops_expired_ones() {
        let store = WorkStore::new();
        store.reserve("node-b", "req-x", 5_000, 1_000);
        store.reserve("node-a", "req-y", 500, 1_000);

        // node-a's hold lapsed at 1_500, so only node-b is still held at 2_000.
        let listed = store.reservations(2_000);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0, "node-b");
        assert_eq!(listed[0].1.request_id, "req-x");
    }

    #[test]
    fn a_predicted_compute_time_flows_from_claim_to_trace() {
        let store = WorkStore::new();
        store.enqueue([big_unit("w1")]);
        // A measured compute score, so a prediction exists: 64x64x64 is 524_288 flops and
        // the node runs 4 threads at the reference rate, so 524_288 / (4 * 6_000) = 22us,
        // plus the per-unit setup the compute term does not see.
        assert_eq!(
            store
                .claim("node-a", &capability(4, 1.0), 1_000)
                .expect("w1 is claimed")
                .work_id,
            "w1"
        );
        let completed = store
            .complete(result("w1", 900), 1_050)
            .expect("w1 completes");
        // The prediction is captured at dispatch and survives to completion, where it sits
        // beside the worker's measurement so the error can be read off.
        assert_eq!(completed.predicted_compute_us, 622);
        assert_eq!(completed.compute_time_us, 900);
        let trace = store.trace("req-test");
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].predicted_compute_us, 622);
        assert_eq!(trace[0].compute_time_us, 900);
    }

    #[test]
    fn an_unmeasured_node_leaves_the_prediction_unset_in_the_trace() {
        let store = WorkStore::new();
        store.enqueue([big_unit("w1")]);
        // compute_score 0 means the node never reported a rate, so nothing is predicted.
        // Zero in the trace must read as "not predicted", not as a zero-microsecond guess.
        store
            .claim("node-a", &capability(4, 0.0), 1_000)
            .expect("w1 is claimed");
        store
            .complete(result("w1", 900), 1_050)
            .expect("w1 completes");
        let trace = store.trace("req-test");
        assert_eq!(trace[0].predicted_compute_us, 0);
        assert_eq!(trace[0].compute_time_us, 900);
    }

    /// A copy of the state to offer a controller, taken from a store that has a file.
    fn state_copy(store: &WorkStore) -> StateReplica {
        (*store
            .state_replica()
            .expect("a file-backed store keeps a copy"))
        .clone()
    }

    /// The promotion path (I4): the controller's own files are gone and the only surviving
    /// copy of the queue is the one a node kept. Without it the queue dies with the volume,
    /// which is where the state lives and nowhere else.
    #[test]
    fn a_controller_without_files_restores_the_copy_a_node_holds() {
        let dir = std::env::temp_dir().join(format!("aic-work-promote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("state.bin");

        let store = WorkStore::with_state(&path);
        store.enqueue([unit("w1"), unit("w2")]);
        let copy = state_copy(&store);
        assert!(copy.epoch_unix_ms > 0, "the copy is dated");
        assert_eq!(
            copy.snapshot.len(),
            std::fs::metadata(&path).expect("snapshot").len() as usize,
            "the copy is the very bytes the files got"
        );

        // The volume died: both of the controller's own files are gone with it.
        std::fs::remove_file(&path).expect("remove the primary");
        std::fs::remove_file(replica_path(&path)).expect("remove the local copy");
        let recovered = WorkStore::with_state(&path);
        assert_eq!(recovered.pending(), 0, "nothing left to load");
        assert!(
            recovered.state_replica().is_none(),
            "and no copy of its own to hand out"
        );

        assert!(
            recovered.restore(&copy),
            "the copy a node holds is the only source left"
        );
        assert_eq!(recovered.pending(), 2, "the queue came back");
        assert!(path.exists(), "and the copy came home to the local files");
        // The next restart is a normal one: it loads the state just restored, not a copy
        // from a node.
        assert_eq!(WorkStore::with_state(&path).pending(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A reload carries the epoch of the file it loaded. Without that, a controller that just
    /// restarted would look like epoch zero and take a node's copy for the newer one — the
    /// queue would roll back to whatever the node last held.
    #[test]
    fn a_reloaded_state_keeps_the_epoch_it_was_saved_with() {
        let dir = std::env::temp_dir().join(format!("aic-work-epoch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("state.bin");

        let store = WorkStore::with_state(&path);
        store.enqueue([unit("w1")]);
        let saved = state_copy(&store).epoch_unix_ms;

        let reloaded = WorkStore::with_state(&path);
        assert_eq!(reloaded.pending(), 1);
        assert_eq!(
            state_copy(&reloaded).epoch_unix_ms,
            saved,
            "the reload kept the epoch instead of starting from zero"
        );
        // The next save moves past it, so a node that kept the old copy still gets an update.
        reloaded.enqueue([unit("w2")]);
        assert!(
            state_copy(&reloaded).epoch_unix_ms > saved,
            "the epoch advances on the next save"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A copy never replaces work already in hand. The promotion path recovers a controller
    /// that lost its files; it does not let a node rewind a controller that is serving.
    #[test]
    fn a_copy_never_overwrites_work_already_in_hand() {
        let dir = std::env::temp_dir().join(format!("aic-work-norewind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("state.bin");

        let store = WorkStore::with_state(&path);
        store.enqueue([unit("w1")]);
        let mut copy = state_copy(&store);
        // Strictly newer than anything, so only the work in hand can refuse it.
        copy.epoch_unix_ms = u64::MAX;
        assert!(!store.restore(&copy), "work in hand is not replaced");
        assert_eq!(store.pending(), 1, "the queue is the one being served");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A copy no newer than the state in hand is refused, so a node holding a copy from
    /// before the last save cannot roll the queue back to it.
    #[test]
    fn a_copy_no_newer_than_the_state_in_hand_is_refused() {
        let dir = std::env::temp_dir().join(format!("aic-work-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("state.bin");

        // Work enqueued and completed leaves an empty snapshot with a real epoch, so the
        // emptiness gate passes and only the epoch comparison can refuse the copy.
        let store = WorkStore::with_state(&path);
        store.enqueue([unit("w1")]);
        store.claim("node-a", &capability(8, 0.0), 1_000);
        store.complete(result("w1", 250), 2_000).expect("w1 ran");
        let reloaded = WorkStore::with_state(&path);
        assert_eq!(reloaded.pending(), 0, "the file holds no work");

        let copy = state_copy(&reloaded);
        assert!(
            !reloaded.restore(&copy),
            "a copy level with the state is not newer"
        );
        let mut newer = copy.clone();
        newer.epoch_unix_ms += 1;
        assert!(reloaded.restore(&newer), "a genuinely newer copy is taken");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A memory-only store keeps no copy and takes none: without a state path there is
    /// nothing to replicate and nothing a copy could restore. A run with no file is a
    /// benchmark, not a cluster — it must not adopt a queue a node happens to be holding.
    #[test]
    fn a_memory_only_store_keeps_and_takes_no_copy() {
        let dir = std::env::temp_dir().join(format!("aic-work-memonly-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("state.bin");

        let backed = WorkStore::with_state(&path);
        backed.enqueue([unit("w1")]);
        let copy = state_copy(&backed);

        let memory = WorkStore::new();
        assert!(memory.state_replica().is_none());
        assert!(
            !memory.restore(&copy),
            "a store with no file has nothing to recover"
        );
        assert_eq!(memory.pending(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
