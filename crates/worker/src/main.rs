use std::{
    env,
    hint::black_box,
    net::SocketAddr,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use real_ai_cluster_benchmark::run as run_benchmark;
use real_ai_cluster_common::hardware::{discover, StorageKind};
use real_ai_cluster_protocol::v1::{
    cluster_control_client::ClusterControlClient,
    worker_runtime_server::{WorkerRuntime, WorkerRuntimeServer},
    *,
};
use tonic::{Request, Response, Status};
use tracing::{info, warn};

#[derive(Clone)]
struct WorkerService {
    node_id: String,
}

#[tonic::async_trait]
impl WorkerRuntime for WorkerService {
    async fn execute_work(
        &self,
        request: Request<ExecuteWorkRequest>,
    ) -> Result<Response<ExecuteWorkResponse>, Status> {
        let work = request
            .into_inner()
            .work
            .ok_or_else(|| Status::invalid_argument("work é obrigatório"))?;

        let started = Instant::now();

        let output = match work.operation.as_str() {
            "echo" => work.payload.clone(),

            "cpu_spin" => {
                if work.payload.len() != 8 {
                    return Err(Status::invalid_argument(
                        "cpu_spin exige payload com 8 bytes little-endian",
                    ));
                }

                let iterations = u64::from_le_bytes(
                    work.payload
                        .as_slice()
                        .try_into()
                        .expect("payload validado"),
                );

                let mut accumulator = 0u64;
                for i in 0..iterations {
                    accumulator = accumulator
                        .wrapping_mul(1_664_525)
                        .wrapping_add(i ^ 0x9e37_79b9);
                }

                black_box(accumulator).to_le_bytes().to_vec()
            }

            operation => {
                return Err(Status::unimplemented(format!(
                    "operação não suportada: {operation}"
                )));
            }
        };

        let duration_us = started.elapsed().as_micros() as u64;

        info!(
            node_id = %self.node_id,
            work_id = %work.work_id,
            operation = %work.operation,
            duration_us,
            "Work Unit executada"
        );

        Ok(Response::new(ExecuteWorkResponse {
            accepted: true,
            message: "execução concluída".into(),
            node_id: self.node_id.clone(),
            duration_us,
            output,
        }))
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn hostname() -> String {
    env::var("HOSTNAME").unwrap_or_else(|_| "worker".into())
}

fn initial_node_info(worker_endpoint: &str) -> NodeInfo {
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
        worker_endpoint: worker_endpoint.into(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let controller_endpoint = env::var("RAC_CONTROLLER_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".into());

    let worker_bind: SocketAddr = env::var("RAC_WORKER_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50052".into())
        .parse()?;

    let advertised_endpoint = env::var("RAC_WORKER_ENDPOINT")
        .unwrap_or_else(|_| format!("http://127.0.0.1:{}", worker_bind.port()));

    let mut client = ClusterControlClient::connect(controller_endpoint).await?;
    let mut node = initial_node_info(&advertised_endpoint);

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
        endpoint = %node.worker_endpoint,
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

    let worker_service = WorkerService {
        node_id: node.node_id.clone(),
    };
    let server_addr = worker_bind;

    tokio::spawn(async move {
        if let Err(error) = tonic::transport::Server::builder()
            .add_service(WorkerRuntimeServer::new(worker_service))
            .serve(server_addr)
            .await
        {
            warn!(%error, "servidor do worker encerrado");
        }
    });

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
