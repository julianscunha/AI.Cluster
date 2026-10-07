use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TensorDType {
    RawBytes,
    U8,
    I8,
    F16,
    BF16,
    F32,
    F64,
}

impl TensorDType {
    pub const fn element_size_bytes(self) -> usize {
        match self {
            Self::RawBytes | Self::U8 | Self::I8 => 1,
            Self::F16 | Self::BF16 => 2,
            Self::F32 => 4,
            Self::F64 => 8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BufferDescriptor {
    pub buffer_id: String,
    pub byte_len: u64,
    pub checksum: Option<String>,
    pub location: DataLocation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TensorDescriptor {
    pub tensor_id: String,
    pub dtype: TensorDType,
    pub shape: Vec<u64>,
    pub buffer: BufferDescriptor,
}

impl TensorDescriptor {
    pub fn element_count(&self) -> Option<u64> {
        self.shape
            .iter()
            .try_fold(1_u64, |total, dimension| total.checked_mul(*dimension))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DataLocation {
    Controller,
    Node { node_id: String },
    Storage { uri: String },
}

impl DataLocation {
    pub fn node_id(&self) -> Option<&str> {
        match self {
            Self::Node { node_id } => Some(node_id.as_str()),
            Self::Controller | Self::Storage { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferStrategy {
    ReuseLocal,
    ControllerRelay,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferPlan {
    pub buffer_id: String,
    pub source: DataLocation,
    pub destination: DataLocation,
    pub strategy: TransferStrategy,
    pub byte_len: u64,
    pub estimated_duration_us: u64,
}

impl TransferPlan {
    pub fn is_local(&self) -> bool {
        self.strategy == TransferStrategy::ReuseLocal
    }
}

pub fn plan_buffer_transfer(
    buffer: &BufferDescriptor,
    destination: DataLocation,
    source_rtt_us: f64,
    destination_rtt_us: f64,
    source_throughput_mb_per_sec: f64,
    destination_throughput_mb_per_sec: f64,
    local_copy_available: bool,
) -> TransferPlan {
    let strategy = if buffer.location == destination && local_copy_available {
        TransferStrategy::ReuseLocal
    } else {
        TransferStrategy::ControllerRelay
    };

    let estimated_duration_us = match strategy {
        TransferStrategy::ReuseLocal => 0,
        TransferStrategy::ControllerRelay => {
            let throughput = source_throughput_mb_per_sec
                .min(destination_throughput_mb_per_sec)
                .max(0.001);
            let transfer_us = (buffer.byte_len as f64 / throughput) as u64;
            (source_rtt_us.max(0.0) + destination_rtt_us.max(0.0)) as u64 + transfer_us
        }
    };

    TransferPlan {
        buffer_id: buffer.buffer_id.clone(),
        source: buffer.location.clone(),
        destination,
        strategy,
        byte_len: buffer.byte_len,
        estimated_duration_us,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        plan_buffer_transfer, BufferDescriptor, DataLocation, TensorDType, TensorDescriptor,
        TransferStrategy,
    };

    #[test]
    fn tensor_element_count_e_dimensionado() {
        let tensor = TensorDescriptor {
            tensor_id: "tensor-1".into(),
            dtype: TensorDType::F32,
            shape: vec![2, 3, 4],
            buffer: BufferDescriptor {
                buffer_id: "buffer-1".into(),
                byte_len: 96,
                checksum: None,
                location: DataLocation::Node {
                    node_id: "node-a".into(),
                },
            },
        };

        assert_eq!(tensor.element_count(), Some(24));
        assert_eq!(TensorDType::F32.element_size_bytes(), 4);
    }

    #[test]
    fn transfer_plan_identifica_reuso_local() {
        let buffer = BufferDescriptor {
            buffer_id: "buffer-1".into(),
            byte_len: 1024,
            checksum: None,
            location: DataLocation::Node {
                node_id: "node-a".into(),
            },
        };

        let plan = plan_buffer_transfer(
            &buffer,
            DataLocation::Node {
                node_id: "node-a".into(),
            },
            100.0,
            100.0,
            100.0,
            100.0,
            true,
        );

        assert_eq!(plan.strategy, TransferStrategy::ReuseLocal);
        assert_eq!(plan.byte_len, 1024);

        assert_eq!(plan.estimated_duration_us, 0);
        assert!(plan.is_local());

        let relay_plan = plan_buffer_transfer(
            &buffer,
            DataLocation::Node {
                node_id: "node-b".into(),
            },
            100.0,
            200.0,
            100.0,
            50.0,
            false,
        );

        assert_eq!(relay_plan.strategy, TransferStrategy::ControllerRelay);
        assert!(relay_plan.estimated_duration_us > 0);
    }
}
