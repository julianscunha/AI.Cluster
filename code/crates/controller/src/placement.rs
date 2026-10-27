//! Placement cost model for the scheduler (Phase G2).
//!
//! Estimates what a work unit will cost on a given node before dispatch, so the
//! scheduler can rank capable candidates instead of taking the first to poll. It
//! is a pure function: the same inputs always give the same score, so the ranking
//! is unit-testable without a running cluster.
//!
//! The score is relative, not a time. The constants below are placeholders until
//! the capability benchmark reports real compute and network rates; only the
//! ordering between nodes is meant to be meaningful today.

use ai_cluster_common::matrix::REFERENCE_CORE_GFLOPS;
use ai_cluster_protocol::cluster::{CapabilityReport, HealthReport, WorkUnit};

use crate::reservations::Hold;

/// Relative weight of one unit of compute work.
const COMPUTE_WEIGHT: f64 = 1.0;
/// Relative weight of one byte moved.
const TRANSFER_WEIGHT: f64 = 1.0;
/// Cost added per unit already in flight to the node: a queued unit waits.
const QUEUE_WEIGHT: f64 = 50.0;
/// How much a degraded health score inflates the total. 0 ignores health.
const HEALTH_WEIGHT: f64 = 1.0;
/// Bytes per f64 element of a matrix.
const BYTES_PER_ELEMENT: f64 = 8.0;

/// Per-unit setup the pure compute model does not see, in microseconds.
///
/// Cloning the operands and spawning the threads costs roughly the same whatever the unit's
/// size, so it dominates a small unit and disappears under a large one. Measured against a
/// real 64³ unit: ~648us where pure compute predicts ~22us. A single constant, **not** a band
/// per size — the three sizes measured (64³/128³/256³) gave 626/433/313us, spread by host
/// noise far more than by size, so more than one number would be fitting the noise. Override
/// with `AI_CLUSTER_UNIT_SETUP_US`; bands are the next rung once the measurement is stable.
const UNIT_SETUP_US: f64 = 600.0;

/// Above this relative deviation the benchmark's runs disagreed too much for its typical/best
/// ratio to mean anything, so the ratio is dropped and the best case is used unchanged.
const DEVIATION_TRUST_LIMIT: f64 = 0.25;

/// The node side of the comparison. Borrowed so scoring does not clone registry
/// state on every poll.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub capability: &'a CapabilityReport,
    pub health: &'a HealthReport,
    /// Units already dispatched to this node and not yet complete.
    pub in_flight: u32,
}

/// Effective compute rate in arbitrary units per microsecond. Uses the benchmark's
/// `compute_score` when the worker reported one and falls back to thread count —
/// the worker leaves `compute_score` at 0 until the capability benchmark runs.
fn compute_rate(capability: &CapabilityReport) -> f64 {
    let threads = effective_threads(capability);
    let per_thread = if capability.compute_score > 0.0 {
        capability.compute_score
    } else {
        1.0
    };
    // Each unit gets a slot's share of the cores, so that is the rate a unit really sees.
    threads * per_thread / f64::from(capability.work_slots.max(1))
}

/// How well memory keeps up when the cores work together, 0.25..1: the aggregate copy rate with up
/// to four threads over what that many threads would do if memory scaled linearly. A board whose
/// memory bus saturates (a single channel shared by every core) reads well below 1. One means
/// "scales" and also "not measured", so a node that reported nothing keeps its thread count.
fn memory_scaling(capability: &CapabilityReport) -> f64 {
    let probed = f64::from(capability.cpu_threads.clamp(1, 4));
    if probed < 2.0
        || capability.memory_bandwidth_score <= 0.0
        || capability.memory_bandwidth_loaded_score <= 0.0
    {
        return 1.0;
    }
    (capability.memory_bandwidth_loaded_score / (capability.memory_bandwidth_score * probed))
        .clamp(0.25, 1.0)
}

