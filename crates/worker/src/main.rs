use std::{env, time::{SystemTime, UNIX_EPOCH}};

use real_ai_cluster_protocol::v1::{
    cluster_control_client::ClusterControlClient,
    *,
};
use tonic::Request;
use tracing::info;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn hostname() -> String {
    env::var("HOSTNAME").unwrap_or_else(|_| "worker".into())
}

fn node_info() -> NodeInfo {
    let node_id = env::var("RAC_NODE_ID").unwrap_or_else(|_| hostname());
    let device_id = env::var("RAC_DEVICE_ID").unwrap_or_else(|_| format!("device-{node_id}"));
    let architecture = env::consts::ARCH.to_string();

    NodeInfo {
        node_id,
        device_id,
        hostname: hostname(),
        architecture,
        state: NodeState::Discovered as i32,
        capability: Some(Capability {
            general_compute: 1.0,
            memory_bandwidth: 1.0,
            storage_score: 1.0,
            network_score: 1.0,
            execution_backends: vec!["cpu".into()],
        }),
        health: Some(Health {
            overall: 1.0,
            cpu: 1.0,
            memory: 1.0,
            storage: 1.0,
            network: 1.0,
            thermal: 1.0,
        }),
        os_version: env::var("RAC_OS_VERSION").unwrap_or_else(|_| "bootstrap".into()),
        runtime_version: env!("CARGO_PKG_VERSION").into(),
        last_heartbeat_unix_ms: now_ms(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let endpoint = env::var("RAC_CONTROLLER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());

    let mut client = ClusterControlClient::connect(endpoint).await?;
    let node = node_info();

    let response = client
        .register_node(Request::new(RegisterNodeRequest { node: Some(node.clone()) }))
        .await?
        .into_inner();

    if !response.accepted {
        return Err(response.message.into());
    }

    info!(node_id = %node.node_id, "worker registrado");

    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3));

    loop {
        ticker.tick().await;

        client
            .heartbeat(Request::new(HeartbeatRequest {
                node_id: node.node_id.clone(),
                unix_ms: now_ms(),
            }))
            .await?;

        client
            .report_health(Request::new(ReportHealthRequest {
                node_id: node.node_id.clone(),
                health: Some(Health {
                    overall: 1.0,
                    cpu: 1.0,
                    memory: 1.0,
                    storage: 1.0,
                    network: 1.0,
                    thermal: 1.0,
                }),
            }))
            .await?;
    }
}
