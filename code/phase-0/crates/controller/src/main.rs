use std::sync::Arc;
use std::time::Duration;

use ai_cluster_controller::registry::{unix_ms, Registry};
use ai_cluster_controller::service::ClusterControlService;
use ai_cluster_protocol::cluster::cluster_control_server::ClusterControlServer;
use tokio::time;
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
    let service = ClusterControlService::new(registry.clone(), "local");

    // ponytail: single sweep interval for all nodes; per-node timers only if grace must vary.
    let sweep = registry.clone();
    tokio::spawn(async move {
        let mut ticker = time::interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            for node_id in sweep.mark_stale_offline(unix_ms(), grace_ms) {
                println!("node {node_id} offline after {grace_ms}ms without heartbeat");
            }
        }
    });

    println!("controller listening on {addr} (heartbeat grace {grace_ms}ms)");
    Server::builder()
        .add_service(ClusterControlServer::new(service))
        .serve(addr)
        .await?;
    Ok(())
}
