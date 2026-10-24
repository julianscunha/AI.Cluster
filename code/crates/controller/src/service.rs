use std::sync::Arc;

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::cluster_control_server::ClusterControl;
use ai_cluster_protocol::cluster::link_probe_server::LinkProbe;
use ai_cluster_protocol::cluster::{
    ApproveNodeRequest, ApproveNodeResponse, EnqueueGraphRequest, EnqueueGraphResponse,
    EnqueueWorkRequest, EnqueueWorkResponse, GetMetricsRequest, GetTraceRequest, GetTraceResponse,
    HealthStreamRequest, HealthStreamResponse, HeartbeatRequest, HeartbeatResponse,
    LeaveNodeRequest, LeaveNodeResponse, ListEventsRequest, ListEventsResponse, ListNodesRequest,
    ListNodesResponse, ListReservationsRequest, ListReservationsResponse, MetricsResponse,
    ProbeLinkRequest, ProbeLinkResponse, RegisterNodeRequest, RegisterNodeResponse,
    ReleaseReservationRequest, ReleaseReservationResponse, RequestWorkRequest, RequestWorkResponse,
    Reservation, ReserveRequest, ReserveResponse, SubmitWorkResultRequest,
    SubmitWorkResultResponse, WorkReportEntry, WorkReportRequest, WorkReportResponse, WorkUnit,
};
use ai_cluster_protocol::metadata::require_operator;
use tonic::{Request, Response, Status};

/// Message ceiling for the `LinkProbe` service, enforced by the server before the payload is
/// buffered: the real probe sends 2 MiB, and the 64 MiB data limit is sized for work units.
pub const MAX_PROBE_BYTES: usize = 4 * 1024 * 1024;

use crate::events::EventLog;
use crate::metrics;
use crate::registry::{Registry, unix_ms};
use crate::work::WorkStore;

#[derive(Debug)]
pub struct ClusterControlService {
    registry: Arc<Registry>,
    work: Arc<WorkStore>,
    events: Arc<EventLog>,
    cluster_id: String,
}

impl ClusterControlService {
    pub fn new(
        registry: Arc<Registry>,
        work: Arc<WorkStore>,
        events: Arc<EventLog>,
        cluster_id: impl Into<String>,
    ) -> Self {
        Self {
            registry,
            work,
            events,
            cluster_id: cluster_id.into(),
        }
    }

    pub fn work(&self) -> &Arc<WorkStore> {
        &self.work
    }
}

/// Hysteresis band for the placement decision (G5), as a fraction of the cheapest
/// candidate's cost. A node yields only when another candidate is cheaper by more than
/// this much, so a near-equal node takes the unit instead of waiting a poll interval
/// for a saving that may not exist. Zero means "strictly cheapest", the pre-G5 policy.
const DEFAULT_PLACEMENT_TOLERANCE: f64 = 0.10;

/// Placement is on unless explicitly disabled. `AI_CLUSTER_PLACEMENT=off` restores
/// first-come-first-served dispatch, so the benchmark can compare the two policies
/// on the same workload without a rebuild.
fn placement_enabled() -> bool {
    !matches!(
        std::env::var("AI_CLUSTER_PLACEMENT").as_deref(),
        Ok("off") | Ok("0") | Ok("false")
    )
}

/// Reads the hysteresis band from `AI_CLUSTER_PLACEMENT_TOLERANCE`.
///
/// See [`clamp_tolerance`] for what happens to a bad value.
fn placement_tolerance() -> f64 {
    clamp_tolerance(
        std::env::var("AI_CLUSTER_PLACEMENT_TOLERANCE")
            .ok()
            .and_then(|value| value.trim().parse::<f64>().ok()),
    )
}

/// The band, clamped so a bad value cannot break the placement decision.
///
/// A negative band would sit below the cheapest cost, so every candidate would yield and
/// the unit would be stranded; a non-finite one would poison the comparison for everyone.
/// A negative value clamps to zero; a non-finite or missing value falls back to the
/// default. Split from the env lookup so the edge cases can be tested exactly.
fn clamp_tolerance(parsed: Option<f64>) -> f64 {
    match parsed {
        Some(value) if value.is_finite() => value.max(0.0),
        _ => DEFAULT_PLACEMENT_TOLERANCE,
    }
}

