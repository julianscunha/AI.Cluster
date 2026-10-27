//! Aggregate cluster metrics (Fase H1).
//!
//! The controller already held everything here: node counts in the registry, queue and
//! settlement counts in the work store. What was missing was a single read that rolls them
//! up into the numbers a dashboard or a `status` wants. The arithmetic lives here, apart
//! from the locks, so averages and the empty-cluster case are tested directly.

use ai_cluster_protocol::cluster::{MetricsResponse, NodeSnapshot};

use crate::work::CompletedWork;

/// Nearest-rank percentile of `values`: the smallest value at or above rank `ceil(p * n)`.
///
/// A percentile, not an interpolation: the answer is always a value that actually happened,
/// which is what an operator wants to read off a latency tail. An empty slice reports zero
/// rather than panicking, so an idle cluster needs no separate guard.
///
/// ponytail: sorts a copy per call, so this is O(n log n) over the units the controller
/// still retains. Fine while that ring is small; keep a running histogram if it grows.
fn percentile(values: &[u64], p: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (p * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

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
    let mut total_predicted_compute_us: u64 = 0;
    for record in completed {
        // Saturating: a hostile or buggy worker could report a compute time near u64::MAX,
        // and a wrap would show a cluster that did almost no work as having done enormous
        // work. Clamping overstates the total instead, which is the safer direction.
        total_compute_time_us = total_compute_time_us.saturating_add(record.compute_time_us);
        total_latency_us = total_latency_us.saturating_add(record.total_latency_us);
        // Zero predicted means "not predicted", so an unpredicted unit adds nothing here
        // while its real time still counts above. The gap the two totals show is honest:
        // the cluster-wide version of the per-unit `erro_pct` in the trace (H5).
        total_predicted_compute_us =
            total_predicted_compute_us.saturating_add(record.predicted_compute_us);
    }

    // `checked_div` returns None when nothing has completed, so the empty cluster reads as
    // zero averages without a separate guard.
    let count = completed.len() as u64;
    let avg_compute_time_us = total_compute_time_us.checked_div(count).unwrap_or(0);
    let avg_latency_us = total_latency_us.checked_div(count).unwrap_or(0);

    // Percentiles need the individual times, not the sum: an average hides the tail, and
    // the tail is what a client waiting on one request feels.
    let compute_times: Vec<u64> = completed
        .iter()
        .map(|record| record.compute_time_us)
        .collect();
    let latencies: Vec<u64> = completed
        .iter()
        .map(|record| record.total_latency_us)
        .collect();

    MetricsResponse {
        nodes_total,
        nodes_available,
        units_completed: completed.len() as u32,
        units_pending,
        units_blocked,
        units_failed,
        total_compute_time_us,
        total_latency_us,
        total_predicted_compute_us,
        avg_compute_time_us,
        avg_latency_us,
        p50_compute_time_us: percentile(&compute_times, 0.50),
        p95_compute_time_us: percentile(&compute_times, 0.95),
        p50_latency_us: percentile(&latencies, 0.50),
        p95_latency_us: percentile(&latencies, 0.95),
        ..Default::default()
    }
}

/// Média dos valores positivos: zero quando nenhum nó mediu.
fn mean_measured(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, count) = values
        .filter(|value| value.is_finite() && *value > 0.0)
        .fold((0.0, 0u32), |(sum, count), value| (sum + value, count + 1));
    if count == 0 {
        0.0
    } else {
        sum / f64::from(count)
    }
}

/// Unidades concluídas por segundo na janela `[now - window_ms, now]`. Zero enquanto o histórico
/// não cobre a janela inteira (a unidade mais antiga que o controller guarda é mais nova que o
/// início da janela): extrapolar de pouca história inventaria uma taxa.
fn units_per_second(completed: &[CompletedWork], now_ms: u64, window_ms: u64) -> f64 {
    let start = now_ms.saturating_sub(window_ms);
    let oldest = completed.iter().map(|c| c.completed_at_unix_ms).min();
    match oldest {
        Some(oldest) if oldest <= start => {
            let in_window = completed
                .iter()
                .filter(|c| c.completed_at_unix_ms > start)
                .count();
            in_window as f64 / (window_ms as f64 / 1000.0)
        }
        _ => 0.0,
    }
}

