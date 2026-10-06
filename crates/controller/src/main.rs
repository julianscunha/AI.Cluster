use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use real_ai_cluster_protocol::v1::{
    cluster_control_server::{ClusterControl, ClusterControlServer},
    worker_runtime_client::WorkerRuntimeClient,
    *,
};
use tokio::{sync::RwLock, task::JoinSet};
use tonic::{Request, Response, Status};
use tracing::{info, warn};

mod scheduler;

#[derive(Clone)]
struct Controller {
    nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
    offline_after: Duration,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            offline_after: Duration::from_secs(10),
        }
    }

    async fn select_worker(&self) -> Result<NodeInfo, Status> {
        self.select_workers(1)
            .await
            .into_iter()
            .next()
            .ok_or_else(|| Status::unavailable("nenhum worker disponível"))
    }

    async fn select_workers(&self, requested: usize) -> Vec<NodeInfo> {
        if requested == 0 {
            return Vec::new();
        }

        let mut nodes = self.nodes.write().await;

        let mut candidates: Vec<NodeInfo> = nodes
            .values()
            .filter(|node| {
                matches!(
                    NodeState::try_from(node.state),
                    Ok(NodeState::Ready | NodeState::Active)
                ) && !node.worker_endpoint.is_empty()
            })
            .cloned()
            .collect();

        if candidates.is_empty() {
            return Vec::new();
        }

        candidates = scheduler::rank_workers(candidates);

        let selected_count = requested.min(candidates.len());
        candidates.truncate(selected_count);

        let selected_ids: Vec<String> =
            candidates.iter().map(|node| node.node_id.clone()).collect();

        for node_id in &selected_ids {
            if let Some(node) = nodes.get_mut(node_id) {
                node.active_work_units = node.active_work_units.saturating_add(1);
            }
        }

        candidates
    }

    async fn release_workers(&self, node_ids: &[String]) {
        if node_ids.is_empty() {
            return;
        }

        let mut nodes = self.nodes.write().await;

        for node_id in node_ids {
            if let Some(node) = nodes.get_mut(node_id) {
                node.active_work_units = node.active_work_units.saturating_sub(1);
            }
        }
    }

    async fn reconcile_loop(self) {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let now = now_ms();
            let timeout_ms = self.offline_after.as_millis() as i64;
            let mut nodes = self.nodes.write().await;

            for node in nodes.values_mut() {
                if node.last_heartbeat_unix_ms > 0
                    && now.saturating_sub(node.last_heartbeat_unix_ms) > timeout_ms
                    && node.state != NodeState::Offline as i32
                {
                    warn!(node_id = %node.node_id, "node marcado como offline");
                    node.state = NodeState::Offline as i32;
                }
            }
        }
    }
}