/// Threads as the placement should count them: the first is whole, the rest are worth what the
/// memory lets them be. Without a loaded measurement this is just the thread count.
fn effective_threads(capability: &CapabilityReport) -> f64 {
    let threads = f64::from(capability.cpu_threads).max(1.0);
    1.0 + (threads - 1.0) * memory_scaling(capability)
}

/// Per-thread rate for a time prediction: the node's current measurement when it has one,
/// the registration baseline otherwise, then the thread fallback.
///
/// Placement ranking deliberately stays on `compute_rate` — the baseline — so a noisy
/// instant cannot reshuffle the candidate order. The prediction uses what the node is doing
/// now, which is what the trace compares against the real time (Fase C3): a node that got
/// slower after registering predicts a longer unit without waiting for a health report to
/// blame memory or temperature.
fn per_thread_compute_current(capability: &CapabilityReport) -> f64 {
    if capability.current_compute_score > 0.0 {
        capability.current_compute_score
    } else if capability.compute_score > 0.0 {
        capability.compute_score
    } else {
        1.0
    }
}

/// The benchmark's typical-over-best ratio, applied to the rate a prediction uses (C3).
///
/// `compute_score` is the best of the runs — an extreme, and the machine rarely repeats it.
/// The median is the typical run, so `median / best` is exactly how much optimism to take
/// back out. Gated by `compute_deviation`: if the benchmark's own runs disagreed by more than
/// the trust limit the ratio is noise, and the best case is used unchanged. No median (a
/// pinned or never-benchmarked node) also means no ratio. Never above 1 — the median cannot
/// beat the best.
fn typical_ratio(capability: &CapabilityReport) -> f64 {
    if capability.compute_deviation > DEVIATION_TRUST_LIMIT
        || capability.compute_score <= 0.0
        || capability.compute_median_score <= 0.0
    {
        return 1.0;
    }
    (capability.compute_median_score / capability.compute_score).clamp(0.0, 1.0)
}

/// The per-unit setup from `AI_CLUSTER_UNIT_SETUP_US`, else the measured constant.
fn setup_us() -> f64 {
    clamp_setup(
        std::env::var("AI_CLUSTER_UNIT_SETUP_US")
            .ok()
            .and_then(|value| value.trim().parse::<f64>().ok()),
    )
}

/// Clamped so a bad value cannot break the prediction.
///
/// A negative setup would predict less than the pure compute time, which no unit can beat;
/// a non-finite one would poison the trace. Negative clamps to zero, non-finite or missing
/// falls back to `UNIT_SETUP_US`. Split from the env lookup so the edges are tested exactly.
fn clamp_setup(parsed: Option<f64>) -> f64 {
    match parsed {
        Some(value) if value.is_finite() => value.max(0.0),
        _ => UNIT_SETUP_US,
    }
}

/// Effective transfer rate. Falls back to 1.0 when unmeasured.
///
/// A *relative* rate, not bytes per microsecond: the worker normalises the link speed against
/// the reference link (1 GbE), so 1.0 means "a reference link" and the fallback for an
/// unmeasured node is the same number — "not measured" reads as the reference, not as the
/// worst link. The transfer term is therefore meaningful only up to that reference, the same
/// "relative, not a time" caveat the module doc states for the compute term.
fn transfer_rate(capability: &CapabilityReport) -> f64 {
    // The nominal link is the ceiling and the probe is what it delivered (C2): the smaller
    // wins. The idle remeasurement (C4) replaces the registration probe once it exists, so a
    // link that degraded after joining costs what it does now. Each zero means "not measured"
    // and drops out, so one measure alone still counts.
    let achieved = if capability.current_network_score > 0.0 {
        capability.current_network_score
    } else {
        capability.network_achieved_score
    };
    [capability.network_score, achieved]
        .into_iter()
        .filter(|rate| *rate > 0.0)
        .fold(None, |best: Option<f64>, rate| {
            Some(best.map_or(rate, |b| b.min(rate)))
        })
        .unwrap_or(1.0)
}