#[tonic::async_trait]
impl ClusterControl for ClusterControlService {
    async fn register_node(
        &self,
        request: Request<RegisterNodeRequest>,
    ) -> Result<Response<RegisterNodeResponse>, Status> {
        let request = request.into_inner();
        let identity = request
            .identity
            .ok_or_else(|| Status::invalid_argument("identidade é obrigatória"))?;
        if identity.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        let node_id = identity.node_id.clone();
        let now = unix_ms();
        // A node offers the copy of the state it holds. A controller that lost its own files
        // takes the newest one — a node keeping the copy is exactly what lets the queue come
        // back after the volume dies (I4).
        let record = self
            .registry
            .register(identity, request.capability.unwrap_or_default(), now);
        // S2: an unapproved node must not be able to write controller state, so its copy is
        // ignored until it is trusted. A node that comes back after approval offers it again.
        if let Some(replica) = request.replica.filter(|_| record.trusted) {
            self.work.restore(&replica);
        }
        self.events.record(now, "node_registered", node_id.clone());
        Ok(Response::new(RegisterNodeResponse {
            accepted: true,
            cluster_id: self.cluster_id.clone(),
            message: if record.trusted {
                format!("nó {node_id} registrado")
            } else {
                format!("nó {node_id} registrado, aguardando aprovação")
            },
        }))
    }

