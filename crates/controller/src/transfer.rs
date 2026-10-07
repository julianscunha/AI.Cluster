use real_ai_cluster_common::tensor::{
    plan_buffer_transfer, BufferDescriptor, DataLocation, TransferPlan,
};
use real_ai_cluster_protocol::v1::NodeInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferPlanError {
    SourceNodeNotFound(String),
    DestinationNodeNotFound(String),
}

impl std::fmt::Display for TransferPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceNodeNotFound(node_id) => {
                write!(formatter, "node de origem não encontrado: {node_id}")
            }
            Self::DestinationNodeNotFound(node_id) => {
                write!(formatter, "node de destino não encontrado: {node_id}")
            }
        }
    }
}

impl std::error::Error for TransferPlanError {}

pub fn plan_output_transfer(
    buffer: &BufferDescriptor,
    destination_node_id: &str,
    nodes: &[NodeInfo],
) -> Result<TransferPlan, TransferPlanError> {
    let source_node_id = buffer
        .location
        .node_id()
        .ok_or_else(|| TransferPlanError::SourceNodeNotFound("localização não é um node".into()))?;

    let source = nodes
        .iter()
        .find(|node| node.node_id == source_node_id)
        .ok_or_else(|| TransferPlanError::SourceNodeNotFound(source_node_id.to_owned()))?;

    let destination = nodes
        .iter()
        .find(|node| node.node_id == destination_node_id)
        .ok_or_else(|| {
            TransferPlanError::DestinationNodeNotFound(destination_node_id.to_owned())
        })?;

    let source_capability = source.capability.as_ref();
    let destination_capability = destination.capability.as_ref();

    Ok(plan_buffer_transfer(
        buffer,
        DataLocation::Node {
            node_id: destination.node_id.clone(),
        },
        source_capability
            .map(|capability| capability.network_rtt_us)
            .unwrap_or_default(),
        destination_capability
            .map(|capability| capability.network_rtt_us)
            .unwrap_or_default(),
        source_capability
            .map(|capability| capability.network_mb_per_sec)
            .unwrap_or_default(),
        destination_capability
            .map(|capability| capability.network_mb_per_sec)
            .unwrap_or_default(),
        false,
    ))
}

#[cfg(test)]
mod tests {
    use real_ai_cluster_common::tensor::{BufferDescriptor, DataLocation, TransferStrategy};
    use real_ai_cluster_protocol::v1::{Capability, NodeInfo, NodeState};

    use super::plan_output_transfer;

    fn node(node_id: &str, rtt: f64, throughput: f64) -> NodeInfo {
        NodeInfo {
            node_id: node_id.into(),
            state: NodeState::Ready as i32,
            worker_endpoint: format!("http://{node_id}:50052"),
            capability: Some(Capability {
                network_rtt_us: rtt,
                network_mb_per_sec: throughput,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn planeja_relay_entre_nodes() {
        let buffer = BufferDescriptor {
            buffer_id: "buffer-1".into(),
            byte_len: 1_000_000,
            checksum: None,
            location: DataLocation::Node {
                node_id: "node-a".into(),
            },
        };

        let plan = plan_output_transfer(
            &buffer,
            "node-b",
            &[node("node-a", 100.0, 100.0), node("node-b", 200.0, 50.0)],
        )
        .unwrap();

        assert_eq!(plan.strategy, TransferStrategy::ControllerRelay);
        assert!(plan.estimated_duration_us > 0);
        assert_eq!(plan.destination.node_id(), Some("node-b"));
    }

    #[test]
    fn rejeita_origem_desconhecida() {
        let buffer = BufferDescriptor {
            buffer_id: "buffer-1".into(),
            byte_len: 10,
            checksum: None,
            location: DataLocation::Node {
                node_id: "missing".into(),
            },
        };

        assert!(plan_output_transfer(&buffer, "node-b", &[node("node-b", 10.0, 100.0)]).is_err());
    }
}
