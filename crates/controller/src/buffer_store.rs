use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};

use real_ai_cluster_common::tensor::BufferDescriptor;

#[derive(Debug, Clone)]
pub struct StoredBuffer {
    pub descriptor: BufferDescriptor,
    pub bytes: Vec<u8>,
    last_access: u64,
}

#[derive(Debug, Default)]
pub struct BufferStore {
    max_bytes: u64,
    bytes_used: u64,
    next_access: u64,
    buffers: HashMap<String, StoredBuffer>,
}

impl BufferStore {
    pub fn new(max_bytes: u64) -> Self {
        Self {
            max_bytes,
            ..Default::default()
        }
    }

    pub fn insert(
        &mut self,
        descriptor: BufferDescriptor,
        bytes: Vec<u8>,
    ) -> Result<(), BufferStoreError> {
        let actual_len = bytes.len() as u64;

        if descriptor.byte_len != actual_len {
            return Err(BufferStoreError::LengthMismatch {
                buffer_id: descriptor.buffer_id.clone(),
                declared: descriptor.byte_len,
                actual: actual_len,
            });
        }

        if actual_len > self.max_bytes {
            return Err(BufferStoreError::CapacityTooSmall {
                buffer_id: descriptor.buffer_id,
                buffer_bytes: actual_len,
                capacity_bytes: self.max_bytes,
            });
        }

        if let Some(previous) = self.buffers.remove(&descriptor.buffer_id) {
            self.bytes_used = self.bytes_used.saturating_sub(previous.bytes.len() as u64);
        }

        self.next_access = self.next_access.saturating_add(1);
        self.bytes_used = self.bytes_used.saturating_add(actual_len);
        self.buffers.insert(
            descriptor.buffer_id.clone(),
            StoredBuffer {
                descriptor,
                bytes,
                last_access: self.next_access,
            },
        );

        self.evict_until_within_capacity();
        Ok(())
    }

    pub fn get(&mut self, buffer_id: &str) -> Option<StoredBuffer> {
        self.next_access = self.next_access.saturating_add(1);
        let next_access = self.next_access;
        let buffer = self.buffers.get_mut(buffer_id)?;
        buffer.last_access = next_access;
        Some(buffer.clone())
    }

    pub fn bytes_used(&self) -> u64 {
        self.bytes_used
    }

    fn evict_until_within_capacity(&mut self) {
        while self.bytes_used > self.max_bytes {
            let victim_id = self
                .buffers
                .iter()
                .min_by_key(|(_, buffer)| buffer.last_access)
                .map(|(buffer_id, _)| buffer_id.clone());

            let Some(victim_id) = victim_id else {
                break;
            };

            if let Some(victim) = self.buffers.remove(&victim_id) {
                self.bytes_used = self.bytes_used.saturating_sub(victim.bytes.len() as u64);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BufferStoreError {
    LengthMismatch {
        buffer_id: String,
        declared: u64,
        actual: u64,
    },
    CapacityTooSmall {
        buffer_id: String,
        buffer_bytes: u64,
        capacity_bytes: u64,
    },
}

impl Display for BufferStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LengthMismatch {
                buffer_id,
                declared,
                actual,
            } => write!(
                formatter,
                "tamanho do buffer incompatível: {buffer_id} declara {declared} bytes, recebeu {actual}"
            ),
            Self::CapacityTooSmall {
                buffer_id,
                buffer_bytes,
                capacity_bytes,
            } => write!(
                formatter,
                "buffer excede a capacidade do cache: {buffer_id} possui {buffer_bytes} bytes, capacidade {capacity_bytes}"
            ),
        }
    }
}

impl std::error::Error for BufferStoreError {}

#[cfg(test)]
mod tests {
    use real_ai_cluster_common::tensor::{BufferDescriptor, DataLocation};

    use super::BufferStore;

    fn descriptor(id: &str, byte_len: u64) -> BufferDescriptor {
        BufferDescriptor {
            buffer_id: id.into(),
            byte_len,
            checksum: None,
            location: DataLocation::Controller,
        }
    }

    #[test]
    fn armazena_recupera_e_atualiza_lru() {
        let mut store = BufferStore::new(10);
        store.insert(descriptor("a", 4), vec![1, 2, 3, 4]).unwrap();
        store.insert(descriptor("b", 4), vec![5, 6, 7, 8]).unwrap();

        assert_eq!(store.bytes_used(), 8);
        assert_eq!(store.get("a").unwrap().bytes, vec![1, 2, 3, 4]);

        store
            .insert(descriptor("c", 4), vec![9, 10, 11, 12])
            .unwrap();

        assert!(store.get("a").is_some());
        assert!(store.get("b").is_none());
        assert!(store.get("c").is_some());
        assert_eq!(store.bytes_used(), 8);
    }

    #[test]
    fn rejeita_tamanho_inconsistente() {
        let mut store = BufferStore::new(10);
        assert!(store.insert(descriptor("a", 4), vec![1, 2, 3]).is_err());
        assert!(store.get("a").is_none());
    }

    #[test]
    fn rejeita_buffer_maior_que_cache() {
        let mut store = BufferStore::new(2);
        assert!(store.insert(descriptor("a", 4), vec![1, 2, 3, 4]).is_err());
        assert!(store.get("a").is_none());
    }
}
