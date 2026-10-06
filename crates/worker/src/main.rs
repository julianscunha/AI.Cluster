use std::{env, time::{SystemTime, UNIX_EPOCH}};

use real_ai_cluster_benchmark::run as run_benchmark;
use real_ai_cluster_common::hardware::{discover, StorageKind};
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

fn initial_node_info() -> NodeInfo {
    let node_id = env::var("RAC_NODE_ID").unwrap_or_else(|_| hostname());
    let device_id = env::var("RAC_DEVICE_ID")
        .unwrap_or_else(|_| format!("device-{node_id}"));
    let hardware = discover();

    let storage = hardware
        .storage
        .iter()
        .map(|disk| StorageInfo {
            name: disk.name.clone(),
            kind: match disk.kind {
                StorageKind::Hdd => "hdd",
                StorageKind::SataSsd => "sata-ssd",
                StorageKind::Nvme => "nvme",
                StorageKind::Unknown => "unknown",
            }
            .into(),
            size_bytes: disk.size_bytes,
            rotational: disk.rotational,
        })
        .collect();

    NodeInfo {
        node_id,
        device_id,
        hostname: hostname(),
        architecture: hardware.cpu.architecture,
        state: NodeState::Validating as i32,
        capability: Some(Capability {
            execution_backends: vec!["cpu".into()],
            cpu_logical_cores: hardware.cpu.logical_cores,
            memory_total_bytes: hardware.memory.total_bytes,
            storage,
            ..Default::default()
        }),
        health: Some(Health {
            overall: 1.0,
            cpu: 1.0,
            memory: 1.0,
            storage: 1.0,
            network: 1.0,
            thermal: 1.0,
        }),
        os_version: env::var("RAC_OS_VERSION")
            .unwrap_or_else(|_| "bootstrap".into()),
        runtime_version: env!("CARGO_PKG_VERSION").into(),
        last_heartbeat_unix_ms: now_ms(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let endpoint = env::var("RAC_CONTROLLER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());

    let mut client = ClusterControlClient::connect(endpoint).await?;
    let mut node = initial_node_info();

    let registration = client
        .register_node(Request::new(RegisterNodeRequest {
            node: Some(node.clone()),
        }))
        .await?
        .into_inner();

    if !registration.accepted {
        return Err(registration.message.into());
    }

    info!(
        node_id = %node.node_id,
        architecture = %node.architecture,
        "worker registrado; iniciando benchmark local"
    );

    let benchmark = tokio::task::spawn_blocking(run_benchmark).await?;

    if let Some(capability) = node.capability.as_mut() {
        capability.cpu_ops_per_sec = benchmark.cpu_ops_per_sec;
        capability.memory_mb_per_sec = benchmark.memory_mb_per_sec;
        capability.storage_write_mb_per_sec = benchmark.storage_write_mb_per_sec;
    }

    client
        .report_capability(Request::new(ReportCapabilityRequest {
            node_id: node.node_id.clone(),
            capability: node.capability.clone(),
        }))
        .await?;

    info!(
        node_id = %node.node_id,
        cpu_ops_per_sec = node.capability.as_ref().map(|c| c.cpu_ops_per_sec).unwrap_or_default(),
        memory_mb_per_sec = node.capability.as_ref().map(|c| c.memory_mb_per_sec).unwrap_or_default(),
        storage_write_mb_per_sec = node.capability.as_ref().map(|c| c.storage_write_mb_per_sec).unwrap_or_default(),
        "benchmark concluído"
    );

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
