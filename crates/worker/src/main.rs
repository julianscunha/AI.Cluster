use std::{
    env,
    hint::black_box,
    net::SocketAddr,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use real_ai_cluster_benchmark::{matmul_row_shard, run as run_benchmark};
use real_ai_cluster_common::hardware::{discover, StorageKind};
use real_ai_cluster_protocol::v1::{
    cluster_control_client::ClusterControlClient,
    worker_runtime_server::{WorkerRuntime, WorkerRuntimeServer},
    *,
};
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

mod buffer_store;

const DEFAULT_WORKER_BUFFER_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone)]
struct WorkerService {
    node_id: String,
    buffers: Arc<RwLock<buffer_store::BufferStore>>,
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
        let mut effective_payload = work.payload.clone();

        for input in &work.inputs {
            if input.local_buffer {
                let bytes = {
                    let mut store = self.buffers.write().await;
                    store.get_for_consumer(&input.buffer_id)
                }
                .ok_or_else(|| {
                    Status::failed_precondition(format!(
                        "buffer local não está disponível: {}",
                        input.buffer_id
                    ))
                })?;

                effective_payload.extend_from_slice(&bytes);
            } else {
                effective_payload.extend_from_slice(&input.data);
            }
        }

        let output = match work.operation.as_str() {
            "echo" => effective_payload.clone(),

            "cpu_spin" => {
                if effective_payload.len() != 8 {
                    return Err(Status::invalid_argument(
                        "cpu_spin exige payload com 8 bytes little-endian",
                    ));
                }

                let iterations = u64::from_le_bytes(
                    effective_payload
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

        if !work.output_buffer_id.is_empty() && work.output_buffer_pins > 0 {
            let store_result = self.buffers.write().await.insert_with_pins(
                work.output_buffer_id.clone(),
                output.clone(),
                work.output_buffer_pins as usize,
            );

            if let Err(error) = store_result {
                warn!(
                    node_id = %self.node_id,
                    buffer_id = %work.output_buffer_id,
                    %error,
                    "output não pôde ser mantido no Buffer Store local; Controller Cache será fallback"
                );
            }
        }

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

    async fn put_buffer(
        &self,
        request: Request<PutBufferRequest>,
    ) -> Result<Response<PutBufferResponse>, Status> {
        let request = request.into_inner();
        if request.buffer_id.is_empty() {
            return Err(Status::invalid_argument("buffer_id é obrigatório"));
        }

        self.buffers
            .write()
            .await
            .insert_with_pins(
                request.buffer_id.clone(),
                request.data,
                request.pins as usize,
            )
            .map_err(|error| Status::resource_exhausted(error.to_string()))?;

        Ok(Response::new(PutBufferResponse {
            accepted: true,
            message: "buffer armazenado localmente".into(),
        }))
    }

    async fn get_buffer(
        &self,
        request: Request<GetBufferRequest>,
    ) -> Result<Response<GetBufferResponse>, Status> {
        let request = request.into_inner();
        let data = {
            let mut store = self.buffers.write().await;
            if request.consume_pin {
                store.get_for_consumer(&request.buffer_id)
            } else {
                store.get(&request.buffer_id)
            }
        };

        Ok(Response::new(match data {
            Some(data) => GetBufferResponse {
                found: true,
                message: "buffer encontrado".into(),
                data,
            },
            None => GetBufferResponse {
                found: false,
                message: "buffer não encontrado".into(),
                data: Vec::new(),
            },
        }))
    }

    async fn has_buffer(
        &self,
        request: Request<HasBufferRequest>,
    ) -> Result<Response<HasBufferResponse>, Status> {
        let request = request.into_inner();
        let found = self.buffers.read().await.contains(&request.buffer_id);
        Ok(Response::new(HasBufferResponse { found }))
    }

    async fn execute_matrix_block(
        &self,
        request: Request<ExecuteMatrixBlockRequest>,
    ) -> Result<Response<ExecuteMatrixBlockResponse>, Status> {
        let req = request.into_inner();

        let rows = req.rows as usize;
        let inner = req.inner as usize;
        let cols = req.cols as usize;
        let row_start = req.row_start as usize;
        let row_count = req.row_count as usize;

        if rows == 0 || inner == 0 || cols == 0 || row_count == 0 {
            return Err(Status::invalid_argument("dimensões inválidas"));
        }

        if row_start.saturating_add(row_count) > rows {
            return Err(Status::invalid_argument(
                "bloco ultrapassa número de linhas",
            ));
        }

        let a_shard = bytes_to_f32(&req.matrix_a_shard)
            .ok_or_else(|| Status::invalid_argument("matrix_a inválida"))?;
        let b = bytes_to_f32(&req.matrix_b)
            .ok_or_else(|| Status::invalid_argument("matrix_b inválida"))?;

        let expected_a = row_count
            .checked_mul(inner)
            .ok_or_else(|| Status::invalid_argument("dimensão do shard de A excede limite"))?;

        let expected_b = inner
            .checked_mul(cols)
            .ok_or_else(|| Status::invalid_argument("dimensão de B excede limite"))?;

        if a_shard.len() != expected_a || b.len() != expected_b {
            return Err(Status::invalid_argument(
                "tamanho dos shards incompatível com dimensões",
            ));
        }

        let started = Instant::now();

        let output = matmul_row_shard(inner, cols, row_count, &a_shard, &b);

        let duration_us = started.elapsed().as_micros() as u64;

        Ok(Response::new(ExecuteMatrixBlockResponse {
            accepted: true,
            message: "bloco calculado".into(),
            node_id: self.node_id.clone(),
            row_start: req.row_start,
            row_count: req.row_count,
            duration_us,
            matrix_c: f32_to_bytes(&output),
        }))
    }
}

fn bytes_to_f32(bytes: &[u8]) -> Option<Vec<f32>> {
    let (chunks, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        return None;
    }

    Some(
        chunks
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect(),
    )
}

fn f32_to_bytes(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
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
    let device_id = env::var("RAC_DEVICE_ID").unwrap_or_else(|_| format!("device-{node_id}"));
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
        os_version: env::var("RAC_OS_VERSION").unwrap_or_else(|_| "bootstrap".into()),
        runtime_version: env!("CARGO_PKG_VERSION").into(),
        last_heartbeat_unix_ms: now_ms(),
        worker_endpoint: worker_endpoint.into(),
        active_work_units: 0,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let controller_endpoint =
        env::var("RAC_CONTROLLER_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:50051".into());

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

    let mut network_samples_us = Vec::with_capacity(5);

    client
        .network_ping(Request::new(NetworkPingRequest {
            node_id: node.node_id.clone(),
            payload: Vec::new(),
        }))
        .await?;

    for _ in 0..5 {
        let started = Instant::now();

        client
            .network_ping(Request::new(NetworkPingRequest {
                node_id: node.node_id.clone(),
                payload: Vec::new(),
            }))
            .await?;

        network_samples_us.push(started.elapsed().as_micros() as u64);
    }

    network_samples_us.sort_unstable();
    let network_rtt_us = network_samples_us
        .get(network_samples_us.len() / 2)
        .copied()
        .unwrap_or_default() as f64;

    const NETWORK_PAYLOAD_BYTES: usize = 512 * 1024;
    const NETWORK_THROUGHPUT_SAMPLES: usize = 3;
    let network_payload = vec![0xA5u8; NETWORK_PAYLOAD_BYTES];
    let mut network_throughput_samples = Vec::with_capacity(NETWORK_THROUGHPUT_SAMPLES);

    for _ in 0..NETWORK_THROUGHPUT_SAMPLES {
        let started = Instant::now();

        let response = client
            .network_ping(Request::new(NetworkPingRequest {
                node_id: node.node_id.clone(),
                payload: network_payload.clone(),
            }))
            .await?
            .into_inner();

        if !response.accepted || response.payload.len() != NETWORK_PAYLOAD_BYTES {
            return Err("probe de throughput retornou payload inválido".into());
        }

        let elapsed_seconds = started.elapsed().as_secs_f64().max(f64::MIN_POSITIVE);
        let megabytes = NETWORK_PAYLOAD_BYTES as f64 / (1024.0 * 1024.0);
        network_throughput_samples.push(megabytes / elapsed_seconds);
    }

    network_throughput_samples.sort_by(f64::total_cmp);
    let network_mb_per_sec = network_throughput_samples
        .get(network_throughput_samples.len() / 2)
        .copied()
        .unwrap_or_default();

    if let Some(capability) = node.capability.as_mut() {
        capability.network_rtt_us = network_rtt_us;
        capability.network_mb_per_sec = network_mb_per_sec;
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
        network_rtt_us,
        network_mb_per_sec,
        "benchmark concluído"
    );

    let buffer_capacity = match env::var("RAC_BUFFER_STORE_BYTES") {
        Ok(value) => value.parse::<u64>()?,
        Err(_) => DEFAULT_WORKER_BUFFER_BYTES,
    };

    info!(
        buffer_capacity_bytes = buffer_capacity,
        "capacidade do buffer store local configurada"
    );

    let worker_service = WorkerService {
        node_id: node.node_id.clone(),
        buffers: Arc::new(RwLock::new(buffer_store::BufferStore::new(buffer_capacity))),
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