/// Acrescenta ao roll-up o que depende dos nós (rede e storage) e do tempo (taxas por segundo).
/// Separado de [`summarize`] para que ele continue puro sobre contagens e unidades.
pub fn add_rollups(
    metrics: &mut MetricsResponse,
    nodes: &[NodeSnapshot],
    completed: &[CompletedWork],
    now_ms: u64,
) {
    let ready: Vec<_> = nodes
        .iter()
        .filter(|node| node.trusted && node.state == "READY")
        .filter_map(|node| node.capability.as_ref())
        .collect();
    metrics.avg_network_score = mean_measured(ready.iter().map(|c| c.network_score));
    metrics.avg_storage_score = mean_measured(ready.iter().map(|c| c.storage_score));
    metrics.avg_storage_read_score = mean_measured(ready.iter().map(|c| c.storage_read_score));
    metrics.avg_storage_latency_ms = mean_measured(ready.iter().map(|c| c.storage_latency_ms));
    metrics.units_per_second_1m = units_per_second(completed, now_ms, 60_000);
    metrics.units_per_second_5m = units_per_second(completed, now_ms, 300_000);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_roll_up_de_rede_e_storage_ignora_quem_nao_mediu_e_quem_nao_esta_pronto() {
        use ai_cluster_protocol::cluster::CapabilityReport;
        let node = |state: &str, trusted: bool, network: f64, storage: f64| NodeSnapshot {
            state: state.into(),
            trusted,
            capability: Some(CapabilityReport {
                network_score: network,
                storage_score: storage,
                ..Default::default()
            }),
            ..Default::default()
        };
        let nodes = [
            node("READY", true, 1.0, 0.4),
            node("READY", true, 3.0, 0.0),
            node("OFFLINE", true, 100.0, 100.0),
            node("READY", false, 100.0, 100.0),
        ];
        let mut metrics = MetricsResponse::default();
        add_rollups(&mut metrics, &nodes, &[], 1_000);
        assert_eq!(metrics.avg_network_score, 2.0, "média de 1 e 3");
        assert_eq!(metrics.avg_storage_score, 0.4, "o zero não puxa a média");
        assert_eq!(metrics.avg_storage_read_score, 0.0, "ninguém mediu");
    }

    #[test]
    fn a_taxa_por_segundo_so_vale_com_historico_que_cobre_a_janela() {
        let at = |ms| CompletedWork {
            completed_at_unix_ms: ms,
            ..completed(1, 1)
        };
        // 3 unidades nos últimos 60 s, e uma antiga (150 s antes) que prova a história
        let now = 400_000;
        let history = [at(250_000), at(350_000), at(360_000), at(390_000)];
        let mut metrics = MetricsResponse::default();
        add_rollups(&mut metrics, &[], &history, now);
        assert!((metrics.units_per_second_1m - 3.0 / 60.0).abs() < 1e-9);
        // a janela de 5 min começa em 100 000 e a unidade mais antiga é de 250 000: sem história
        assert_eq!(metrics.units_per_second_5m, 0.0);
        let mut empty = MetricsResponse::default();
        add_rollups(&mut empty, &[], &[], now);
        assert_eq!(empty.units_per_second_1m, 0.0);
    }

    fn completed(compute_time_us: u64, total_latency_us: u64) -> CompletedWork {
        CompletedWork {
            completed_at_unix_ms: 0,
            work_id: "w".to_string(),
            node_id: "n".to_string(),
            request_id: "req-test".to_string(),
            compute_time_us,
            total_latency_us,
            transfer_time_us: total_latency_us.saturating_sub(compute_time_us),
            // These tests cover the compute/latency roll-up, not the prediction (H5), so
            // the predicted time is left unset. Zero reads as "not predicted".
            predicted_compute_us: 0,
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
    fn sums_the_prediction_beside_the_measurement() {
        // A predicted unit contributes to the predicted total; an unpredicted one (zero)
        // does not, while its measured time still counts — the two totals answer "what the
        // model expected" and "what happened" over the same set of completed units.
        let predicted = CompletedWork {
            predicted_compute_us: 30,
            ..completed(100, 250)
        };
        let records = [predicted, completed(300, 550)];
        let metrics = summarize(2, 2, &records, 0, 0, 0);

        assert_eq!(metrics.total_compute_time_us, 400);
        assert_eq!(metrics.total_predicted_compute_us, 30);
    }

    #[test]
    fn an_empty_cluster_reports_zero_instead_of_dividing_by_zero() {
        let metrics = summarize(0, 0, &[], 0, 0, 0);

        assert_eq!(metrics.units_completed, 0);
        assert_eq!(metrics.avg_compute_time_us, 0);
        assert_eq!(metrics.avg_latency_us, 0);
        assert_eq!(metrics.p50_compute_time_us, 0);
        assert_eq!(metrics.p95_latency_us, 0);
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        assert_eq!(percentile(&[], 0.50), 0);
        // One value: every percentile is that value.
        assert_eq!(percentile(&[7], 0.50), 7);
        assert_eq!(percentile(&[7], 0.95), 7);
        // 1..=10: p50 is rank ceil(5) = 5, so the 5th smallest; p95 is rank ceil(9.5) = 10.
        let values: Vec<u64> = (1..=10).collect();
        assert_eq!(percentile(&values, 0.50), 5);
        assert_eq!(percentile(&values, 0.95), 10);
        // The answer is always a value that actually happened, never an interpolation.
        assert_eq!(percentile(&[10, 20], 0.50), 10);
        assert_eq!(percentile(&[10, 20], 0.95), 20);
        // Input order is irrelevant: the copy is sorted, the caller's slice is untouched.
        assert_eq!(percentile(&[30, 10, 20], 0.50), 20);
    }

    #[test]
    fn summarize_reports_percentiles_beside_the_average() {
        // Latency spread wider than compute, so the two tails are distinguishable.
        let records = [
            completed(100, 100),
            completed(200, 200),
            completed(300, 900),
        ];
        let metrics = summarize(1, 1, &records, 0, 0, 0);

        assert_eq!(metrics.avg_compute_time_us, 200);
        assert_eq!(metrics.p50_compute_time_us, 200);
        assert_eq!(metrics.p95_compute_time_us, 300);
        assert_eq!(metrics.p50_latency_us, 200);
        assert_eq!(metrics.p95_latency_us, 900);
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