#[tonic::async_trait]
impl ClusterControl for Controller {
    async fn register_node(
        &self,
        request: Request<RegisterNodeRequest>,
    ) -> Result<Response<RegisterNodeResponse>, Status> {
        let mut node = request
            .into_inner()
            .node
            .ok_or_else(|| Status::invalid_argument("node é obrigatório"))?;

        node.last_heartbeat_unix_ms = now_ms();
        node.state = NodeState::Ready as i32;
        node.active_work_units = 0;

        let mut nodes = self.nodes.write().await;
        nodes.insert(node.node_id.clone(), node.clone());

        info!(
            node_id = %node.node_id,
            hostname = %node.hostname,
            endpoint = %node.worker_endpoint,
            "node registrado"
        );

        Ok(Response::new(RegisterNodeResponse {
            accepted: true,
            message: "node registrado".into(),
            node: Some(node),
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let req = request.into_inner();
        let mut nodes = self.nodes.write().await;

        let node = nodes
            .get_mut(&req.node_id)
            .ok_or_else(|| Status::not_found("node não registrado"))?;

        node.last_heartbeat_unix_ms = req.unix_ms.max(now_ms());

        if node.state == NodeState::Offline as i32 {
            node.state = NodeState::Ready as i32;
        }

        Ok(Response::new(HeartbeatResponse {
            accepted: true,
            state: node.state,
        }))
    }

    async fn report_health(
        &self,
        request: Request<ReportHealthRequest>,
    ) -> Result<Response<ReportHealthResponse>, Status> {
        let req = request.into_inner();
        let mut nodes = self.nodes.write().await;

        let node = nodes
            .get_mut(&req.node_id)
            .ok_or_else(|| Status::not_found("node não registrado"))?;

        if let Some(health) = req.health {
            node.health = Some(health);
        }

        let overall = node.health.as_ref().map(|h| h.overall).unwrap_or(1.0);

        if overall < 0.5 {
            node.state = NodeState::Degraded as i32;
        } else if node.state != NodeState::Draining as i32 {
            node.state = NodeState::Active as i32;
        }

        Ok(Response::new(ReportHealthResponse {
            accepted: true,
            state: node.state,
        }))
    }

    async fn report_capability(
        &self,
        request: Request<ReportCapabilityRequest>,
    ) -> Result<Response<ReportCapabilityResponse>, Status> {
        let req = request.into_inner();
        let mut nodes = self.nodes.write().await;

        let node = nodes
            .get_mut(&req.node_id)
            .ok_or_else(|| Status::not_found("node não registrado"))?;

        if let Some(capability) = req.capability {
            node.capability = Some(capability);
        }

        Ok(Response::new(ReportCapabilityResponse { accepted: true }))
    }

    async fn submit_work(
        &self,
        request: Request<SubmitWorkRequest>,
    ) -> Result<Response<SubmitWorkResponse>, Status> {
        let work = request
            .into_inner()
            .work
            .ok_or_else(|| Status::invalid_argument("work é obrigatório"))?;

        let node = self.select_worker().await?;

        let execution_result = async {
            let mut client = WorkerRuntimeClient::connect(node.worker_endpoint.clone())
                .await
                .map_err(|error| Status::unavailable(format!("worker indisponível: {error}")))?;

            client
                .execute_work(Request::new(ExecuteWorkRequest {
                    work: Some(work.clone()),
                }))
                .await
                .map_err(|error| Status::unavailable(format!("execução falhou: {error}")))
                .map(|response| response.into_inner())
        }
        .await;

        self.release_workers(std::slice::from_ref(&node.node_id))
            .await;

        let result = execution_result?;

        if !result.accepted {
            return Err(Status::aborted(result.message));
        }

        Ok(Response::new(SubmitWorkResponse {
            accepted: true,
            message: result.message,
            assigned_node_id: node.node_id,
            operation: work.operation,
            duration_us: result.duration_us,
            output: result.output,
        }))
    }

    async fn distributed_mat_mul(
        &self,
        request: Request<DistributedMatMulRequest>,
    ) -> Result<Response<DistributedMatMulResponse>, Status> {
        let req = request.into_inner();

        let rows = req.rows as usize;
        let inner = req.inner as usize;
        let cols = req.cols as usize;

        validate_matrix_payload(rows, inner, cols, &req.matrix_a, &req.matrix_b)?;

        if rows == 0 || inner == 0 || cols == 0 {
            return Err(Status::invalid_argument(
                "dimensões devem ser maiores que zero",
            ));
        }

        let requested_workers = if req.max_workers == 0 {
            rows
        } else {
            req.max_workers as usize
        };

        let workers = self.select_workers(requested_workers.min(rows)).await;
        if workers.is_empty() {
            return Err(Status::unavailable("nenhum worker disponível"));
        }

        let reserved_worker_ids: Vec<String> =
            workers.iter().map(|node| node.node_id.clone()).collect();

        let worker_count = workers.len().min(rows);
        let mut ranges = Vec::with_capacity(worker_count);
        let base = rows / worker_count;
        let remainder = rows % worker_count;
        let mut start = 0usize;

        for index in 0..worker_count {
            let count = base + usize::from(index < remainder);
            ranges.push((start, count));
            start += count;
        }

        let started = Instant::now();
        let mut jobs = JoinSet::new();

        for (node, (row_start, row_count)) in workers.into_iter().zip(ranges.into_iter()) {
            let endpoint = node.worker_endpoint.clone();
            let work_id = req.work_id.clone();
            let matrix_a = req.matrix_a.clone();
            let matrix_b = req.matrix_b.clone();

            jobs.spawn(async move {
                let mut client = WorkerRuntimeClient::connect(endpoint)
                    .await
                    .map_err(|e| format!("falha ao conectar ao worker: {e}"))?;

                let result = client
                    .execute_matrix_block(Request::new(ExecuteMatrixBlockRequest {
                        work_id,
                        rows: rows as u32,
                        inner: inner as u32,
                        cols: cols as u32,
                        row_start: row_start as u32,
                        row_count: row_count as u32,
                        matrix_a,
                        matrix_b,
                    }))
                    .await
                    .map_err(|e| format!("falha na execução do bloco: {e}"))?
                    .into_inner();

                if !result.accepted {
                    return Err(result.message);
                }

                Ok::<ExecuteMatrixBlockResponse, String>(result)
            });
        }

        let mut blocks = Vec::with_capacity(worker_count);
        let mut execution_error = None;

        while let Some(joined) = jobs.join_next().await {
            match joined {
                Ok(Ok(block)) => blocks.push(block),
                Ok(Err(error)) => {
                    execution_error = Some(Status::unavailable(error));
                }
                Err(error) => {
                    execution_error =
                        Some(Status::internal(format!("task do worker falhou: {error}")));
                }
            }
        }

        self.release_workers(&reserved_worker_ids).await;

        if let Some(error) = execution_error {
            return Err(error);
        }

        blocks.sort_by_key(|block| block.row_start);

        validate_matrix_blocks(rows, cols, &blocks)?;

        let compute_duration_us = blocks
            .iter()
            .map(|block| block.duration_us)
            .max()
            .unwrap_or_default();

        let per_worker_input_bytes =
            (req.matrix_a.len() as u64).saturating_add(req.matrix_b.len() as u64);
        let input_bytes_sent = per_worker_input_bytes.saturating_mul(blocks.len() as u64);

        let mut matrix_c = Vec::with_capacity(rows * cols * std::mem::size_of::<f32>());

        for block in &blocks {
            matrix_c.extend_from_slice(&block.matrix_c);
        }

        Ok(Response::new(DistributedMatMulResponse {
            accepted: true,
            message: "multiplicação distribuída concluída".into(),
            workers_used: blocks.len() as u32,
            duration_us: started.elapsed().as_micros() as u64,
            matrix_c,
            compute_duration_us,
            input_bytes_sent,
        }))
    }

    async fn network_ping(
        &self,
        request: Request<NetworkPingRequest>,
    ) -> Result<Response<NetworkPingResponse>, Status> {
        let req = request.into_inner();

        let nodes = self.nodes.read().await;
        if !nodes.contains_key(&req.node_id) {
            return Err(Status::not_found("node não registrado"));
        }

        Ok(Response::new(NetworkPingResponse {
            accepted: true,
            message: "probe recebido".into(),
            payload: req.payload,
        }))
    }

    async fn list_nodes(
        &self,
        _request: Request<ListNodesRequest>,
    ) -> Result<Response<ListNodesResponse>, Status> {
        let nodes = self.nodes.read().await;

        Ok(Response::new(ListNodesResponse {
            nodes: nodes.values().cloned().collect(),
        }))
    }
}

fn validate_matrix_blocks(
    rows: usize,
    cols: usize,
    blocks: &[ExecuteMatrixBlockResponse],
) -> Result<(), Status> {
    if blocks.is_empty() {
        return Err(Status::internal("nenhum bloco retornado pelos workers"));
    }

    let mut next_row = 0usize;
    let expected_block_bytes = |row_count: usize| {
        row_count
            .checked_mul(cols)
            .and_then(|values| values.checked_mul(4))
            .unwrap_or(usize::MAX)
    };

    for block in blocks {
        let row_start = block.row_start as usize;
        let row_count = block.row_count as usize;

        if row_start != next_row {
            return Err(Status::internal(
                "blocos retornados não cobrem as linhas de forma contínua",
            ));
        }

        if row_count == 0 || row_start.checked_add(row_count).unwrap_or(usize::MAX) > rows {
            return Err(Status::internal(
                "bloco retornado possui intervalo de linhas inválido",
            ));
        }

        if block.matrix_c.len() != expected_block_bytes(row_count) {
            return Err(Status::internal(
                "tamanho da matriz C do bloco é incompatível",
            ));
        }

        next_row += row_count;
    }

    if next_row != rows {
        return Err(Status::internal(
            "blocos retornados não cobrem todas as linhas da matriz C",
        ));
    }

    Ok(())
}

fn validate_matrix_payload(
    rows: usize,
    inner: usize,
    cols: usize,
    matrix_a: &[u8],
    matrix_b: &[u8],
) -> Result<(), Status> {
    let expected_a = rows
        .checked_mul(inner)
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| Status::invalid_argument("dimensões de A excedem o limite"))?;

    let expected_b = inner
        .checked_mul(cols)
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| Status::invalid_argument("dimensões de B excedem o limite"))?;

    if matrix_a.len() != expected_a {
        return Err(Status::invalid_argument("tamanho de matrix_a incompatível"));
    }

    if matrix_b.len() != expected_b {
        return Err(Status::invalid_argument("tamanho de matrix_b incompatível"));
    }

    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let addr: SocketAddr = std::env::var("RAC_CONTROLLER_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50051".into())
        .parse()?;

    let controller = Controller::new();
    tokio::spawn(controller.clone().reconcile_loop());

    info!(%addr, "Real.AI.Cluster Controller iniciado");

    tonic::transport::Server::builder()
        .add_service(ClusterControlServer::new(controller))
        .serve(addr)
        .await?;

    Ok(())
}

