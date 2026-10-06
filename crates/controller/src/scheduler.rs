use real_ai_cluster_protocol::v1::NodeInfo;

#[derive(Debug, Clone, Copy)]
pub struct SchedulerWeights {
    pub capacity: f64,
    pub health: f64,
    pub load: f64,
    pub network: f64,
}

impl Default for SchedulerWeights {
    fn default() -> Self {
        Self {
            capacity: 0.50,
            health: 0.25,
            load: 0.15,
            network: 0.10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RankedWorker {
    pub node: NodeInfo,
    pub score: f64,
}

pub fn rank_workers(candidates: Vec<NodeInfo>) -> Vec<NodeInfo> {
    rank_workers_with_scores(candidates)
        .into_iter()
        .map(|worker| worker.node)
        .collect()
}

pub fn rank_workers_with_scores(candidates: Vec<NodeInfo>) -> Vec<RankedWorker> {
    let weights = SchedulerWeights::default();

    let max_compute = candidates
        .iter()
        .map(compute_capacity)
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let max_cores = candidates
        .iter()
        .map(|node| {
            node.capability
                .as_ref()
                .map(|capability| capability.cpu_logical_cores)
                .unwrap_or_default()
        })
        .max()
        .unwrap_or_default()
        .max(1);

    let min_rtt_us = candidates
        .iter()
        .filter_map(|node| {
            node.capability
                .as_ref()
                .map(|capability| capability.network_rtt_us)
                .filter(|rtt| *rtt > 0.0)
        })
        .fold(f64::INFINITY, f64::min);

    let max_network_mb_per_sec = candidates
        .iter()
        .filter_map(|node| {
            node.capability
                .as_ref()
                .map(|capability| capability.network_mb_per_sec)
                .filter(|throughput| *throughput > 0.0)
        })
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let mut ranked: Vec<RankedWorker> = candidates
        .into_iter()
        .map(|node| {
            let score = score_worker(
                &node,
                max_compute,
                max_cores,
                min_rtt_us,
                max_network_mb_per_sec,
                weights,
            );

            RankedWorker { node, score }
        })
        .collect();

    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    ranked
}

fn compute_capacity(node: &NodeInfo) -> f64 {
    node.capability
        .as_ref()
        .map(|capability| {
            if capability.cpu_ops_per_sec > 0.0 {
                capability.cpu_ops_per_sec
            } else {
                f64::from(capability.cpu_logical_cores)
            }
        })
        .unwrap_or(0.0)
}

fn score_worker(
    node: &NodeInfo,
    max_compute: f64,
    max_cores: u32,
    min_rtt_us: f64,
    max_network_mb_per_sec: f64,
    weights: SchedulerWeights,
) -> f64 {
    let capability = node.capability.as_ref();
    let capacity = compute_capacity(node);
    let compute_score = capacity / max_compute.max(1.0);
    let core_score = capability
        .map(|value| f64::from(value.cpu_logical_cores) / f64::from(max_cores))
        .unwrap_or(0.0);
    let capacity_score = if capacity > 0.0 {
        compute_score
    } else {
        core_score
    };

    let health_score = node
        .health
        .as_ref()
        .map(|health| health.overall.clamp(0.0, 1.0))
        .unwrap_or(1.0);

    let load_score = 1.0 / (1.0 + f64::from(node.active_work_units));

    let network_score = capability
        .map(|value| {
            let latency_score = if value.network_rtt_us > 0.0 && min_rtt_us.is_finite() {
                (min_rtt_us / value.network_rtt_us).clamp(0.0, 1.0)
            } else {
                0.5
            };

            let throughput_score = if value.network_mb_per_sec > 0.0 {
                (value.network_mb_per_sec / max_network_mb_per_sec.max(1.0)).clamp(0.0, 1.0)
            } else {
                0.5
            };

            (latency_score * 0.5) + (throughput_score * 0.5)
        })
        .unwrap_or(0.5);

    (capacity_score * weights.capacity)
        + (health_score * weights.health)
        + (load_score * weights.load)
        + (network_score * weights.network)
}

#[cfg(test)]
mod tests {
    use super::rank_workers;
    use real_ai_cluster_protocol::v1::{Capability, Health, NodeInfo};

    fn node(
        node_id: &str,
        cpu_ops_per_sec: f64,
        active_work_units: u32,
        network_rtt_us: f64,
        network_mb_per_sec: f64,
    ) -> NodeInfo {
        NodeInfo {
            node_id: node_id.into(),
            capability: Some(Capability {
                execution_backends: vec!["cpu".into()],
                cpu_logical_cores: 4,
                cpu_ops_per_sec,
                network_rtt_us,
                network_mb_per_sec,
                ..Default::default()
            }),
            health: Some(Health {
                overall: 1.0,
                ..Default::default()
            }),
            active_work_units,
            ..Default::default()
        }
    }

    #[test]
    fn rank_calcula_score_com_um_unico_node() {
        let ranked = rank_workers_with_scores(vec![node("single", 100.0, 0, 100.0, 100.0)]);

        assert_eq!(ranked.len(), 1);
        assert!(ranked[0].score > 0.0);
    }

    #[test]
    fn rank_prioriza_capacidade() {
        let ranked = rank_workers(vec![
            node("slow", 100.0, 0, 100.0, 100.0),
            node("fast", 200.0, 0, 100.0, 100.0),
        ]);

        assert_eq!(ranked[0].node_id, "fast");
    }

    #[test]
    fn rank_penaliza_carga_ativa() {
        let ranked = rank_workers(vec![
            node("busy", 100.0, 3, 100.0, 100.0),
            node("idle", 100.0, 0, 100.0, 100.0),
        ]);

        assert_eq!(ranked[0].node_id, "idle");
    }

    #[test]
    fn rank_prefere_menor_rtt() {
        let ranked = rank_workers(vec![
            node("far", 100.0, 0, 500.0, 100.0),
            node("near", 100.0, 0, 100.0, 100.0),
        ]);

        assert_eq!(ranked[0].node_id, "near");
    }

    #[test]
    fn rank_considera_throughput() {
        let ranked = rank_workers(vec![
            node("slow-net", 100.0, 0, 100.0, 50.0),
            node("fast-net", 100.0, 0, 100.0, 200.0),
        ]);

        assert_eq!(ranked[0].node_id, "fast-net");
    }
}