    /// Trusts a node so it can receive work (S2). Unknown nodes answer `approved: false`.
    async fn approve_node(
        &self,
        request: Request<ApproveNodeRequest>,
    ) -> Result<Response<ApproveNodeResponse>, Status> {
        require_operator(&request)?;
        let node_id = request.into_inner().node_id;
        if node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        let approved = self.registry.approve(&node_id);
        if approved {
            self.events.record(unix_ms(), "node_approved", node_id);
        }
        Ok(Response::new(ApproveNodeResponse { approved }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        // The node-supplied timestamp is advisory only; liveness uses controller time.
        let acknowledged = self.registry.heartbeat(&request.node_id, unix_ms());
        // A node that is behind gets the current copy; one that is level with it gets nothing,
        // so a beat stays a beat (I4). The node acknowledges by reporting the epoch back on
        // the next beat, so this needs no per-node bookkeeping — and a node that leaves leaves
        // nothing behind.
        let replica = self
            .work
            .state_replica()
            .filter(|replica| replica.epoch_unix_ms > request.replica_epoch_unix_ms)
            .map(|replica| (*replica).clone());
        Ok(Response::new(HeartbeatResponse {
            acknowledged,
            replica,
        }))
    }

    async fn report_health(
        &self,
        request: Request<HealthStreamRequest>,
    ) -> Result<Response<HealthStreamResponse>, Status> {
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        let accepted = self.registry.report_health(
            &request.node_id,
            request.health.unwrap_or_default(),
            request.capability,
        );
        Ok(Response::new(HealthStreamResponse { accepted }))
    }

    async fn list_nodes(
        &self,
        _request: Request<ListNodesRequest>,
    ) -> Result<Response<ListNodesResponse>, Status> {
        Ok(Response::new(ListNodesResponse {
            nodes: self.registry.list(),
        }))
    }

    /// Planned departure: stop dispatching to the node, hand its in-flight units
    /// back to the queue, then drop it from the registry.
    ///
    /// An unknown node is not an error — a node may leave twice, and a node that
    /// was already reaped as offline has nothing left to requeue.
    async fn leave_node(
        &self,
        request: Request<LeaveNodeRequest>,
    ) -> Result<Response<LeaveNodeResponse>, Status> {
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        if !self.registry.begin_drain(&request.node_id) {
            return Ok(Response::new(LeaveNodeResponse {
                accepted: false,
                requeued: 0,
            }));
        }
        let outcome = self.work.requeue_for_node(&request.node_id);
        self.registry.remove(&request.node_id);
        println!(
            "nó {} saiu; {} unidades em voo reenfileiradas, {} falharam",
            request.node_id, outcome.requeued, outcome.failed
        );
        self.events.record(
            unix_ms(),
            "node_left",
            format!(
                "{} reenfileiradas={} falharam={}",
                request.node_id, outcome.requeued, outcome.failed
            ),
        );
        Ok(Response::new(LeaveNodeResponse {
            accepted: true,
            requeued: outcome.requeued as u32,
        }))
    }

    async fn request_work(
        &self,
        request: Request<RequestWorkRequest>,
    ) -> Result<Response<RequestWorkResponse>, Status> {
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        if self.registry.awaiting_approval(&request.node_id) {
            return Ok(Response::new(RequestWorkResponse::default()));
        }
        // Unknown, leaving or dead nodes get no work.
        if !self.registry.is_available(&request.node_id) {
            return Err(Status::failed_precondition(
                "nó não está disponível para trabalho",
            ));
        }
        // Candidate filtering: the unit's declared requirement and its own footprint
        // are checked against what this node reported, so a node that cannot run the
        // unit is never handed it.
        let capability = self
            .registry
            .capability(&request.node_id)
            .unwrap_or_default();
        // Placement (G3): rank the asking node against the other idle candidates and
        // hand it a unit only when it is cheap enough for it — cheapest, or within the
        // hysteresis band (G5) of cheapest. A node that is neither gets nothing and
        // polls again, so the unit waits for the node that fits it better instead of
        // going to whoever asked first.
        let claimed = if placement_enabled() {
            let candidates = self.registry.candidates();
            self.work.claim_placed(
                &request.node_id,
                &capability,
                unix_ms(),
                &candidates,
                placement_tolerance(),
            )
        } else {
            self.work.claim(&request.node_id, &capability, unix_ms())
        };
        match claimed {
            Some(work) => Ok(Response::new(RequestWorkResponse {
                has_work: true,
                work: Some(work),
            })),
            None => Ok(Response::new(RequestWorkResponse {
                has_work: false,
                work: None,
            })),
        }
    }

    async fn submit_work_result(
        &self,
        request: Request<SubmitWorkResultRequest>,
    ) -> Result<Response<SubmitWorkResultResponse>, Status> {
        let result = request
            .into_inner()
            .result
            .ok_or_else(|| Status::invalid_argument("resultado é obrigatório"))?;
        if result.work_id.trim().is_empty() {
            return Err(Status::invalid_argument("work_id é obrigatório"));
        }
        match self.work.complete(result, unix_ms()) {
            Some(completed) => {
                println!(
                    "trabalho {} finalizado em {} compute={}us transfer={}us total={}us checksum={:.6}",
                    completed.work_id,
                    completed.node_id,
                    completed.compute_time_us,
                    completed.transfer_time_us,
                    completed.total_latency_us,
                    completed.checksum,
                );
                self.events.record(
                    unix_ms(),
                    "work_completed",
                    format!(
                        "{} em {} compute={}us transfer={}us",
                        completed.work_id,
                        completed.node_id,
                        completed.compute_time_us,
                        completed.transfer_time_us
                    ),
                );
                Ok(Response::new(SubmitWorkResultResponse {
                    accepted: true,
                    message: "resultado registrado".to_string(),
                }))
            }
            None => Ok(Response::new(SubmitWorkResultResponse {
                accepted: false,
                message: "trabalho não foi despachado por este controller".to_string(),
            })),
        }
    }

    async fn enqueue_work(
        &self,
        request: Request<EnqueueWorkRequest>,
    ) -> Result<Response<EnqueueWorkResponse>, Status> {
        require_operator(&request)?;
        let request = request.into_inner();
        if request.rows == 0 || request.shared == 0 || request.cols == 0 {
            return Err(Status::invalid_argument(
                "todas as dimensões da matriz devem ser maiores que zero",
            ));
        }
        let repetitions = request.repetitions.max(1);
        let request_id = if request.request_id.is_empty() {
            self.work.next_request_id()
        } else {
            request.request_id.clone()
        };
        let units = (0..repetitions).map(|index| {
            let seed = request.seed.wrapping_add(u64::from(index));
            // A and B get different seeds so the product is not a square of one matrix.
            let a = Matrix::seeded(request.rows, request.shared, seed);
            let b = Matrix::seeded(request.shared, request.cols, seed ^ 0x9E37_79B9_7F4A_7C15);
            WorkUnit {
                work_id: self.work.next_work_id(),
                rows: request.rows,
                shared: request.shared,
                cols: request.cols,
                matrix_a: a.data,
                matrix_b: b.data,
                depends_on: Vec::new(),
                matrix_a_from: String::new(),
                matrix_b_from: String::new(),
                min_cpu_threads: request.min_cpu_threads,
                request_id: request_id.clone(),
            }
        });
        let enqueued = self.work.enqueue(units);
        self.events.record(
            unix_ms(),
            "work_enqueued",
            format!(
                "{} unidades {}x{}x{} req={request_id}",
                enqueued, request.rows, request.shared, request.cols
            ),
        );
        Ok(Response::new(EnqueueWorkResponse {
            enqueued: enqueued as u32,
            request_id,
        }))
    }

    /// Enqueues a small diamond with real data flow: A -> (B, C) -> D.
    ///
    /// B and C take A's output as their left input and a seeded matrix as their
    /// right input, so they run in parallel after A. D takes B's output and C's
    /// output as its two inputs, so it runs only once both branches have finished.
    /// This is the smallest graph that exercises gating, parallelism and the
    /// hand-off of one unit's output into the next unit's input.
    async fn enqueue_graph(
        &self,
        request: Request<EnqueueGraphRequest>,
    ) -> Result<Response<EnqueueGraphResponse>, Status> {
        require_operator(&request)?;
        let request = request.into_inner();
        if request.size == 0 {
            return Err(Status::invalid_argument(
                "o tamanho deve ser maior que zero",
            ));
        }
        let size = request.size;
        let request_id = if request.request_id.is_empty() {
            self.work.next_request_id()
        } else {
            request.request_id.clone()
        };
        let first = self.work.next_work_id();
        let left = self.work.next_work_id();
        let right = self.work.next_work_id();
        let join = self.work.next_work_id();
        // A bound input is filled at dispatch from its source's output, so its
        // payload stays empty here. The salt only seeds the non-bound operand.
        let make = |work_id: String,
                    depends_on: Vec<String>,
                    a_from: String,
                    b_from: String,
                    salt: u64| {
            let a = Matrix::seeded(size, size, request.seed);
            let b = Matrix::seeded(size, size, request.seed ^ salt);
            WorkUnit {
                work_id,
                rows: size,
                shared: size,
                cols: size,
                matrix_a: if a_from.is_empty() {
                    a.data
                } else {
                    Vec::new()
                },
                matrix_b: if b_from.is_empty() {
                    b.data
                } else {
                    Vec::new()
                },
                depends_on,
                matrix_a_from: a_from,
                matrix_b_from: b_from,
                // Graph units carry no requirement; size is the same for all of them.
                min_cpu_threads: 0,
                request_id: request_id.clone(),
            }
        };
        let units = vec![
            // A: both operands seeded.
            make(
                first.clone(),
                Vec::new(),
                String::new(),
                String::new(),
                0x9E37_79B9_7F4A_7C15,
            ),
            // B and C: left operand from A, distinct seeded right operands.
            make(
                left.clone(),
                vec![first.clone()],
                first.clone(),
                String::new(),
                0x1234_5678_9ABC_DEF0,
            ),
            make(
                right.clone(),
                vec![first.clone()],
                first.clone(),
                String::new(),
                0x0FED_CBA9_8765_4321,
            ),
            // D: both operands from the two branches.
            make(join, vec![left.clone(), right.clone()], left, right, 0),
        ];
        let enqueued = self.work.enqueue(units);
        self.events.record(
            unix_ms(),
            "graph_enqueued",
            format!("{enqueued} unidades tamanho={size} req={request_id}"),
        );
        Ok(Response::new(EnqueueGraphResponse {
            enqueued: enqueued as u32,
            blocked: self.work.blocked() as u32,
            request_id,
        }))
    }

    async fn work_report(
        &self,
        _request: Request<WorkReportRequest>,
    ) -> Result<Response<WorkReportResponse>, Status> {
        let completed = self
            .work
            .completed()
            .into_iter()
            .map(|record| WorkReportEntry {
                work_id: record.work_id,
                node_id: record.node_id,
                compute_time_us: record.compute_time_us,
                transfer_time_us: record.transfer_time_us,
                total_latency_us: record.total_latency_us,
                checksum: record.checksum,
            })
            .collect();
        Ok(Response::new(WorkReportResponse {
            completed,
            pending: self.work.pending() as u32,
            failed: self.work.failed() as u32,
        }))
    }

    async fn list_events(
        &self,
        request: Request<ListEventsRequest>,
    ) -> Result<Response<ListEventsResponse>, Status> {
        let limit = request.into_inner().limit as usize;
        Ok(Response::new(ListEventsResponse {
            events: self.events.recent(limit),
        }))
    }

    async fn get_metrics(
        &self,
        _request: Request<GetMetricsRequest>,
    ) -> Result<Response<MetricsResponse>, Status> {
        // Node counts come from one registry lock and work counts from the work store, so
        // the read is a consistent-enough snapshot: a unit completing between the two calls
        // can land in either bucket, never both. These are point-in-time metrics, not a
        // transaction.
        let (nodes_total, nodes_available) = self.registry.counts();
        let completed = self.work.completed();
        Ok(Response::new(metrics::summarize(
            nodes_total,
            nodes_available,
            &completed,
            self.work.pending() as u32,
            self.work.blocked() as u32,
            self.work.failed() as u32,
        )))
    }

    async fn get_trace(
        &self,
        request: Request<GetTraceRequest>,
    ) -> Result<Response<GetTraceResponse>, Status> {
        let request_id = request.into_inner().request_id;
        Ok(Response::new(GetTraceResponse {
            units: self.work.trace(&request_id),
            request_id,
        }))
    }

    /// Retém um nó para uma requisição futura (G6).
    ///
    /// O nó precisa existir e estar disponível: reservar um nó que o controller não
    /// entregaria trabalho nenhum não protege nada, então isso é recusado em vez de
    /// aceito e ignorado. Uma reserva nova sobre o mesmo nó substitui a anterior.
    async fn reserve(
        &self,
        request: Request<ReserveRequest>,
    ) -> Result<Response<ReserveResponse>, Status> {
        require_operator(&request)?;
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        if request.request_id.trim().is_empty() {
            return Err(Status::invalid_argument("request_id é obrigatório"));
        }
        if !self.registry.is_available(&request.node_id) {
            return Ok(Response::new(ReserveResponse {
                granted: false,
                expires_at_unix_ms: 0,
                message: format!("nó {} não está disponível para reserva", request.node_id),
            }));
        }
        let now = unix_ms();
        let reservation =
            self.work
                .reserve(&request.node_id, &request.request_id, request.ttl_ms, now);
        self.events.record(
            now,
            "node_reserved",
            format!(
                "{} retido para {} até {}",
                request.node_id, request.request_id, reservation.expires_at_unix_ms
            ),
        );
        Ok(Response::new(ReserveResponse {
            granted: true,
            expires_at_unix_ms: reservation.expires_at_unix_ms,
            message: String::new(),
        }))
    }

    async fn list_reservations(
        &self,
        _request: Request<ListReservationsRequest>,
    ) -> Result<Response<ListReservationsResponse>, Status> {
        let reservations = self
            .work
            .reservations(unix_ms())
            .into_iter()
            .map(|(node_id, reservation)| Reservation {
                node_id,
                request_id: reservation.request_id,
                expires_at_unix_ms: reservation.expires_at_unix_ms,
            })
            .collect();
        Ok(Response::new(ListReservationsResponse { reservations }))
    }

    async fn release_reservation(
        &self,
        request: Request<ReleaseReservationRequest>,
    ) -> Result<Response<ReleaseReservationResponse>, Status> {
        require_operator(&request)?;
        let node_id = request.into_inner().node_id;
        if node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        let released = self.work.release_reservation(&node_id);
        if released {
            self.events
                .record(unix_ms(), "reservation_released", node_id.clone());
        }
        Ok(Response::new(ReleaseReservationResponse { released }))
    }
}

/// Throughput probe (C2): the worker times the upload, so the answer is only the byte count.
#[derive(Debug, Default)]
pub struct LinkProbeService;

#[tonic::async_trait]
impl LinkProbe for LinkProbeService {
    async fn probe_link(
        &self,
        request: Request<ProbeLinkRequest>,
    ) -> Result<Response<ProbeLinkResponse>, Status> {
        Ok(Response::new(ProbeLinkResponse {
            received_bytes: request.into_inner().payload.len() as u64,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_cluster_protocol::cluster::{
        CapabilityReport, HealthReport, NodeIdentity, WorkResult, WorkUnit,
    };

    /// A request that arrived with the operator token, as the interceptor would mark it.
    fn op<T>(message: T) -> Request<T> {
        let mut request = Request::new(message);
        request
            .extensions_mut()
            .insert(ai_cluster_protocol::metadata::Operator);
        request
    }

    fn service_with_work() -> (ClusterControlService, Arc<WorkStore>) {
        let work = Arc::new(WorkStore::new());
        let service = ClusterControlService::new(
            Arc::new(Registry::new()),
            work.clone(),
            Arc::new(EventLog::new()),
            "test-cluster",
        );
        (service, work)
    }

    fn service_with_registry_and_work() -> (ClusterControlService, Arc<Registry>, Arc<WorkStore>) {
        let registry = Arc::new(Registry::new());
        let work = Arc::new(WorkStore::new());
        let service = ClusterControlService::new(
            registry.clone(),
            work.clone(),
            Arc::new(EventLog::new()),
            "test-cluster",
        );
        (service, registry, work)
    }

    fn service() -> ClusterControlService {
        service_with_work().0
    }

    fn identity(node_id: &str) -> NodeIdentity {
        NodeIdentity {
            node_id: node_id.to_string(),
            device_id: format!("{node_id}-device"),
            ..Default::default()
        }
    }

    /// A unit large enough that compute dominates the fixed transfer term, so a thread
    /// difference is a real cost difference. See the same helper in `work.rs`.
    fn big_unit(work_id: &str) -> WorkUnit {
        WorkUnit {
            work_id: work_id.to_string(),
            rows: 64,
            shared: 64,
            cols: 64,
            matrix_a: vec![0.0; 64 * 64],
            matrix_b: vec![0.0; 64 * 64],
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
            request_id: "req-test".to_string(),
        }
    }

    fn unit(work_id: &str) -> WorkUnit {
        WorkUnit {
            work_id: work_id.to_string(),
            rows: 2,
            shared: 2,
            cols: 2,
            matrix_a: vec![1.0, 2.0, 3.0, 4.0],
            matrix_b: vec![5.0, 6.0, 7.0, 8.0],
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
            request_id: "req-test".to_string(),
        }
    }

    #[tokio::test]
    async fn registration_heartbeat_health_and_list_round_trip() {
        let service = service();
        let registered = service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: Some(CapabilityReport {
                    cpu_threads: 4,
                    ..Default::default()
                }),
                replica: None,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(registered.accepted);
        assert_eq!(registered.cluster_id, "test-cluster");

        let heartbeat = service
            .heartbeat(Request::new(HeartbeatRequest {
                node_id: "node-a".to_string(),
                unix_ms: 123,
                replica_epoch_unix_ms: 0,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(heartbeat.acknowledged);

        let health = service
            .report_health(Request::new(HealthStreamRequest {
                node_id: "node-a".to_string(),
                health: Some(HealthReport {
                    health_score: 0.9,
                    ..Default::default()
                }),
                capability: None,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(health.accepted);

        let listed = service
            .list_nodes(Request::new(ListNodesRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(listed.nodes.len(), 1);
        assert_eq!(listed.nodes[0].state, "READY");
        assert_eq!(listed.nodes[0].capability.as_ref().unwrap().cpu_threads, 4);
        assert_eq!(listed.nodes[0].health.as_ref().unwrap().health_score, 0.9);
    }

    #[tokio::test]
    async fn missing_identity_and_unknown_node_are_rejected() {
        let service = service();

        let missing = service
            .register_node(Request::new(RegisterNodeRequest::default()))
            .await
            .unwrap_err();
        assert_eq!(missing.code(), tonic::Code::InvalidArgument);

        let blank = service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(NodeIdentity::default()),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(blank.code(), tonic::Code::InvalidArgument);

        let ghost = service
            .heartbeat(Request::new(HeartbeatRequest {
                node_id: "ghost".to_string(),
                unix_ms: 0,
                replica_epoch_unix_ms: 0,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!ghost.acknowledged);
    }

    #[tokio::test]
    async fn work_dispatch_and_result_round_trip() {
        let (service, work_store) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        let idle = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!idle.has_work);
        assert!(idle.work.is_none());

        work_store.enqueue([unit("w1")]);
        let claimed = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(claimed.has_work);
        let claimed = claimed.work.expect("work unit present");
        assert_eq!(claimed.work_id, "w1");
        assert_eq!(claimed.matrix_a.len(), 4);

        let accepted = service
            .submit_work_result(Request::new(SubmitWorkResultRequest {
                result: Some(WorkResult {
                    work_id: "w1".to_string(),
                    node_id: "node-a".to_string(),
                    rows: 2,
                    cols: 2,
                    matrix_c: vec![19.0, 22.0, 43.0, 50.0],
                    checksum: 134.0,
                    compute_time_us: 1_000,
                }),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(accepted.accepted);
        assert_eq!(work_store.completed().len(), 1);
        assert_eq!(work_store.completed()[0].work_id, "w1");
    }

    #[tokio::test]
    async fn work_requests_require_a_registered_node() {
        let service = service();

        let blank = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "  ".to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(blank.code(), tonic::Code::InvalidArgument);

        let unknown = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "ghost".to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(unknown.code(), tonic::Code::FailedPrecondition);

        let missing = service
            .submit_work_result(Request::new(SubmitWorkResultRequest { result: None }))
            .await
            .unwrap_err();
        assert_eq!(missing.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn enqueue_work_builds_units_matching_the_declared_shape() {
        let (service, work_store) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        let enqueued = service
            .enqueue_work(op(EnqueueWorkRequest {
                rows: 4,
                shared: 4,
                cols: 4,
                seed: 7,
                repetitions: 2,
                min_cpu_threads: 0,
                request_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(enqueued.enqueued, 2);
        assert_eq!(work_store.pending(), 2);

        let claimed = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner()
            .work
            .expect("work unit present");
        assert_eq!(claimed.rows, 4);
        assert_eq!(claimed.shared, 4);
        assert_eq!(claimed.cols, 4);
        assert_eq!(claimed.matrix_a.len(), 16);
        assert_eq!(claimed.matrix_b.len(), 16);

        let invalid = service
            .enqueue_work(op(EnqueueWorkRequest {
                rows: 0,
                shared: 4,
                cols: 4,
                seed: 1,
                repetitions: 1,
                min_cpu_threads: 0,
                request_id: String::new(),
            }))
            .await
            .unwrap_err();
        assert_eq!(invalid.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn leave_node_requeues_in_flight_work_and_drops_the_node() {
        let (service, _registry, work_store) = service_with_registry_and_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        work_store.enqueue([unit("w1")]);
        let claimed = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(claimed.has_work);
        assert_eq!(work_store.pending(), 1);

        let left = service
            .leave_node(Request::new(LeaveNodeRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(left.accepted);
        assert_eq!(left.requeued, 1);

        // The node is gone and its unit is back in the queue.
        let listed = service
            .list_nodes(Request::new(ListNodesRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert!(listed.nodes.is_empty());
        assert_eq!(work_store.pending(), 1);

        // Leaving twice reports accepted=false rather than failing the call.
        let again = service
            .leave_node(Request::new(LeaveNodeRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!again.accepted);
        assert_eq!(again.requeued, 0);

        // A blank node id is a client error.
        let blank = service
            .leave_node(Request::new(LeaveNodeRequest {
                node_id: "  ".to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(blank.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn request_work_is_refused_for_an_offline_node() {
        let (service, registry, _work) = service_with_registry_and_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        registry.mark_stale_offline(unix_ms() + 20_000, 10_000);
        let refused = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn enqueue_graph_builds_a_diamond_with_blocked_branches() {
        let (service, work_store) = service_with_work();
        let response = service
            .enqueue_graph(op(EnqueueGraphRequest {
                size: 2,
                seed: 7,
                request_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.enqueued, 4);
        // A is ready; B, C and D wait on their dependencies.
        assert_eq!(response.blocked, 3);
        assert_eq!(work_store.pending(), 4);
        assert_eq!(work_store.blocked(), 3);

        // A size of zero is a client error.
        let invalid = service
            .enqueue_graph(op(EnqueueGraphRequest {
                size: 0,
                seed: 7,
                request_id: String::new(),
            }))
            .await
            .unwrap_err();
        assert_eq!(invalid.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn placement_prefers_the_cheaper_idle_node() {
        let (service, _registry, work_store) = service_with_registry_and_work();
        for (id, threads) in [("node-weak", 2u32), ("node-strong", 8)] {
            service
                .register_node(Request::new(RegisterNodeRequest {
                    identity: Some(identity(id)),
                    capability: Some(CapabilityReport {
                        cpu_threads: threads,
                        ..Default::default()
                    }),
                    replica: None,
                }))
                .await
                .unwrap();
        }
        work_store.enqueue([big_unit("w1")]);

        // The weak node polls first, but the strong node is idle and cheaper, so the
        // weak node yields and the unit stays queued. First-come-first-served would
        // have handed it to the weak node.
        // The unit is 64x64x64 on purpose: on a small unit the thread-independent
        // transfer term dwarfs compute, the two costs land within the G5 hysteresis
        // band, and the weak node would rightly take it instead of yielding.
        let weak = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-weak".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!weak.has_work);
        assert!(weak.work.is_none());
        assert_eq!(work_store.pending(), 1);

        // The node that fits the unit better takes it.
        let strong = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-strong".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(strong.has_work);
        assert_eq!(strong.work.expect("strong node got w1").work_id, "w1");
    }

    #[tokio::test]
    async fn lifecycle_events_are_recorded_and_listed() {
        let (service, _work) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        let events = service
            .list_events(Request::new(ListEventsRequest { limit: 0 }))
            .await
            .unwrap()
            .into_inner()
            .events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "node_registered");
        assert!(events[0].detail.contains("node-a"));

        // A limit returns only the newest event, still oldest first.
        let limited = service
            .list_events(Request::new(ListEventsRequest { limit: 1 }))
            .await
            .unwrap()
            .into_inner()
            .events;
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].kind, "node_registered");
    }

    #[test]
    fn clamps_the_placement_tolerance_band() {
        // A negative band would sit below the cheapest cost and strand every unit, so
        // it clamps to zero — strictly cheapest.
        assert_eq!(clamp_tolerance(Some(-1.0)), 0.0);
        // A non-finite value would poison the comparison for every candidate.
        assert_eq!(clamp_tolerance(Some(f64::NAN)), DEFAULT_PLACEMENT_TOLERANCE);
        assert_eq!(
            clamp_tolerance(Some(f64::INFINITY)),
            DEFAULT_PLACEMENT_TOLERANCE
        );
        // Unset means the default, not zero.
        assert_eq!(clamp_tolerance(None), DEFAULT_PLACEMENT_TOLERANCE);
        // A real value passes through untouched.
        assert_eq!(clamp_tolerance(Some(0.25)), 0.25);
        assert_eq!(clamp_tolerance(Some(0.0)), 0.0);
    }

    #[tokio::test]
    async fn metrics_roll_up_nodes_and_completed_work() {
        let (service, work_store) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        // An idle cluster: a node is present, nothing has run, and the averages are zero
        // rather than a division by zero.
        let metrics = service
            .get_metrics(Request::new(GetMetricsRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(metrics.nodes_total, 1);
        assert_eq!(metrics.nodes_available, 1);
        assert_eq!(metrics.units_completed, 0);
        assert_eq!(metrics.units_pending, 0);
        assert_eq!(metrics.avg_compute_time_us, 0);

        work_store.enqueue([unit("w1")]);
        service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap();
        service
            .submit_work_result(Request::new(SubmitWorkResultRequest {
                result: Some(WorkResult {
                    work_id: "w1".to_string(),
                    node_id: "node-a".to_string(),
                    rows: 2,
                    cols: 2,
                    matrix_c: vec![19.0, 22.0, 43.0, 50.0],
                    checksum: 134.0,
                    compute_time_us: 1_000,
                }),
            }))
            .await
            .unwrap();

        // The unit moved from pending to completed and its compute time is summed. The
        // latency total is not asserted: it comes from the controller clock while compute
        // comes from the worker, so on a unit that finishes within one millisecond the
        // worker's self-report can exceed the observed wall time. That is a measurement
        // artifact of an instant test, not a bug — in production a unit takes milliseconds.
        let metrics = service
            .get_metrics(Request::new(GetMetricsRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(metrics.units_completed, 1);
        assert_eq!(metrics.units_pending, 0);
        assert_eq!(metrics.total_compute_time_us, 1_000);
        assert_eq!(metrics.avg_compute_time_us, 1_000);
    }

    #[tokio::test]
    async fn get_trace_follows_a_requests_units_through_the_service() {
        let (service, _work_store) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        // The service mints a request id when the client does not supply one.
        let enqueued = service
            .enqueue_work(op(EnqueueWorkRequest {
                rows: 2,
                shared: 2,
                cols: 2,
                seed: 7,
                repetitions: 1,
                min_cpu_threads: 0,
                request_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!enqueued.request_id.is_empty());

        // Before dispatch the unit is queued and carries no cost.
        let trace = service
            .get_trace(Request::new(GetTraceRequest {
                request_id: enqueued.request_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(trace.units.len(), 1);
        assert_eq!(trace.units[0].status, "queued");

        // Run the unit end to end: dispatch, compute, submit.
        let claimed = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner()
            .work
            .expect("work unit present");
        service
            .submit_work_result(Request::new(SubmitWorkResultRequest {
                result: Some(WorkResult {
                    work_id: claimed.work_id.clone(),
                    node_id: "node-a".to_string(),
                    rows: 2,
                    cols: 2,
                    matrix_c: vec![19.0, 22.0, 43.0, 50.0],
                    checksum: 134.0,
                    compute_time_us: 1_000,
                }),
            }))
            .await
            .unwrap();

        // The same request id now resolves to the completed unit and its timings.
        let trace = service
            .get_trace(Request::new(GetTraceRequest {
                request_id: enqueued.request_id,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(trace.units.len(), 1);
        assert_eq!(trace.units[0].work_id, claimed.work_id);
        assert_eq!(trace.units[0].status, "completed");
        assert_eq!(trace.units[0].node_id, "node-a");
        assert_eq!(trace.units[0].compute_time_us, 1_000);
    }

    /// A unit under a given client request, so a reservation can be matched to it (G6).
    fn unit_for(work_id: &str, request_id: &str) -> WorkUnit {
        WorkUnit {
            request_id: request_id.to_string(),
            ..unit(work_id)
        }
    }

    async fn register(service: &ClusterControlService, node_id: &str, threads: u32) {
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity(node_id)),
                capability: Some(CapabilityReport {
                    cpu_threads: threads,
                    ..Default::default()
                }),
                replica: None,
            }))
            .await
            .unwrap();
    }

    fn reserve_request(node_id: &str, request_id: &str, ttl_ms: u64) -> Request<ReserveRequest> {
        op(ReserveRequest {
            node_id: node_id.to_string(),
            request_id: request_id.to_string(),
            ttl_ms,
        })
    }

    #[tokio::test]
    async fn reserve_holds_a_registered_node_and_lists_the_hold() {
        let (service, _work) = service_with_work();
        register(&service, "node-a", 8).await;

        let granted = service
            .reserve(reserve_request("node-a", "req-x", 5_000))
            .await
            .unwrap()
            .into_inner();
        assert!(granted.granted);
        assert!(granted.expires_at_unix_ms > 0);

        let listed = service
            .list_reservations(Request::new(ListReservationsRequest {}))
            .await
            .unwrap()
            .into_inner()
            .reservations;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].node_id, "node-a");
        assert_eq!(listed[0].request_id, "req-x");
    }

    #[tokio::test]
    async fn a_reserved_node_stops_taking_another_requests_work() {
        let (service, _registry, work_store) = service_with_registry_and_work();
        // Only node-a exists, so if it refuses the unit nothing else can take it and the
        // refusal is unambiguously the hold, not the cost ranking.
        register(&service, "node-a", 8).await;
        work_store.enqueue([unit_for("w1", "req-x")]);
        service
            .reserve(reserve_request("node-a", "req-other", 5_000))
            .await
            .unwrap();

        let refused = service
            .request_work(Request::new(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!refused.has_work);
        assert_eq!(work_store.pending(), 1);
    }

    #[tokio::test]
    async fn reserve_refuses_an_unknown_node_without_an_error() {
        let (service, _work) = service_with_work();

        // Not reserving is a valid answer, not a transport failure: the client gets
        // granted=false and a reason it can print.
        let refused = service
            .reserve(reserve_request("ghost", "req-x", 0))
            .await
            .unwrap()
            .into_inner();
        assert!(!refused.granted);
        assert!(refused.message.contains("ghost"));
    }

    #[tokio::test]
    async fn reserve_requires_a_node_and_a_request_id() {
        let (service, _work) = service_with_work();

        let missing_node = service
            .reserve(reserve_request("  ", "req-x", 0))
            .await
            .unwrap_err();
        assert_eq!(missing_node.code(), tonic::Code::InvalidArgument);

        let missing_request = service
            .reserve(reserve_request("node-a", "", 0))
            .await
            .unwrap_err();
        assert_eq!(missing_request.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn release_reservation_frees_the_hold() {
        let (service, _work) = service_with_work();
        register(&service, "node-a", 8).await;
        service
            .reserve(reserve_request("node-a", "req-x", 5_000))
            .await
            .unwrap();

        let released = service
            .release_reservation(op(ReleaseReservationRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(released.released);

        // Releasing a node nobody holds is not an error, just false.
        let again = service
            .release_reservation(op(ReleaseReservationRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!again.released);
    }
}
