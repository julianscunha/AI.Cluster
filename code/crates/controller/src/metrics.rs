//! Aggregate cluster metrics (Fase H1).
//!
//! The controller already held everything here: node counts in the registry, queue and
//! settlement counts in the work store. What was missing was a single read that rolls them
//! up into the numbers a dashboard or a `status` wants. The arithmetic lives here, apart
//! from the locks, so averages and the empty-cluster case are tested directly.

use ai_cluster_protocol::cluster::MetricsResponse;

use crate::work::CompletedWork;

/// Rolls registry and work state up into the `GetMetrics` response.
///
/// Pure: takes the counts and the completed records, returns the message. The service does
/// the reading, so this can be tested without spinning up a registry or a work store.
///
/// Averages are integer division, so a fractional microsecond is dropped. Zero completions
/// yields zero averages rather than a division by zero — the cluster has simply done
/// nothing yet, which is a valid state, not an error.
pub fn summarize(
    nodes_total: u32,
    nodes_available: u32,
    completed: &[CompletedWork],
    units_pending: u32,
    units_blocked: u32,
    units_failed: u32,
) -> MetricsResponse {
    let mut total_compute_time_us: u64 = 0;
    let mut total_latency_us: u64 = 0;
    for record in completed {
        // Saturating: a hostile or buggy worker could report a compute time near u64::MAX,
        // and a wrap would show a cluster that did almost no work as having done enormous
        // work. Clamping overstates the total instead, which is the safer direction.
        total_compute_time_us = total_compute_time_us.saturating_add(record.compute_time_us);
        total_latency_us = total_latency_us.saturating_add(record.total_latency_us);
    }

    // `checked_div` returns None when nothing has completed, so the empty cluster reads as
    // zero averages without a separate guard.
    let count = completed.len() as u64;
    let avg_compute_time_us = total_compute_time_us.checked_div(count).unwrap_or(0);
    let avg_latency_us = total_latency_us.checked_div(count).unwrap_or(0);

    MetricsResponse {
        nodes_total,
        nodes_available,
        units_completed: completed.len() as u32,
        units_pending,
        units_blocked,
        units_failed,
        total_compute_time_us,
        total_latency_us,
        avg_compute_time_us,
        avg_latency_us,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed(compute_time_us: u64, total_latency_us: u64) -> CompletedWork {
        CompletedWork {
            work_id: "w".to_string(),
            node_id: "n".to_string(),
            request_id: "req-test".to_string(),
            compute_time_us,
            total_latency_us,
            transfer_time_us: total_latency_us.saturating_sub(compute_time_us),
            checksum: 0.0,
        }
    }

    #[test]
    fn sums_and_averages_over_completed_units() {
        let records = [completed(100, 250), completed(300, 550)];
        let metrics = summarize(2, 1, &records, 3, 4, 5);

        assert_eq!(metrics.nodes_total, 2);
        assert_eq!(metrics.nodes_available, 1);
        assert_eq!(metrics.units_completed, 2);
        assert_eq!(metrics.units_pending, 3);
        assert_eq!(metrics.units_blocked, 4);
        assert_eq!(metrics.units_failed, 5);
        assert_eq!(metrics.total_compute_time_us, 400);
        assert_eq!(metrics.total_latency_us, 800);
        assert_eq!(metrics.avg_compute_time_us, 200);
        assert_eq!(metrics.avg_latency_us, 400);
    }

    #[test]
    fn an_empty_cluster_reports_zero_instead_of_dividing_by_zero() {
        let metrics = summarize(0, 0, &[], 0, 0, 0);

        assert_eq!(metrics.units_completed, 0);
        assert_eq!(metrics.avg_compute_time_us, 0);
        assert_eq!(metrics.avg_latency_us, 0);
    }

    #[test]
    fn averages_round_down_rather_than_up() {
        // 201/2 = 100.5; truncation must yield 100, not round up to 101 and claim more
        // work was done per unit than was.
        let records = [completed(100, 101), completed(101, 200)];
        let metrics = summarize(2, 2, &records, 0, 0, 0);
        assert_eq!(metrics.total_compute_time_us, 201);
        assert_eq!(metrics.avg_compute_time_us, 100);
        assert_eq!(metrics.total_latency_us, 301);
        assert_eq!(metrics.avg_latency_us, 150);
    }

    #[test]
    fn a_nonsensical_timing_saturates_instead_of_wrapping() {
        // A worker reporting u64::MAX compute time must not wrap the total to near zero.
        let records = [completed(u64::MAX, u64::MAX), completed(1_000, 2_000)];
        let metrics = summarize(1, 1, &records, 0, 0, 0);

        assert_eq!(metrics.total_compute_time_us, u64::MAX);
        assert_eq!(metrics.total_latency_us, u64::MAX);
        // The average is taken from the clamped total, so it stays finite and large.
        assert_eq!(metrics.avg_compute_time_us, u64::MAX / 2);
    }
}
