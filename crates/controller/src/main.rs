use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};

use real_ai_cluster_protocol::v1::{
    cluster_control_server::{ClusterControl, ClusterControlServer},
    *,
};
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

#[derive(Clone, Default)]
struct Controller {
    nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
    offline_after: Duration,
}

impl Controller {
    fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            offline_after: Duration::from_secs(10),
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
                    warn!(node_id = %node.node_id, "node marked offline");
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
        let mut node = request.into_inner().node.ok_or_else(|| {
            Status::invalid_argument("node é obrigatório")
        })?;

        node.last_heartbeat_unix_ms = now_ms();
        node.state = NodeState::Ready as i32;

        let mut nodes = self.nodes.write().await;
        nodes.insert(node.node_id.clone(), node.clone());

        info!(node_id = %node.node_id, hostname = %node.hostname, "node registrado");

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

        let node = nodes.get_mut(&req.node_id).ok_or_else(|| {
            Status::not_found("node não registrado")
        })?;

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

        let node = nodes.get_mut(&req.node_id).ok_or_else(|| {
            Status::not_found("node não registrado")
        })?;

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

    Server::builder()
        .add_service(ClusterControlServer::new(controller))
        .serve(addr)
        .await?;

    Ok(())
}
