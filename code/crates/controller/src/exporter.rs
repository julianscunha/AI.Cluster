//! Prometheus exporter (Fase H1).
//!
//! `GetMetrics` is a read a client has to ask for. A scraper cannot ask: it polls an
//! endpoint on its own schedule. This serves the same roll-up, plus the per-node signals,
//! in the text exposition format, so a Prometheus server can watch the cluster without
//! anyone writing a bridge.
//!
//! Names are prefixed `ai_cluster_` and otherwise match the `MetricsResponse` field names,
//! so a number read here and the same number read from `ai-cluster metrics` are recognisably
//! the same thing.

use std::sync::Arc;

use ai_cluster_protocol::cluster::{MetricsResponse, NodeSnapshot};
use ai_cluster_protocol::metadata::same_token;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::get;
use axum::{Router, response::IntoResponse};

use crate::metrics;
use crate::registry::Registry;
use crate::work::WorkStore;

#[derive(Clone)]
struct ExporterState {
    registry: Arc<Registry>,
    work: Arc<WorkStore>,
    /// Bearer token a scraper must present (S5): the operator token, which is what Prometheus's
    /// `authorization` setting carries. Read-only data, but the port was open to the network.
    token: Arc<str>,
}

/// Whether the request carries `Authorization: Bearer <token>`.
fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    !token.is_empty() && same_token(token.as_bytes(), given.as_bytes())
}

/// Serves `GET /metrics` on `addr` until the process ends.
///
/// A bind failure is returned rather than swallowed: a scraper silently getting nothing is
/// worse than a controller that says why it could not start the endpoint.
pub async fn serve(
    addr: String,
    registry: Arc<Registry>,
    work: Arc<WorkStore>,
    token: Arc<str>,
) -> Result<(), std::io::Error> {
    let app = Router::new()
        .route("/metrics", get(handle))
        .with_state(ExporterState {
            registry,
            work,
            token,
        });
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await
}

async fn handle(State(state): State<ExporterState>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&headers, &state.token) {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "token ausente ou inválido
"
            .to_string(),
        )
            .into_response();
    }
    let (nodes_total, nodes_available) = state.registry.counts();
    let (pending, blocked, failed) = (
        state.work.pending() as u32,
        state.work.blocked() as u32,
        state.work.failed() as u32,
    );
    let nodes = state.registry.list();
    let summary = state.work.with_completed(|completed| {
        let mut summary = metrics::summarize(
            nodes_total,
            nodes_available,
            completed,
            pending,
            blocked,
            failed,
        );
        metrics::add_rollups(&mut summary, &nodes, completed, crate::registry::unix_ms());
        summary
    });
    let body = render(&summary, &nodes);
    ([("content-type", "text/plain; version=0.0.4")], body).into_response()
}

/// Writes one header block for a metric, then a sample per node carrying a `node` label.
///
/// The `HELP`/`TYPE` lines go out once, not per sample: a strict Prometheus parser rejects a
/// metric whose type is declared twice, and the obvious loop-the-nodes-and-print-everything
/// shape does exactly that.
fn node_metric(
    out: &mut String,
    name: &str,
    help: &str,
    nodes: &[NodeSnapshot],
    value: impl Fn(&NodeSnapshot) -> f64,
) {
    out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} gauge\n"));
    for node in nodes {
        let id = node
            .identity
            .as_ref()
            .map(|identity| identity.node_id.as_str())
            .unwrap_or("");
        out.push_str(&format!("{name}{{node=\"{id}\"}} {}\n", value(node)));
    }
}

fn gauge(out: &mut String, name: &str, help: &str, value: f64) {
    out.push_str(&format!(
        "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}\n"
    ));
}