/// Whether `asking` should take `unit` now, or yield it to a better-placed node (G3/G5/G6).
///
/// Pure policy, split out of `WorkStore::claim_placed` so the ranking is testable without a
/// store. The store keeps the lock, the queue and the dispatch; it hands in two lookups:
/// `in_flight` (units a node already runs) and `held` (the request a node is reserved for).
///
/// A reservation outranks cost: a node held for another request is no candidate, and a unit
/// whose request holds a node belongs to that node alone — the hold is a promise that the work
/// lands there. Otherwise the asking node takes the unit when it is the cheapest idle fit, or
/// within `tolerance` of it (the hysteresis band): yielding costs up to a poll interval, so a
/// rounding-level saving is not worth it. A zero band means "strictly cheapest".
pub fn decide(
    unit: &WorkUnit,
    asking: &str,
    candidates: &[(String, CapabilityReport, HealthReport)],
    in_flight: impl Fn(&str) -> u32,
    held: impl Fn(&str) -> Option<Hold>,
    tolerance: f64,
) -> bool {
    let mut best: Option<f64> = None;
    let mut asking_cost: Option<f64> = None;
    let mut held_somewhere = false;
    let mut held_here = false;
    for (id, capability, health) in candidates {
        let in_flight = in_flight(id);
        // A node is busy only when every slot it runs units in is taken (G4).
        if in_flight >= capability.work_slots.max(1) || !fits(unit, capability) {
            continue;
        }
        match held(id) {
            // Another request holds the node: it is a candidate only while a partial hold leaves
            // a slot free beyond the held ones.
            Some(hold) if hold.request_id != unit.request_id => {
                if !hold.admits(&unit.request_id, in_flight, capability.work_slots) {
                    continue;
                }
            }
            Some(_) => {
                held_somewhere = true;
                held_here |= id == asking;
            }
            None => {}
        }
        let cost = placement_cost(
            unit,
            &Candidate {
                capability,
                health,
                in_flight,
            },
        );
        if id == asking {
            asking_cost = Some(cost);
        }
        best = Some(best.map_or(cost, |current| current.min(cost)));
    }
    if held_somewhere {
        return held_here;
    }
    match (asking_cost, best) {
        (Some(cost), Some(best)) => cost <= best * (1.0 + tolerance),
        _ => false,
    }
}

/// Bytes a unit needs resident to run: two operands read in and one result written
/// back, at `BYTES_PER_ELEMENT` each. Saturating, so an absurd shape cannot wrap.
pub fn unit_footprint_bytes(unit: &WorkUnit) -> u64 {
    let elements = u64::from(unit.rows) * u64::from(unit.shared)
        + u64::from(unit.shared) * u64::from(unit.cols)
        + u64::from(unit.rows) * u64::from(unit.cols);
    elements.saturating_mul(BYTES_PER_ELEMENT as u64)
}

/// Whether a node's reported capability can run the unit at all.
///
/// The declared `min_cpu_threads` and the unit's own footprint are **hard** filters:
/// a node that cannot run the unit never gets it, however cheap it would otherwise
/// be. `memory_bytes == 0` means the node did not report memory, so the memory
/// filter is skipped rather than read as "no RAM" — the same convention as an
/// unreported health score.
pub fn fits(unit: &WorkUnit, capability: &CapabilityReport) -> bool {
    unit.min_cpu_threads <= capability.cpu_threads
        && (capability.memory_bytes == 0 || unit_footprint_bytes(unit) <= capability.memory_bytes)
}

