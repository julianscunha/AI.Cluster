use std::sync::Arc;

use ai_cluster_protocol::cluster::cluster_control_server::ClusterControl;
use ai_cluster_protocol::cluster::{
    HealthStreamRequest, HealthStreamResponse, HeartbeatRequest, HeartbeatResponse, ListNodesRequest,
    ListNodesResponse, RegisterNodeRequest, RegisterNodeResponse,
};
use tonic::{Request, Response, Status};

use crate::registry::{unix_ms, Registry};

#[derive(Debug)]
pub struct ClusterControlService {
    registry: Arc<Registry>,
    cluster_id: String,
}

impl ClusterControlService {
    pub fn new(registry: Arc<Registry>, cluster_id: impl Into<String>) -> Self {
        Self {
            registry,
            cluster_id: cluster_id.into(),
        }
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
}
