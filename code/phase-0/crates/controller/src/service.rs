use std::sync::Arc;

use ai_cluster_protocol::cluster::cluster_control_server::ClusterControl;
use ai_cluster_protocol::cluster::{
    HealthStreamRequest, HealthStreamResponse, HeartbeatRequest, HeartbeatResponse,
    ListNodesRequest, ListNodesResponse, RegisterNodeRequest, RegisterNodeResponse,
    RequestWorkRequest, RequestWorkResponse, SubmitWorkResultRequest, SubmitWorkResultResponse,
};
use tonic::{Request, Response, Status};

use crate::registry::{Registry, unix_ms};
use crate::work::WorkStore;

#[derive(Debug)]
pub struct ClusterControlService {
    registry: Arc<Registry>,
    work: Arc<WorkStore>,
    cluster_id: String,
}

impl ClusterControlService {
    pub fn new(
        registry: Arc<Registry>,
        work: Arc<WorkStore>,
        cluster_id: impl Into<String>,
    ) -> Self {
        Self {
            registry,
            work,
            cluster_id: cluster_id.into(),
        }
    }

    pub fn work(&self) -> &Arc<WorkStore> {
        &self.work
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
            .ok_or_else(|| Status::invalid_argument("identity is required"))?;
        if identity.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id is required"));
        }
        let node_id = identity.node_id.clone();
        self.registry
            .register(identity, request.capability.unwrap_or_default(), unix_ms());
        Ok(Response::new(RegisterNodeResponse {
            accepted: true,
            cluster_id: self.cluster_id.clone(),
            message: format!("node {node_id} registered"),
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id is required"));
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
            return Err(Status::invalid_argument("node_id is required"));
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

    async fn request_work(
        &self,
        request: Request<RequestWorkRequest>,
    ) -> Result<Response<RequestWorkResponse>, Status> {
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id is required"));
        }
        // An unknown node gets no work; it must register first.
        if !self.registry.knows(&request.node_id) {
            return Err(Status::failed_precondition("node is not registered"));
        }
        match self.work.claim(&request.node_id, unix_ms()) {
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
            .ok_or_else(|| Status::invalid_argument("result is required"))?;
        if result.work_id.trim().is_empty() {
            return Err(Status::invalid_argument("work_id is required"));
        }
        match self.work.complete(result, unix_ms()) {
            Some(completed) => {
                println!(
                    "work {} finished on {} compute={}us transfer={}us total={}us checksum={:.6}",
                    completed.work_id,
                    completed.node_id,
                    completed.compute_time_us,
                    completed.transfer_time_us,
                    completed.total_latency_us,
                    completed.checksum,
                );
                Ok(Response::new(SubmitWorkResultResponse {
                    accepted: true,
                    message: "result recorded".to_string(),
                }))
            }
            None => Ok(Response::new(SubmitWorkResultResponse {
                accepted: false,
                message: "work was not dispatched by this controller".to_string(),
            })),
        }
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
        let service =
            ClusterControlService::new(Arc::new(Registry::new()), work.clone(), "test-cluster");
        (service, work)
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
}
