use std::sync::Arc;
use std::time::Duration;

use ai_cluster_controller::events::EventLog;
use ai_cluster_controller::registry::{Registry, unix_ms};
use ai_cluster_controller::service::{ClusterControlService, LinkProbeService, MAX_PROBE_BYTES};
use ai_cluster_controller::work::WorkStore;
use ai_cluster_protocol::cluster::cluster_control_server::ClusterControlServer;
use ai_cluster_protocol::cluster::link_probe_server::LinkProbeServer;
use ai_cluster_protocol::metadata::{MAX_MESSAGE_BYTES, ServerMetadata};
use tokio::time;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::var("AI_CLUSTER_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50051".to_string())
        .parse::<std::net::SocketAddr>()?;
    let grace_ms = std::env::var("AI_CLUSTER_HEARTBEAT_GRACE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(15_000);

    let registry = Arc::new(Registry::new());
    // AI_CLUSTER_STATE names a snapshot file. Unset means memory only, which is what
    // the benchmark runs use; point it at a mounted volume so a restart keeps the
    // queue instead of losing it with the container.
    let work = Arc::new(match std::env::var("AI_CLUSTER_STATE") {
        Ok(path) if !path.trim().is_empty() => WorkStore::with_state(path),
        _ => WorkStore::new(),
    });
    let events = Arc::new(EventLog::new());
    let service =
        ClusterControlService::new(registry.clone(), work.clone(), events.clone(), "local");

    // ponytail: single sweep interval for all nodes; per-node timers only if grace must vary.
    let sweep = registry.clone();
    let sweep_work = work.clone();
    let sweep_events = events.clone();
    // Quando o controller subiu. A posse restaurada do disco é medida contra isto, não
    // contra o horário do despacho original: o que importa é se o dono voltou depois do
    // restart.
    let started_unix_ms = unix_ms();
    tokio::spawn(async move {
        let mut ticker = time::interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            for node_id in sweep.mark_stale_offline(unix_ms(), grace_ms) {
                // A node that went silent still holds its in-flight units; hand
                // them back so the surviving nodes can finish the work.
                let outcome = sweep_work.requeue_for_node(&node_id);
                println!(
                    "nó {node_id} offline após {grace_ms}ms sem heartbeat; {} unidades em voo reenfileiradas, {} falharam",
                    outcome.requeued, outcome.failed
                );
                sweep_events.record(
                    unix_ms(),
                    "node_offline",
                    format!(
                        "{node_id} após {grace_ms}ms reenfileiradas={} falharam={}",
                        outcome.requeued, outcome.failed
                    ),
                );
            }
            // Unidades em voo cujo dono nunca reapareceu depois de um restart: a posse veio
            // do snapshot, mas o nó não voltou. Antes do grace elas esperam — o dono pode
            // estar voltando, e reoferecer agora só faria a unidade rodar duas vezes. Passado
            // o grace valem as mesmas regras de um nó perdido: volta para a fila, ou falha se
            // o orçamento de tentativas acabou.
            if unix_ms().saturating_sub(started_unix_ms) >= grace_ms {
                let known: Vec<String> = sweep
                    .list()
                    .iter()
                    .filter_map(|node| node.identity.as_ref().map(|id| id.node_id.clone()))
                    .collect();
                for node_id in sweep_work.unknown_owners(&known) {
                    let outcome = sweep_work.requeue_for_node(&node_id);
                    println!(
                        "nó {node_id} não voltou após o restart; {} unidades em voo reenfileiradas, {} falharam",
                        outcome.requeued, outcome.failed
                    );
                    sweep_events.record(
                        unix_ms(),
                        "node_offline",
                        format!(
                            "{node_id} ausente após restart reenfileiradas={} falharam={}",
                            outcome.requeued, outcome.failed
                        ),
                    );
                }
            }
        }
    });

    // Prometheus exporter (H1): the same roll-up pushed to whoever scrapes, so watching the
    // cluster does not require a client that speaks gRPC. A failure here is reported and
    // never fatal — losing the exporter must not take the control plane down with it.
    let exporter_registry = registry.clone();
    let exporter_work = work.clone();
    tokio::spawn(async move {
        let metrics_addr =
            std::env::var("AI_CLUSTER_METRICS_ADDR").unwrap_or_else(|_| "0.0.0.0:9101".to_string());
        println!("exportador de métricas em http://{metrics_addr}/metrics");
        if let Err(err) =
            ai_cluster_controller::exporter::serve(metrics_addr, exporter_registry, exporter_work)
                .await
        {
            println!("exportador de métricas parou: {err}");
        }
    });

    // The generated `with_interceptor` builds its own server without the size limits, so
    // the limits are set on the server first and the interceptor is attached afterwards.
    let server = ClusterControlServer::new(service)
        .max_decoding_message_size(MAX_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES);
    // Messages a little over the probe payload (protobuf framing), still far under the data limit.
    let probe =
        LinkProbeServer::new(LinkProbeService).max_decoding_message_size(MAX_PROBE_BYTES + 1024);
    println!("controller escutando em {addr} (graça de heartbeat {grace_ms}ms)");
    Server::builder()
        .add_service(InterceptedService::new(server, ServerMetadata.clone()))
        .add_service(InterceptedService::new(probe, ServerMetadata))
        .serve(addr)
        .await?;
    Ok(())
}
