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
    Grpc,
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

#[cfg(test)]
mod tests {
    use super::{
        BufferDescriptor, DataLocation, TensorDescriptor, TensorDType, TransferPlan,
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
        let plan = TransferPlan {
            buffer_id: "buffer-1".into(),
            source: DataLocation::Node {
                node_id: "node-a".into(),
            },
            destination: DataLocation::Node {
                node_id: "node-a".into(),
            },
            strategy: TransferStrategy::ReuseLocal,
            byte_len: 1024,
            estimated_duration_us: 0,
        };

        assert!(plan.is_local());
    }
}