/// Predicted cost of running `unit` on the candidate node. Lower is better.
///
/// Terms: compute (flops over the node's rate), transfer (bytes over its network
/// rate) and queue (units already in flight). Health scales the total, so a
/// degraded node loses ties against a healthy one of equal capability. A health
/// score of 0 means "not reported" and is ignored rather than read as terrible.
///
/// Known ceiling: locality, thermal headroom, state-movement cost and deadline
/// feasibility are not modelled — the registry carries no data for them yet. See
/// `docs/system/02-subsistemas/execucao/scheduler.md`.
pub fn placement_cost(unit: &WorkUnit, candidate: &Candidate) -> f64 {
    let rows = f64::from(unit.rows);
    let shared = f64::from(unit.shared);
    let cols = f64::from(unit.cols);

    // One multiply-add per output element, so two flops per element.
    let flops = 2.0 * rows * shared * cols;
    let compute = COMPUTE_WEIGHT * flops / compute_rate(candidate.capability);

    // A and B are read in, C is written back.
    let transfer =
        TRANSFER_WEIGHT * unit_footprint_bytes(unit) as f64 / transfer_rate(candidate.capability);

    // A unit waiting behind a slot costs less on a node with many slots.
    let queue = QUEUE_WEIGHT * f64::from(candidate.in_flight)
        / f64::from(candidate.capability.work_slots.max(1));

    let health_penalty = if candidate.health.health_score > 0.0 {
        1.0 + HEALTH_WEIGHT * (1.0 - candidate.health.health_score.clamp(0.0, 1.0))
    } else {
        1.0
    };

    (compute + transfer + queue) * health_penalty
}

