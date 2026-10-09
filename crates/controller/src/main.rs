use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use real_ai_cluster_common::{
    residency::{plan_buffer_access, BufferAccessOptions, BufferAccessSource},
    tensor::{BufferDescriptor, DataLocation, TransferPlan, TransferStrategy},
};
use real_ai_cluster_protocol::v1::{
    cluster_control_server::{ClusterControl, ClusterControlServer},
    worker_runtime_client::WorkerRuntimeClient,
    *,
};
use tokio::{sync::RwLock, task::JoinSet};
use tonic::{Request, Response, Status};
use tracing::{info, warn};

mod buffer_store;
mod execution;
mod scheduler;
mod transfer;

const DEFAULT_BUFFER_CACHE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone)]
struct Controller {
    nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
    buffers: Arc<RwLock<buffer_store::BufferStore>>,
    offline_after: Duration,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    fn new() -> Self {
        Self::with_buffer_cache_capacity(DEFAULT_BUFFER_CACHE_BYTES)
    }

    fn with_buffer_cache_capacity(max_buffer_cache_bytes: u64) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            buffers: Arc::new(RwLock::new(buffer_store::BufferStore::new(
                max_buffer_cache_bytes,
            ))),
            offline_after: Duration::from_secs(10),
        }
    }

    async fn plan_single_work(
        &self,
        work: &WorkUnit,
    ) -> Result<(execution::PlacementPlan, NodeInfo), Status> {
        let graph = execution::ExecutionGraph::single(work.clone());
        let mut nodes = self.nodes.write().await;
        let candidates: Vec<NodeInfo> = nodes.values().cloned().collect();

        let plan = execution::PlacementPlanner
            .plan(&graph, &candidates)
            .map_err(|error| Status::unavailable(error.to_string()))?;

        let assignment = plan
            .node_for(&work.work_id)
            .ok_or_else(|| Status::internal("placement plan não contém a work unit solicitada"))?;

        let node = nodes
            .get_mut(&assignment.node_id)
            .ok_or_else(|| Status::unavailable("node selecionado deixou de existir"))?;

        node.active_work_units = node.active_work_units.saturating_add(1);

        info!(
            execution_id = %plan.execution_id,
            work_unit_id = %assignment.work_unit_id,
            node_id = %assignment.node_id,
            rank = assignment.rank,
            score = assignment.score,
            "placement criado"
        );

        Ok((plan, node.clone()))
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

    async fn reserve_workers(&self, node_ids: &[String]) {
        if node_ids.is_empty() {
            return;
        }

        let mut nodes = self.nodes.write().await;

        for node_id in node_ids {
            if let Some(node) = nodes.get_mut(node_id) {
                node.active_work_units = node.active_work_units.saturating_add(1);
            }
        }
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
        let mut work = request
            .into_inner()
            .work
            .ok_or_else(|| Status::invalid_argument("work é obrigatório"))?;
        work.inputs.clear();
        work.output_buffer_id.clear();
        work.output_buffer_pins = 0;

        let (placement_plan, node) = self.plan_single_work(&work).await?;
        if placement_plan.node_for(&work.work_id).is_none() {
            return Err(Status::internal("placement inválido para a work unit"));
        }

        let execution_result = execute_work_on_node(node.clone(), work.clone()).await;

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

    async fn submit_graph(
        &self,
        request: Request<SubmitGraphRequest>,
    ) -> Result<Response<SubmitGraphResponse>, Status> {
        let req = request.into_inner();

        if req.execution_id.is_empty() {
            return Err(Status::invalid_argument("execution_id é obrigatório"));
        }

        if req.units.is_empty() {
            return Err(Status::invalid_argument(
                "o grafo deve possuir ao menos uma work unit",
            ));
        }

        let mut graph = execution::ExecutionGraph::new(req.execution_id.clone());

        for graph_unit in req.units {
            let mut work = graph_unit
                .work
                .ok_or_else(|| Status::invalid_argument("work é obrigatória"))?;
            work.inputs.clear();
            work.output_buffer_id.clear();
            work.output_buffer_pins = 0;

            graph
                .add_unit(
                    execution::WorkUnitSpec::new(work)
                        .with_dependencies(graph_unit.depends_on)
                        .with_data_inputs(graph_unit.data_from)
                        .with_required_backends(graph_unit.required_backends),
                )
                .map_err(|error| Status::invalid_argument(error.to_string()))?;
        }

        let nodes = {
            let nodes = self.nodes.read().await;
            nodes.values().cloned().collect::<Vec<_>>()
        };

        let plan = execution::PlacementPlanner
            .plan(&graph, &nodes)
            .map_err(|error| Status::unavailable(error.to_string()))?;

        info!(
            execution_id = %plan.execution_id,
            work_units = graph.units.len(),
            assignments = plan.assignments.len(),
            "execution graph planejado"
        );

        let results = self.execute_graph(&graph, &plan).await?;

        Ok(Response::new(SubmitGraphResponse {
            accepted: true,
            message: "execution graph concluído".into(),
            execution_id: plan.execution_id,
            results,
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

        for (node, (row_start, row_count)) in workers.into_iter().zip(ranges) {
            let endpoint = node.worker_endpoint.clone();
            let work_id = req.work_id.clone();
            let a_start = row_start
                .checked_mul(inner)
                .and_then(|value| value.checked_mul(4))
                .ok_or_else(|| Status::internal("offset do shard de A excede limite"))?;
            let a_end = row_start
                .checked_add(row_count)
                .and_then(|end| end.checked_mul(inner))
                .and_then(|value| value.checked_mul(4))
                .ok_or_else(|| Status::internal("fim do shard de A excede limite"))?;
            let matrix_a = req.matrix_a[a_start..a_end].to_vec();
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
                        matrix_a_shard: matrix_a,
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

impl Controller {
    async fn execute_graph(
        &self,
        graph: &execution::ExecutionGraph,
        plan: &execution::PlacementPlan,
    ) -> Result<Vec<GraphWorkResult>, Status> {
        let levels = graph
            .levels()
            .map_err(|error| Status::invalid_argument(error.to_string()))?;

        let mut results = Vec::with_capacity(graph.units.len());
        let mut buffer_consumer_counts = HashMap::new();

        for unit in &graph.units {
            for source_work_id in &unit.data_from {
                let buffer_id = output_buffer_id(&graph.execution_id, source_work_id);
                *buffer_consumer_counts.entry(buffer_id).or_insert(0usize) += 1;
            }
        }

        for level in levels {
            let mut executions = Vec::with_capacity(level.len());
            let mut reserved_node_ids = Vec::with_capacity(level.len());

            let node_list = {
                let nodes = self.nodes.read().await;
                nodes.values().cloned().collect::<Vec<_>>()
            };

            for work_unit_id in &level {
                    let assignment = plan
                        .node_for(work_unit_id)
                        .ok_or_else(|| Status::internal("placement plan não contém a work unit"))?;

                    let node = node_list
                        .iter()
                        .find(|node| node.node_id == assignment.node_id)
                        .cloned()
                        .ok_or_else(|| Status::unavailable("node selecionado não está disponível"))?;

                    if node.worker_endpoint.is_empty() {
                        return Err(Status::unavailable(
                            "node selecionado não possui endpoint de worker",
                        ));
                    }

                    let unit = graph
                        .units
                        .iter()
                        .find(|unit| unit.work.work_id == *work_unit_id)
                        .ok_or_else(|| Status::internal("work unit não encontrada no grafo"))?;

                    let prepared = prepare_graph_work(
                        unit,
                        &graph.execution_id,
                        &self.buffers,
                        &assignment.node_id,
                        &node_list,
                    )
                    .await?;

                    for transfer in &prepared.1 {
                        info!(
                            work_unit_id = %work_unit_id,
                            buffer_id = %transfer.buffer_id,
                            bytes = transfer.byte_len,
                            estimated_us = transfer.estimated_duration_us,
                            strategy = ?transfer.strategy,
                            "transfer plan criado"
                        );
                    }

                    let local_output_pins = graph
                        .units
                        .iter()
                        .filter(|consumer| {
                            plan.node_for(&consumer.work.work_id)
                                .is_some_and(|candidate| {
                                    candidate.node_id.as_str() == assignment.node_id.as_str()
                                })
                        })
                        .map(|consumer| {
                            consumer
                                .data_from
                                .iter()
                                .filter(|source_id| source_id.as_str() == work_unit_id.as_str())
                                .count()
                        })
                        .sum::<usize>();

                    let mut prepared_work = prepared.0;
                    prepared_work.output_buffer_id =
                        output_buffer_id(&graph.execution_id, work_unit_id);
                    prepared_work.output_buffer_pins =
                        u32::try_from(local_output_pins).unwrap_or(u32::MAX);

                    executions.push((prepared_work, node));
                    reserved_node_ids.push(assignment.node_id.clone());
            }

            self.reserve_workers(&reserved_node_ids).await;

            let mut jobs = JoinSet::new();

            for (work, node) in executions {
                jobs.spawn(async move {
                    let result = execute_work_on_node(node, work.clone()).await?;
                    Ok::<GraphWorkResult, Status>(GraphWorkResult {
                        work_id: work.work_id,
                        accepted: result.accepted,
                        message: result.message,
                        node_id: result.node_id,
                        duration_us: result.duration_us,
                        output: result.output,
                    })
                });
            }

            let mut level_results = Vec::new();
            let mut execution_error = None;

            while let Some(joined) = jobs.join_next().await {
                match joined {
                    Ok(Ok(result)) if result.accepted => level_results.push(result),
                    Ok(Ok(result)) => {
                        execution_error = Some(Status::aborted(result.message));
                    }
                    Ok(Err(error)) => {
                        execution_error = Some(error);
                    }
                    Err(error) => {
                        execution_error = Some(Status::internal(format!(
                            "task da work unit falhou: {error}"
                        )));
                    }
                }
            }

            self.release_workers(&reserved_node_ids).await;

            if let Some(error) = execution_error {
                return Err(error);
            }

            level_results.sort_by(|left, right| left.work_id.cmp(&right.work_id));

            for result in &level_results {
                let buffer = BufferDescriptor {
                    buffer_id: output_buffer_id(&graph.execution_id, &result.work_id),
                    byte_len: result.output.len() as u64,
                    checksum: None,
                    location: DataLocation::Node {
                        node_id: result.node_id.clone(),
                    },
                };
                let pins = buffer_consumer_counts
                    .get(&buffer.buffer_id)
                    .copied()
                    .unwrap_or_default();

                let mut buffers = self.buffers.write().await;
                buffers
                    .insert_with_pins(buffer, result.output.clone(), pins)
                    .map_err(|error| Status::resource_exhausted(error.to_string()))?;
                info!(
                    work_unit_id = %result.work_id,
                    buffer_bytes = buffers.bytes_used(),
                    consumers = pins,
                    "output armazenado no buffer store do Controller"
                );
            }

            results.extend(level_results);
        }

        Ok(results)
    }
}

async fn prepare_graph_work(
    unit: &execution::WorkUnitSpec,
    execution_id: &str,
    buffers: &RwLock<buffer_store::BufferStore>,
    destination_node_id: &str,
    nodes: &[NodeInfo],
) -> Result<(WorkUnit, Vec<TransferPlan>), Status> {
    let mut work = unit.work.clone();
    let mut transfer_plans = Vec::with_capacity(unit.data_from.len());
    let destination = nodes
        .iter()
        .find(|node| node.node_id == destination_node_id)
        .ok_or_else(|| Status::unavailable("node de destino não encontrado"))?;

    let mut local_client = if unit.data_from.is_empty() || destination.worker_endpoint.is_empty() {
        None
    } else {
        tokio::time::timeout(
            Duration::from_millis(500),
            WorkerRuntimeClient::connect(destination.worker_endpoint.clone()),
        )
        .await
        .ok()
        .and_then(Result::ok)
    };

    for source_work_id in &unit.data_from {
        let buffer_id = output_buffer_id(execution_id, source_work_id);
        let cached_descriptor = {
            let mut store = buffers.write().await;
            store.get_descriptor(&buffer_id)
        };

        let local_copy_available = if let Some(client) = local_client.as_mut() {
            tokio::time::timeout(
                Duration::from_millis(250),
                client.has_buffer(Request::new(HasBufferRequest {
                    buffer_id: buffer_id.clone(),
                })),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .map(|response| response.into_inner().found)
            .unwrap_or(false)
        } else {
            false
        };

        let controller_cache_available = cached_descriptor.is_some();
        let cache_transfer_duration_us = cached_descriptor
            .as_ref()
            .map(|descriptor| estimate_controller_relay_us(descriptor.byte_len, destination));

        let decision = plan_buffer_access(BufferAccessOptions {
            local_copy_available,
            controller_cache_available,
            primary_available: false,
            cache_transfer_duration_us,
            primary_transfer_duration_us: None,
            recompute_duration_us: None,
        });

        info!(
            buffer_id = %buffer_id,
            destination_node_id = %destination_node_id,
            residency_source = ?decision.source,
            estimated_us = ?decision.estimated_duration_us,
            "decisão de residency criada"
        );

        match decision.source {
            Some(BufferAccessSource::Local) => {
                if controller_cache_available {
                    let mut store = buffers.write().await;
                    store.release_consumer(&buffer_id);
                }

                work.inputs.push(WorkInput {
                    buffer_id,
                    data: Vec::new(),
                    local_buffer: true,
                });
            }
            Some(BufferAccessSource::ControllerCache) => {
                let output = {
                    let mut store = buffers.write().await;
                    store.get_for_consumer(&buffer_id)
                }
                .ok_or_else(|| {
                    Status::failed_precondition(format!(
                        "buffer de saída deixou de estar disponível: {buffer_id}"
                    ))
                })?;

                let byte_len = output.bytes.len() as u64;
                transfer_plans.push(TransferPlan {
                    buffer_id: buffer_id.clone(),
                    source: DataLocation::Controller,
                    destination: DataLocation::Node {
                        node_id: destination_node_id.to_owned(),
                    },
                    strategy: TransferStrategy::ControllerRelay,
                    byte_len,
                    estimated_duration_us: estimate_controller_relay_us(byte_len, destination),
                });

                work.inputs.push(WorkInput {
                    buffer_id,
                    data: output.bytes,
                    local_buffer: false,
                });
            }
            Some(BufferAccessSource::Primary) => {
                return Err(Status::unimplemented(
                    "leitura remota da residência primária ainda não está implementada",
                ));
            }
            Some(BufferAccessSource::Recompute) => {
                return Err(Status::unimplemented(
                    "recomputação de buffer ainda não está implementada",
                ));
            }
            None => {
                return Err(Status::failed_precondition(format!(
                    "nenhuma fonte de buffer está disponível: {buffer_id}"
                )));
            }
        }
    }

    Ok((work, transfer_plans))
}

fn estimate_controller_relay_us(byte_len: u64, destination: &NodeInfo) -> u64 {
    let Some(capability) = destination.capability.as_ref() else {
        return u64::MAX / 2;
    };

    let throughput = capability.network_mb_per_sec;
    if !throughput.is_finite() || throughput <= 0.0 {
        return u64::MAX / 2;
    }

    let rtt = if capability.network_rtt_us.is_finite() && capability.network_rtt_us >= 0.0 {
        capability.network_rtt_us.ceil() as u64
    } else {
        0
    };
    let transfer_us =
        ((byte_len as f64 / (1024.0 * 1024.0)) / throughput * 1_000_000.0).ceil() as u64;
    rtt.saturating_add(transfer_us)
}

fn output_buffer_id(execution_id: &str, work_unit_id: &str) -> String {
    format!("{execution_id}:{work_unit_id}:output")
}

async fn execute_work_on_node(
    node: NodeInfo,
    work: WorkUnit,
) -> Result<ExecuteWorkResponse, Status> {
    let mut client = WorkerRuntimeClient::connect(node.worker_endpoint)
        .await
        .map_err(|error| Status::unavailable(format!("worker indisponível: {error}")))?;

    client
        .execute_work(Request::new(ExecuteWorkRequest { work: Some(work) }))
        .await
        .map_err(|error| Status::unavailable(format!("execução falhou: {error}")))
        .map(|response| response.into_inner())
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

        if row_count == 0 || row_start.saturating_add(row_count) > rows {
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

#[cfg(test)]
mod tests {
    use super::{
        buffer_store::BufferStore, execution::WorkUnitSpec, output_buffer_id, prepare_graph_work,
    };
    use real_ai_cluster_common::tensor::{
        BufferDescriptor, DataLocation, TransferStrategy,
    };
    use real_ai_cluster_protocol::v1::{Capability, NodeInfo, NodeState, WorkUnit};

    #[test]
    fn prepare_graph_work_copia_payload_e_anexa_outputs() {
        let unit = WorkUnitSpec::new(WorkUnit {
            work_id: "consumer".into(),
            operation: "echo".into(),
            payload: b"base".to_vec(),
            ..Default::default()
        })
        .with_data_inputs(vec!["producer".into()]);

        let buffers = tokio::sync::RwLock::new(BufferStore::new(1024));
        {
            let mut store = buffers.blocking_write();
            store
                .insert(
                    BufferDescriptor {
                        buffer_id: output_buffer_id("exec-1", "producer"),
                        byte_len: 7,
                        checksum: None,
                        location: DataLocation::Node {
                            node_id: "node-a".into(),
                        },
                    },
                    b"-output".to_vec(),
                )
                .unwrap();
        }
        let nodes = vec![
            NodeInfo {
                node_id: "node-a".into(),
                state: NodeState::Ready as i32,
                capability: Some(Capability {
                    network_rtt_us: 100.0,
                    network_mb_per_sec: 100.0,
                    ..Default::default()
                }),
                worker_endpoint: "http://node-a:50052".into(),
                ..Default::default()
            },
            NodeInfo {
                node_id: "node-b".into(),
                state: NodeState::Ready as i32,
                capability: Some(Capability {
                    network_rtt_us: 200.0,
                    network_mb_per_sec: 50.0,
                    ..Default::default()
                }),
                worker_endpoint: "http://node-b:50052".into(),
                ..Default::default()
            },
        ];

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (prepared, transfers) = runtime
            .block_on(prepare_graph_work(
                &unit, "exec-1", &buffers, "node-b", &nodes,
            ))
            .unwrap();

        assert_eq!(prepared.payload, b"base");
        assert_eq!(prepared.inputs.len(), 1);
        assert_eq!(prepared.inputs[0].buffer_id, output_buffer_id("exec-1", "producer"));
        assert_eq!(prepared.inputs[0].data.as_slice(), b"-output");
        assert!(!prepared.inputs[0].local_buffer);
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].strategy, TransferStrategy::ControllerRelay);
    }

    #[test]
    fn prepare_graph_work_nao_le_primary_remota_sem_cache() {
        let unit = WorkUnitSpec::new(WorkUnit {
            work_id: "consumer".into(),
            operation: "echo".into(),
            ..Default::default()
        })
        .with_data_inputs(vec!["producer".into()]);

        let buffers = tokio::sync::RwLock::new(BufferStore::new(1024));
        let nodes = vec![
            NodeInfo {
                node_id: "node-a".into(),
                state: NodeState::Ready as i32,
                capability: Some(Capability {
                    network_rtt_us: 100.0,
                    network_mb_per_sec: 100.0,
                    ..Default::default()
                }),
                worker_endpoint: "http://node-a:50052".into(),
                ..Default::default()
            },
            NodeInfo {
                node_id: "node-b".into(),
                state: NodeState::Ready as i32,
                capability: Some(Capability {
                    network_rtt_us: 100.0,
                    network_mb_per_sec: 100.0,
                    ..Default::default()
                }),
                worker_endpoint: "http://node-b:50052".into(),
                ..Default::default()
            },
        ];

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(prepare_graph_work(
            &unit, "exec-1", &buffers, "node-b", &nodes,
        ));

        assert!(matches!(
            result,
            Err(status) if status.code() == tonic::Code::FailedPrecondition
        ));
    }

    #[test]
    fn prepare_graph_work_rejeita_output_inexistente() {
        let unit = WorkUnitSpec::new(WorkUnit {
            work_id: "consumer".into(),
            operation: "echo".into(),
            ..Default::default()
        })
        .with_data_inputs(vec!["missing".into()]);

        let buffers = tokio::sync::RwLock::new(BufferStore::new(1024));
        let nodes = vec![NodeInfo {
            node_id: "node-b".into(),
            state: NodeState::Ready as i32,
            worker_endpoint: "http://node-b:50052".into(),
            ..Default::default()
        }];

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(prepare_graph_work(
            &unit, "exec-1", &buffers, "node-b", &nodes,
        ));

        assert!(result.is_err());
    }
}
