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

use ai_cluster_protocol::cluster::{CapabilityReport, HealthReport, WorkUnit};

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
    let threads = f64::from(capability.cpu_threads).max(1.0);
    let per_thread = if capability.compute_score > 0.0 {
        capability.compute_score
    } else {
        1.0
    };
    threads * per_thread
}

/// Effective transfer rate. Falls back to 1.0 when unmeasured.
fn transfer_rate(capability: &CapabilityReport) -> f64 {
    if capability.network_score > 0.0 {
        capability.network_score
    } else {
        1.0
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
/// `implementation/phase-2/placement.md`.
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

    let queue = QUEUE_WEIGHT * f64::from(candidate.in_flight);

    let health_penalty = if candidate.health.health_score > 0.0 {
        1.0 + HEALTH_WEIGHT * (1.0 - candidate.health.health_score.clamp(0.0, 1.0))
    } else {
        1.0
    };

    (compute + transfer + queue) * health_penalty
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
}