/// Predicted time the worker will spend computing `unit`, in microseconds (H5).
///
/// Zero when the node has neither a baseline nor a current compute score: an unmeasured node
/// has no rate to predict from, and zero reads as "not predicted" rather than as "instant" —
/// the same convention as an unreported health score. A caller comparing against actual time
/// must skip the zero rows rather than read them as a perfect prediction.
///
/// Uses the node's **current** measurement when it has one (C3) and the registration baseline
/// otherwise, so the prediction follows a node that got slower after joining instead of
/// reporting what it could do on day one. Placement ranking still uses the baseline, so the
/// prediction and the ranking answer different questions: "how long will this take now?" and
/// "which node is the best home for it?".
///
/// The cost model's compute term is `flops / compute_rate`, whose unit is only meaningful
/// up to the reference core's rate. Inverting the worker's normalisation — a core scoring
/// 1.0 runs at `REFERENCE_CORE_GFLOPS` — turns it back into time: `compute_rate` times the
/// reference is the node's throughput in GFLOP/s, and 1 GFLOP/s is 1e3 flops per microsecond.
///
/// Known ceiling: assumes compute scales linearly with thread count, the same assumption
/// the worker's benchmark makes. Turbo and SMT mean a real multithreaded run is a little
/// under the linear estimate, so the prediction runs optimistic on wide nodes.
///
/// Two corrections sit on top of the pure `flops / rate` (`predictions.md`): the rate is
/// multiplied by the benchmark's typical/best ratio (`typical_ratio`), so the prediction uses
/// the typical run instead of the luckiest one; and the per-unit setup (`setup_us`) is added,
/// which the compute term has no constant for and which is what made a small unit's
/// prediction an order of magnitude short. Zero still means "not measured" — a node without a
/// score is not a zero-microsecond prediction.
pub fn predicted_compute_us(unit: &WorkUnit, capability: &CapabilityReport) -> u64 {
    if capability.compute_score <= 0.0 && capability.current_compute_score <= 0.0 {
        return 0;
    }
    let threads = effective_threads(capability);
    let per_thread = per_thread_compute_current(capability) * typical_ratio(capability);
    let gflops = threads * per_thread * REFERENCE_CORE_GFLOPS;
    let flops = 2.0 * f64::from(unit.rows) * f64::from(unit.shared) * f64::from(unit.cols);
    (flops / (gflops * 1e3) + setup_us()).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(rows: u32, shared: u32, cols: u32) -> WorkUnit {
        WorkUnit {
            work_id: "w".to_string(),
            rows,
            shared,
            cols,
            matrix_a: Vec::new(),
            matrix_b: Vec::new(),
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
            request_id: "req-test".to_string(),
            priority: 0,
        }
    }

    fn capability(cpu_threads: u32, compute_score: f64, network_score: f64) -> CapabilityReport {
        CapabilityReport {
            cpu_threads,
            compute_score,
            network_score,
            ..Default::default()
        }
    }

    #[test]
    fn transfer_rate_is_the_smaller_of_nominal_and_achieved() {
        let rate = |nominal, achieved| {
            transfer_rate(&CapabilityReport {
                network_score: nominal,
                network_achieved_score: achieved,
                ..Default::default()
            })
        };
        assert_eq!(rate(10.0, 2.0), 2.0);
        assert_eq!(rate(1.0, 5.0), 1.0);
        assert_eq!(rate(0.0, 3.0), 3.0);
        assert_eq!(rate(4.0, 0.0), 4.0);
        assert_eq!(rate(0.0, 0.0), 1.0);
        // The remeasurement wins over the registration probe, even when it is higher.
        let with_current = |achieved, current| {
            transfer_rate(&CapabilityReport {
                network_score: 10.0,
                network_achieved_score: achieved,
                current_network_score: current,
                ..Default::default()
            })
        };
        assert_eq!(with_current(5.0, 1.5), 1.5);
        assert_eq!(with_current(2.0, 6.0), 6.0);
        assert_eq!(with_current(5.0, 0.0), 5.0);
    }

    fn pool() -> Vec<(String, CapabilityReport, HealthReport)> {
        vec![
            ("slow".to_string(), capability(4, 1.0, 1.0), health(1.0)),
            ("fast".to_string(), capability(4, 4.0, 1.0), health(1.0)),
        ]
    }

    #[test]
    fn decide_lets_the_cheapest_node_take_the_unit_and_makes_a_worse_one_yield() {
        let unit = unit(64, 64, 64);
        let idle = |_: &str| 0;
        let none = |_: &str| None;
        assert!(decide(&unit, "fast", &pool(), idle, none, 0.1));
        assert!(!decide(&unit, "slow", &pool(), idle, none, 0.1));
        // A band wide enough to cover the gap lets the worse node take it.
        assert!(decide(&unit, "slow", &pool(), idle, none, 10.0));
    }

    #[test]
    fn decide_ignores_busy_nodes_as_candidates() {
        let unit = unit(64, 64, 64);
        let busy_fast = |id: &str| u32::from(id == "fast");
        // The fast node is busy, so the slow one is the best idle fit.
        assert!(decide(
            &unit,
            "slow",
            &pool(),
            busy_fast,
            |_: &str| None,
            0.1
        ));
    }

    fn hold_of(id: &str, node: &str, request: &str, slots: u32) -> Option<Hold> {
        (id == node).then(|| Hold {
            request_id: request.to_string(),
            slots,
        })
    }

    #[test]
    fn uma_reserva_parcial_nao_tira_o_no_das_outras_requisicoes() {
        let mut unit = unit(64, 64, 64);
        unit.request_id = "req-b".to_string();
        let mut pool = pool();
        // 4 slots em cada nó; o rápido tem 1 guardado para req-a
        for (_, capability, _) in &mut pool {
            capability.work_slots = 4;
        }
        let idle = |_: &str| 0;
        let partial = |id: &str| hold_of(id, "fast", "req-a", 1);
        // sobra slot além do guardado: o nó rápido continua sendo o melhor lugar para req-b
        assert!(decide(&unit, "fast", &pool, idle, partial, 0.1));
        // três em voo + um guardado = quatro: sem folga, o nó sai e o lento assume
        let three = |id: &str| u32::from(id == "fast") * 3;
        assert!(!decide(&unit, "fast", &pool, three, partial, 0.1));
        assert!(decide(&unit, "slow", &pool, three, partial, 0.1));
        // reserva do nó inteiro: o rápido nunca é candidato para req-b
        let whole = |id: &str| hold_of(id, "fast", "req-a", 0);
        assert!(!decide(&unit, "fast", &pool, idle, whole, 0.1));
    }

    #[test]
    fn decide_gives_a_held_unit_only_to_the_held_node() {
        let mut unit = unit(64, 64, 64);
        unit.request_id = "req-a".to_string();
        let idle = |_: &str| 0;
        let held_slow = |id: &str| hold_of(id, "slow", "req-a", 0);
        // The hold outranks cost: the cheaper node must yield to the held one.
        assert!(decide(&unit, "slow", &pool(), idle, held_slow, 0.1));
        assert!(!decide(&unit, "fast", &pool(), idle, held_slow, 0.1));
        // A node held for another request is no candidate for this unit.
        let held_other = |id: &str| hold_of(id, "fast", "req-b", 0);
        assert!(!decide(&unit, "fast", &pool(), idle, held_other, 0.1));
        assert!(decide(&unit, "slow", &pool(), idle, held_other, 0.1));
    }

    #[test]
    fn a_node_with_free_slots_is_still_a_candidate() {
        let unit = unit(64, 64, 64);
        let mut pool = pool();
        for (_, capability, _) in &mut pool {
            capability.work_slots = 2;
        }
        let one_running = |id: &str| u32::from(id == "fast");
        // The fast node runs one of its two slots: it still takes the unit, so the slow one yields.
        assert!(decide(
            &unit,
            "fast",
            &pool,
            one_running,
            |_: &str| None,
            0.1
        ));
        assert!(!decide(
            &unit,
            "slow",
            &pool,
            one_running,
            |_: &str| None,
            0.1
        ));
        let two_running = |id: &str| 2 * u32::from(id == "fast");
        // With both slots taken it is busy and the slow node is the best idle fit.
        assert!(decide(
            &unit,
            "slow",
            &pool,
            two_running,
            |_: &str| None,
            0.1
        ));
    }

    fn health(health_score: f64) -> HealthReport {
        HealthReport {
            health_score,
            ..Default::default()
        }
    }

    fn cost(
        capability: &CapabilityReport,
        health: &HealthReport,
        in_flight: u32,
        unit: &WorkUnit,
    ) -> f64 {
        placement_cost(
            unit,
            &Candidate {
                capability,
                health,
                in_flight,
            },
        )
    }

    #[test]
    fn more_threads_costs_less() {
        let u = unit(64, 64, 64);
        let weak = capability(2, 0.0, 0.0);
        let strong = capability(8, 0.0, 0.0);
        assert!(cost(&strong, &health(1.0), 0, &u) < cost(&weak, &health(1.0), 0, &u));
    }

    #[test]
    fn a_bigger_unit_costs_more() {
        let cap = capability(4, 0.0, 0.0);
        let small = unit(32, 32, 32);
        let large = unit(128, 128, 128);
        assert!(cost(&cap, &health(1.0), 0, &large) > cost(&cap, &health(1.0), 0, &small));
    }

    #[test]
    fn a_queued_node_costs_more_than_an_idle_one() {
        let u = unit(64, 64, 64);
        let cap = capability(4, 0.0, 0.0);
        assert!(cost(&cap, &health(1.0), 3, &u) > cost(&cap, &health(1.0), 0, &u));
    }

    #[test]
    fn degraded_health_costs_more() {
        let u = unit(64, 64, 64);
        let cap = capability(4, 0.0, 0.0);
        assert!(cost(&cap, &health(0.25), 0, &u) > cost(&cap, &health(1.0), 0, &u));
    }

    #[test]
    fn unreported_health_does_not_penalise() {
        let u = unit(64, 64, 64);
        let cap = capability(4, 0.0, 0.0);
        assert_eq!(
            cost(&cap, &health(0.0), 0, &u),
            cost(&cap, &health(1.0), 0, &u)
        );
    }

    #[test]
    fn memoria_que_satura_vale_menos_que_a_contagem_de_threads() {
        let mut cap = capability(4, 1.0, 1.0);
        let unmeasured = compute_rate(&cap);
        // 4 threads copiando juntas dão só 2x uma thread: a memória escala pela metade
        cap.memory_bandwidth_score = 1.0;
        cap.memory_bandwidth_loaded_score = 2.0;
        assert!((memory_scaling(&cap) - 0.5).abs() < 1e-9);
        assert!((effective_threads(&cap) - 2.5).abs() < 1e-9);
        assert!(compute_rate(&cap) < unmeasured);
        // memória que escala, ou ruído acima do linear, não passa de 1
        cap.memory_bandwidth_loaded_score = 9.0;
        assert_eq!(memory_scaling(&cap), 1.0);
        // sem a medida sob carga, ou com uma thread só, nada muda
        cap.memory_bandwidth_loaded_score = 0.0;
        assert_eq!(compute_rate(&cap), unmeasured);
        let mut one = capability(1, 1.0, 1.0);
        one.memory_bandwidth_score = 1.0;
        one.memory_bandwidth_loaded_score = 0.1;
        assert_eq!(effective_threads(&one), 1.0);
        // o piso evita que uma medida ruim zere o nó
        let mut bad = capability(4, 1.0, 1.0);
        bad.memory_bandwidth_score = 1.0;
        bad.memory_bandwidth_loaded_score = 0.01;
        assert_eq!(memory_scaling(&bad), 0.25);
    }

    #[test]
    fn a_measured_compute_score_beats_the_thread_fallback() {
        let u = unit(64, 64, 64);
        let measured = capability(4, 2.0, 0.0);
        let fallback = capability(4, 0.0, 0.0);
        assert!(cost(&measured, &health(1.0), 0, &u) < cost(&fallback, &health(1.0), 0, &u));
    }

    #[test]
    fn a_deep_queue_outweighs_a_thread_advantage() {
        // The queue term is what keeps a busy but nominally faster node from being
        // chosen: enough units already in flight must be able to flip the comparison.
        let u = unit(16, 16, 16);
        let busy = capability(8, 0.0, 0.0);
        let idle = capability(4, 0.0, 0.0);
        assert!(cost(&idle, &health(1.0), 0, &u) < cost(&busy, &health(1.0), 40, &u));
    }

    #[test]
    fn footprint_is_the_three_matrices_in_bytes() {
        // 2x2x2: (4 + 4 + 4) elements * 8 bytes per element.
        assert_eq!(unit_footprint_bytes(&unit(2, 2, 2)), 96);
    }

    #[test]
    fn unreported_memory_does_not_filter_a_unit_out() {
        // CapabilityReport::default() leaves memory_bytes at 0, meaning "not reported".
        assert!(fits(&unit(64, 64, 64), &capability(2, 0.0, 0.0)));
    }

    #[test]
    fn a_unit_that_does_not_fit_the_reported_memory_is_excluded() {
        let mut cap = capability(8, 0.0, 0.0);
        cap.memory_bytes = 1_000;
        assert!(fits(&unit(2, 2, 2), &cap)); // 96 bytes
        assert!(!fits(&unit(64, 64, 64), &cap)); // 98_304 bytes
    }

    #[test]
    fn the_prediction_matches_the_reference_rate() {
        // 64x64x64 is 524_288 flops. One core at the reference rate (6 GFLOP/s, i.e.
        // 6_000 flops/us) needs 524_288 / 6_000 = 87us. A score of 1.0 on one thread is
        // exactly that core, so the compute term has to land on 87 — this pins the whole
        // normalisation-inversion chain, including the shared reference constant — and the
        // per-unit setup is added on top, which is what a bare compute term cannot express.
        let u = unit(64, 64, 64);
        assert_eq!(
            predicted_compute_us(&u, &capability(1, 1.0, 0.0)),
            87 + UNIT_SETUP_US as u64
        );
    }

    #[test]
    fn more_threads_predicts_less_time() {
        let u = unit(64, 64, 64);
        let weak = predicted_compute_us(&u, &capability(2, 1.0, 0.0));
        let strong = predicted_compute_us(&u, &capability(8, 1.0, 0.0));
        assert!(strong < weak, "strong={strong} weak={weak}");
    }

    #[test]
    fn a_bigger_unit_predicts_more_time() {
        let cap = capability(4, 1.0, 0.0);
        assert!(
            predicted_compute_us(&unit(128, 128, 128), &cap)
                > predicted_compute_us(&unit(32, 32, 32), &cap)
        );
    }

    #[test]
    fn an_unmeasured_node_predicts_nothing() {
        // compute_score 0 means "not measured", so there is no rate to predict from.
        // Zero reads as "not predicted", never as a zero-microsecond prediction.
        let u = unit(64, 64, 64);
        assert_eq!(predicted_compute_us(&u, &capability(4, 0.0, 0.0)), 0);
    }

    #[test]
    fn a_current_score_overrides_the_baseline_in_the_prediction() {
        // Baseline 1.0, current 2.0: the node is twice as fast as when it registered, so the
        // prediction has to halve. The ranking still reads the baseline — this is only the
        // time estimate following what the node is doing now (C3).
        let u = unit(64, 64, 64);
        let mut cap = capability(1, 1.0, 0.0);
        assert_eq!(predicted_compute_us(&u, &cap), 687);
        cap.current_compute_score = 2.0;
        assert_eq!(predicted_compute_us(&u, &cap), 644);
    }

    #[test]
    fn a_decayed_node_predicts_more_than_its_baseline_suggests() {
        // Baseline 2.0, current 0.5: the node got slower after joining. A time estimate that
        // kept reporting day-one speed would under-predict the run by 4x.
        let u = unit(64, 64, 64);
        let mut cap = capability(1, 2.0, 0.0);
        assert_eq!(predicted_compute_us(&u, &cap), 644);
        cap.current_compute_score = 0.5;
        assert_eq!(predicted_compute_us(&u, &cap), 775);
    }

    #[test]
    fn a_current_score_alone_still_predicts() {
        // The registration benchmark failed (baseline 0) but the idle remeasurement succeeded.
        // A node must not lose its prediction because the first measurement failed.
        let u = unit(64, 64, 64);
        let mut cap = capability(1, 0.0, 0.0);
        assert_eq!(predicted_compute_us(&u, &cap), 0);
        cap.current_compute_score = 1.0;
        assert_eq!(predicted_compute_us(&u, &cap), 687);
    }

    #[test]
    fn a_trustworthy_median_pulls_the_prediction_longer() {
        // Best 1.0, median 0.5: the machine typically runs at half its luckiest rate, so the
        // compute term doubles. 524_288 / (0.5 * 6_000) = 175us, plus the setup.
        let u = unit(64, 64, 64);
        let mut cap = capability(1, 1.0, 0.0);
        cap.compute_median_score = 0.5;
        cap.compute_deviation = 0.05;
        assert_eq!(predicted_compute_us(&u, &cap), 175 + UNIT_SETUP_US as u64);
    }

    #[test]
    fn an_untrustworthy_median_is_ignored() {
        // Same median, but the runs disagreed past the trust limit: the ratio is noise, so
        // the best case is used unchanged rather than a ratio the benchmark cannot support.
        let u = unit(64, 64, 64);
        let mut cap = capability(1, 1.0, 0.0);
        cap.compute_median_score = 0.5;
        cap.compute_deviation = DEVIATION_TRUST_LIMIT + 0.01;
        assert_eq!(predicted_compute_us(&u, &cap), 87 + UNIT_SETUP_US as u64);
    }

    #[test]
    fn a_node_without_a_median_predicts_from_the_best_case() {
        // A pinned score runs no benchmark, so there is no median and therefore no ratio.
        let u = unit(64, 64, 64);
        let cap = capability(1, 1.0, 0.0);
        assert_eq!(typical_ratio(&cap), 1.0);
        assert_eq!(predicted_compute_us(&u, &cap), 87 + UNIT_SETUP_US as u64);
    }

    #[test]
    fn the_setup_clamps_to_something_sane() {
        // A negative setup would predict less than the pure compute time, which no unit can
        // beat; a non-finite one would poison every trace.
        assert_eq!(clamp_setup(None), UNIT_SETUP_US);
        assert_eq!(clamp_setup(Some(f64::NAN)), UNIT_SETUP_US);
        assert_eq!(clamp_setup(Some(f64::INFINITY)), UNIT_SETUP_US);
        assert_eq!(clamp_setup(Some(-5.0)), 0.0);
        assert_eq!(clamp_setup(Some(1_234.0)), 1_234.0);
    }
}
