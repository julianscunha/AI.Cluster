use std::sync::Arc;

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::cluster_control_server::ClusterControl;
use ai_cluster_protocol::cluster::{
    EnqueueGraphRequest, EnqueueGraphResponse, EnqueueWorkRequest, EnqueueWorkResponse,
    HealthStreamRequest, HealthStreamResponse, HeartbeatRequest, HeartbeatResponse,
    LeaveNodeRequest, LeaveNodeResponse, ListEventsRequest, ListEventsResponse, ListNodesRequest,
    ListNodesResponse, RegisterNodeRequest, RegisterNodeResponse, RequestWorkRequest,
    RequestWorkResponse, SubmitWorkResultRequest, SubmitWorkResultResponse, WorkReportEntry,
    WorkReportRequest, WorkReportResponse, WorkUnit,
};
use tonic::{Request, Response, Status};

use crate::events::EventLog;
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

/// Placement is on unless explicitly disabled. `AI_CLUSTER_PLACEMENT=off` restores
/// first-come-first-served dispatch, so the benchmark can compare the two policies
/// on the same workload without a rebuild.
fn placement_enabled() -> bool {
    !matches!(
        std::env::var("AI_CLUSTER_PLACEMENT").as_deref(),
        Ok("off") | Ok("0") | Ok("false")
    )
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
        self.registry
            .register(identity, request.capability.unwrap_or_default(), now);
        self.events.record(now, "node_registered", node_id.clone());
        Ok(Response::new(RegisterNodeResponse {
            accepted: true,
            cluster_id: self.cluster_id.clone(),
            message: format!("nó {node_id} registrado"),
        }))
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
        Ok(Response::new(HeartbeatResponse { acknowledged }))
    }

    async fn report_health(
        &self,
        request: Request<HealthStreamRequest>,
    ) -> Result<Response<HealthStreamResponse>, Status> {
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        let accepted = self
            .registry
            .report_health(&request.node_id, request.health.unwrap_or_default());
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
        // Unknown, leaving or dead nodes get no work.
        if !self.registry.is_available(&request.node_id) {
            return Err(Status::failed_precondition(
                "nó não está disponível para trabalho",
            ));
        }
        // Candidate filtering: the unit's declared requirement is checked against
        // what this node reported, so a weak node is not handed work it cannot run.
        let cpu_threads = self
            .registry
            .capability(&request.node_id)
            .map(|capability| capability.cpu_threads)
            .unwrap_or(0);
        // Placement (G3): rank the asking node against the other idle candidates and
        // hand it a unit only when it is the cheapest fit. A node that is not the best
        // fit gets nothing and polls again, so the unit waits for the node that fits
        // it better instead of going to whoever asked first.
        let claimed = if placement_enabled() {
            let candidates = self.registry.candidates();
            self.work
                .claim_placed(&request.node_id, cpu_threads, unix_ms(), &candidates)
        } else {
            self.work.claim(&request.node_id, cpu_threads, unix_ms())
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
        let request = request.into_inner();
        if request.rows == 0 || request.shared == 0 || request.cols == 0 {
            return Err(Status::invalid_argument(
                "todas as dimensões da matriz devem ser maiores que zero",
            ));
        }
        let repetitions = request.repetitions.max(1);
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
            }
        });
        let enqueued = self.work.enqueue(units);
        self.events.record(
            unix_ms(),
            "work_enqueued",
            format!(
                "{} unidades {}x{}x{}",
                enqueued, request.rows, request.shared, request.cols
            ),
        );
        Ok(Response::new(EnqueueWorkResponse {
            enqueued: enqueued as u32,
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
        let request = request.into_inner();
        if request.size == 0 {
            return Err(Status::invalid_argument(
                "o tamanho deve ser maior que zero",
            ));
        }
        let size = request.size;
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
            format!("{enqueued} unidades tamanho={size}"),
        );
        Ok(Response::new(EnqueueGraphResponse {
            enqueued: enqueued as u32,
            blocked: self.work.blocked() as u32,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_cluster_protocol::cluster::{
        CapabilityReport, HealthReport, NodeIdentity, WorkResult, WorkUnit,
    };

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
            }))
            .await
            .unwrap_err();
        assert_eq!(blank.code(), tonic::Code::InvalidArgument);

        let ghost = service
            .heartbeat(Request::new(HeartbeatRequest {
                node_id: "ghost".to_string(),
                unix_ms: 0,
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
            }))
            .await
            .unwrap();

        let enqueued = service
            .enqueue_work(Request::new(EnqueueWorkRequest {
                rows: 4,
                shared: 4,
                cols: 4,
                seed: 7,
                repetitions: 2,
                min_cpu_threads: 0,
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
            .enqueue_work(Request::new(EnqueueWorkRequest {
                rows: 0,
                shared: 4,
                cols: 4,
                seed: 1,
                repetitions: 1,
                min_cpu_threads: 0,
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
            .enqueue_graph(Request::new(EnqueueGraphRequest { size: 2, seed: 7 }))
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
            .enqueue_graph(Request::new(EnqueueGraphRequest { size: 0, seed: 7 }))
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
                }))
                .await
                .unwrap();
        }
        work_store.enqueue([unit("w1")]);

        // The weak node polls first, but the strong node is idle and cheaper, so the
        // weak node yields and the unit stays queued. First-come-first-served would
        // have handed it to the weak node.
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
}