/// Renders the roll-up and the node signals in the Prometheus text format.
///
/// Pure, so the format is tested without binding a socket. Node gauges read from the same
/// `NodeSnapshot` the `nodes` command prints, so the two surfaces cannot disagree.
pub fn render(summary: &MetricsResponse, nodes: &[NodeSnapshot]) -> String {
    let mut out = String::new();

    gauge(
        &mut out,
        "ai_cluster_nodes_total",
        "Nós registrados no cluster.",
        f64::from(summary.nodes_total),
    );
    gauge(
        &mut out,
        "ai_cluster_nodes_available",
        "Nós que o placement pode escolher agora.",
        f64::from(summary.nodes_available),
    );
    gauge(
        &mut out,
        "ai_cluster_units_completed",
        "Unidades concluídas que o controller ainda retém.",
        f64::from(summary.units_completed),
    );
    gauge(
        &mut out,
        "ai_cluster_units_per_second_1m",
        "Unidades concluídas por segundo nos últimos 60 s. Zero sem história que cubra a janela.",
        summary.units_per_second_1m,
    );
    gauge(
        &mut out,
        "ai_cluster_units_per_second_5m",
        "Unidades concluídas por segundo nos últimos 300 s. A razão 1m/5m é a tendência.",
        summary.units_per_second_5m,
    );
    gauge(
        &mut out,
        "ai_cluster_avg_network_score",
        "Média do score de rede dos nós prontos que mediram. Zero é nenhum nó mediu.",
        summary.avg_network_score,
    );
    gauge(
        &mut out,
        "ai_cluster_avg_storage_score",
        "Média da escrita sequencial dos nós prontos que mediram, sobre 500 MB/s.",
        summary.avg_storage_score,
    );
    gauge(
        &mut out,
        "ai_cluster_avg_storage_read_score",
        "Média da leitura O_DIRECT dos nós prontos que mediram, sobre 500 MB/s.",
        summary.avg_storage_read_score,
    );
    gauge(
        &mut out,
        "ai_cluster_avg_storage_latency_ms",
        "Média da latência de escrita com fsync (ms) dos nós prontos que mediram.",
        summary.avg_storage_latency_ms,
    );
    gauge(
        &mut out,
        "ai_cluster_units_pending",
        "Unidades em fila mais as em voo.",
        f64::from(summary.units_pending),
    );
    gauge(
        &mut out,
        "ai_cluster_units_blocked",
        "Unidades à espera de dependência do grafo.",
        f64::from(summary.units_blocked),
    );
    gauge(
        &mut out,
        "ai_cluster_units_failed",
        "Unidades que nunca vão completar.",
        f64::from(summary.units_failed),
    );
    gauge(
        &mut out,
        "ai_cluster_compute_time_us_total",
        "Soma do compute auto-reportado pelos workers, em microssegundos.",
        summary.total_compute_time_us as f64,
    );
    gauge(
        &mut out,
        "ai_cluster_latency_us_total",
        "Soma da latência despacho→resultado observada pelo controller, em microssegundos.",
        summary.total_latency_us as f64,
    );
    gauge(
        &mut out,
        "ai_cluster_predicted_compute_time_us_total",
        "Soma do compute previsto pelo modelo de custo nas unidades concluídas, em microssegundos.",
        summary.total_predicted_compute_us as f64,
    );
    gauge(
        &mut out,
        "ai_cluster_p50_compute_time_us",
        "Mediana do compute das unidades concluídas, em microssegundos.",
        summary.p50_compute_time_us as f64,
    );
    gauge(
        &mut out,
        "ai_cluster_p95_compute_time_us",
        "p95 do compute das unidades concluídas, em microssegundos.",
        summary.p95_compute_time_us as f64,
    );
    gauge(
        &mut out,
        "ai_cluster_p50_latency_us",
        "Mediana da latência das unidades concluídas, em microssegundos.",
        summary.p50_latency_us as f64,
    );
    gauge(
        &mut out,
        "ai_cluster_p95_latency_us",
        "p95 da latência das unidades concluídas, em microssegundos.",
        summary.p95_latency_us as f64,
    );

    node_metric(
        &mut out,
        "ai_cluster_node_threads",
        "Threads que o nó reporta.",
        nodes,
        |node| {
            f64::from(
                node.capability
                    .as_ref()
                    .map_or(0, |capability| capability.cpu_threads),
            )
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_memory_bytes",
        "Memória que o nó reporta, em bytes.",
        nodes,
        |node| {
            node.capability
                .as_ref()
                .map_or(0.0, |capability| capability.memory_bytes as f64)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_compute_score",
        "Compute por core medido no worker, relativo ao core de referência. Melhor caso do benchmark.",
        nodes,
        |node| {
            node.capability
                .as_ref()
                .map_or(0.0, |capability| capability.compute_score)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_compute_median_score",
        "Mediana do benchmark de registro, na mesma escala de compute_score. Zero é não medido.",
        nodes,
        |node| {
            node.capability
                .as_ref()
                .map_or(0.0, |capability| capability.compute_median_score)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_compute_deviation",
        "Desvio relativo (desvio-padrão sobre média) das execuções do benchmark. Zero é estável ou não medido.",
        nodes,
        |node| {
            node.capability
                .as_ref()
                .map_or(0.0, |capability| capability.compute_deviation)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_baseline_confidence",
        "Quão limpo foi o benchmark do registro, 0..1 (melhor execução sobre a mediana). Zero é não medido.",
        nodes,
        |node| {
            node.capability
                .as_ref()
                .map_or(0.0, |capability| capability.baseline_confidence)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_network_retransmit_ratio",
        "Fração de segmentos TCP retransmitidos durante a sonda de rede do registro, 0..1. Zero é sem perda vista ou não medido.",
        nodes,
        |node| {
            node.capability
                .as_ref()
                .map_or(0.0, |capability| capability.network_retransmit_ratio)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_network_jitter",
        "Desvio relativo da taxa entre as repetições da sonda de rede. Zero é estável ou não medido.",
        nodes,
        |node| {
            node.capability
                .as_ref()
                .map_or(0.0, |capability| capability.network_jitter)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_memory_bandwidth_loaded_score",
        "Banda de memória com os núcleos copiando juntos, sobre 10 GB/s. Zero é não medido.",
        nodes,
        |node| {
            node.capability
                .as_ref()
                .map_or(0.0, |capability| capability.memory_bandwidth_loaded_score)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_storage_read_score",
        "Leitura sequencial sem cache (O_DIRECT) sobre 500 MB/s. Zero é não medido.",
        nodes,
        |node| {
            node.capability
                .as_ref()
                .map_or(0.0, |capability| capability.storage_read_score)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_health_score",
        "Saúde do nó: 1.0 saudável, 0 sem informação.",
        nodes,
        |node| {
            node.health
                .as_ref()
                .map_or(0.0, |health| health.health_score)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_cpu_utilization",
        "Fração de CPU em uso, 0..1. Zero na primeira leitura, que não tem intervalo anterior.",
        nodes,
        |node| {
            node.health
                .as_ref()
                .map_or(0.0, |health| health.cpu_utilization)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_memory_utilization",
        "Fração de memória em uso, 0..1.",
        nodes,
        |node| {
            node.health
                .as_ref()
                .map_or(0.0, |health| health.memory_utilization)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_swap_utilization",
        "Fração de swap em uso, 0..1.",
        nodes,
        |node| {
            node.health
                .as_ref()
                .map_or(0.0, |health| health.swap_utilization)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_temperature_celsius",
        "Temperatura do nó em Celsius. Zero significa sensor não visível.",
        nodes,
        |node| {
            node.health
                .as_ref()
                .map_or(0.0, |health| health.temperature_c)
        },
    );
    node_metric(
        &mut out,
        "ai_cluster_node_disk_io_ops",
        "Operações de disco por segundo desde o relatório anterior.",
        nodes,
        |node| {
            node.health
                .as_ref()
                .map_or(0.0, |health| health.disk_io_ops)
        },
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_cluster_protocol::cluster::{CapabilityReport, HealthReport, NodeIdentity};

    fn node(id: &str, cpu_utilization: f64) -> NodeSnapshot {
        NodeSnapshot {
            identity: Some(NodeIdentity {
                node_id: id.to_string(),
                ..Default::default()
            }),
            capability: Some(CapabilityReport {
                cpu_threads: 4,
                memory_bytes: 1_000,
                compute_score: 1.5,
                ..Default::default()
            }),
            health: Some(HealthReport {
                health_score: 0.9,
                cpu_utilization,
                swap_utilization: 0.25,
                temperature_c: 42.0,
                disk_io_ops: 12.0,
                ..Default::default()
            }),
            state: "READY".to_string(),
            registered_at_unix_ms: 0,
            last_heartbeat_unix_ms: 0,
            trusted: true,
            attested: false,
        }
    }

    fn summary() -> MetricsResponse {
        MetricsResponse {
            nodes_total: 2,
            nodes_available: 1,
            units_completed: 3,
            units_pending: 1,
            units_blocked: 0,
            units_failed: 0,
            total_compute_time_us: 300,
            total_latency_us: 900,
            total_predicted_compute_us: 40,
            avg_compute_time_us: 100,
            avg_latency_us: 300,
            p50_compute_time_us: 90,
            p95_compute_time_us: 200,
            p50_latency_us: 250,
            p95_latency_us: 800,
            units_per_second_1m: 0.5,
            avg_network_score: 2.0,
            ..Default::default()
        }
    }

    #[test]
    fn renders_the_cluster_roll_up() {
        let out = render(&summary(), &[]);
        assert!(out.contains("ai_cluster_nodes_total 2\n"));
        assert!(out.contains("ai_cluster_nodes_available 1\n"));
        assert!(out.contains("ai_cluster_units_completed 3\n"));
        assert!(out.contains("ai_cluster_compute_time_us_total 300\n"));
        assert!(out.contains("ai_cluster_predicted_compute_time_us_total 40\n"));
        assert!(out.contains("ai_cluster_p95_latency_us 800\n"));
        assert!(out.contains("ai_cluster_units_per_second_1m 0.5\n"));
        assert!(out.contains("ai_cluster_avg_network_score 2\n"));
        // Every metric declares its type exactly once; a repeat makes a strict parser fail.
        assert_eq!(out.matches("# TYPE ai_cluster_nodes_total").count(), 1);
    }

    #[test]
    fn renders_one_sample_per_node_with_the_node_label() {
        let out = render(&summary(), &[node("node-a", 0.5), node("node-b", 0.0)]);
        assert!(out.contains("ai_cluster_node_cpu_utilization{node=\"node-a\"} 0.5\n"));
        assert!(out.contains("ai_cluster_node_cpu_utilization{node=\"node-b\"} 0\n"));
        assert!(out.contains("ai_cluster_node_threads{node=\"node-a\"} 4\n"));
        assert!(out.contains("ai_cluster_node_temperature_celsius{node=\"node-a\"} 42\n"));
        // The header is emitted once even with two samples under it.
        assert_eq!(
            out.matches("# TYPE ai_cluster_node_cpu_utilization")
                .count(),
            1
        );
    }

    #[test]
    fn a_node_without_identity_does_not_break_the_sample() {
        // A node mid-registration has no identity yet; the metric still has to parse.
        let bare = NodeSnapshot::default();
        let out = render(&summary(), &[bare]);
        assert!(out.contains("ai_cluster_node_cpu_utilization{node=\"\"} 0\n"));
    }

    #[test]
    fn only_the_bearer_token_may_scrape() {
        let with = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::AUTHORIZATION, value.parse().unwrap());
            headers
        };
        assert!(authorized(&with("Bearer secret"), "secret"));
        for bad in ["Bearer wrong", "Bearer ", "secret", "Basic secret"] {
            assert!(!authorized(&with(bad), "secret"), "{bad:?}");
        }
        assert!(!authorized(&HeaderMap::new(), "secret"));
        assert!(!authorized(&with("Bearer "), ""));
    }
}
