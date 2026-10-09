use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};

use real_ai_cluster_common::tensor::sha256_checksum;

#[derive(Debug)]
struct StoredBuffer {
    data: Vec<u8>,
    checksum_sha256: String,
    last_access: u64,
    pins: usize,
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

    pub fn insert_with_pins(
        &mut self,
        buffer_id: String,
        data: Vec<u8>,
        pins: usize,
    ) -> Result<(), BufferStoreError> {
        let data_len = data.len() as u64;

        if let Some(existing) = self.buffers.get(&buffer_id) {
            if existing.pins > 0 {
                return Err(BufferStoreError::PinnedBufferReplacement(buffer_id));
            }
        }

        if data_len > self.max_bytes {
            return Err(BufferStoreError::CapacityTooSmall {
                buffer_id,
                buffer_bytes: data_len,
                capacity_bytes: self.max_bytes,
            });
        }

        let existing_len = self
            .buffers
            .get(&buffer_id)
            .map(|buffer| buffer.data.len() as u64)
            .unwrap_or_default();
        let mut projected_bytes = self
            .bytes_used
            .saturating_sub(existing_len)
            .saturating_add(data_len);
        let mut victims = Vec::new();

        if projected_bytes > self.max_bytes {
            let mut candidates: Vec<(String, u64, u64)> = self
                .buffers
                .iter()
                .filter(|(id, buffer)| **id != buffer_id && buffer.pins == 0)
                .map(|(id, buffer)| (id.clone(), buffer.data.len() as u64, buffer.last_access))
                .collect();
            candidates.sort_by_key(|(_, _, last_access)| *last_access);

            for (victim_id, victim_bytes, _) in candidates {
                if projected_bytes <= self.max_bytes {
                    break;
                }
                projected_bytes = projected_bytes.saturating_sub(victim_bytes);
                victims.push(victim_id);
            }

            if projected_bytes > self.max_bytes {
                return Err(BufferStoreError::CapacityExhausted {
                    buffer_id,
                    required_bytes: projected_bytes,
                    capacity_bytes: self.max_bytes,
                });
            }
        }

        for victim_id in victims {
            if let Some(victim) = self.buffers.remove(&victim_id) {
                self.bytes_used = self.bytes_used.saturating_sub(victim.data.len() as u64);
            }
        }

        if let Some(previous) = self.buffers.remove(&buffer_id) {
            self.bytes_used = self.bytes_used.saturating_sub(previous.data.len() as u64);
        }

        self.next_access = self.next_access.saturating_add(1);
        self.bytes_used = self.bytes_used.saturating_add(data_len);
        self.buffers.insert(
            buffer_id,
            StoredBuffer {
                checksum_sha256: sha256_checksum(&data),
                data,
                last_access: self.next_access,
                pins,
            },
        );

        Ok(())
    }

    pub fn get(&mut self, buffer_id: &str) -> Option<Vec<u8>> {
        self.get_with_checksum(buffer_id).map(|(data, _)| data)
    }

    pub fn get_with_checksum(&mut self, buffer_id: &str) -> Option<(Vec<u8>, String)> {
        self.next_access = self.next_access.saturating_add(1);
        let next_access = self.next_access;
        let buffer = self.buffers.get_mut(buffer_id)?;
        buffer.last_access = next_access;
        Some((buffer.data.clone(), buffer.checksum_sha256.clone()))
    }

    pub fn get_for_consumer(&mut self, buffer_id: &str) -> Option<Vec<u8>> {
        self.get_for_consumer_with_checksum(buffer_id)
            .map(|(data, _)| data)
    }

    pub fn get_for_consumer_with_checksum(&mut self, buffer_id: &str) -> Option<(Vec<u8>, String)> {
        let data = self.get_with_checksum(buffer_id)?;
        if let Some(buffer) = self.buffers.get_mut(buffer_id) {
            buffer.pins = buffer.pins.saturating_sub(1);
        }
        Some(data)
    }

    pub fn contains(&self, buffer_id: &str) -> bool {
        self.buffers.contains_key(buffer_id)
    }

    pub fn release_consumer(&mut self, buffer_id: &str) -> bool {
        let Some(buffer) = self.buffers.get_mut(buffer_id) else {
            return false;
        };
        buffer.pins = buffer.pins.saturating_sub(1);
        true
    }

    pub fn bytes_used(&self) -> u64 {
        self.bytes_used
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BufferStoreError {
    CapacityTooSmall {
        buffer_id: String,
        buffer_bytes: u64,
        capacity_bytes: u64,
    },
    CapacityExhausted {
        buffer_id: String,
        required_bytes: u64,
        capacity_bytes: u64,
    },
    PinnedBufferReplacement(String),
}

impl Display for BufferStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CapacityTooSmall {
                buffer_id,
                buffer_bytes,
                capacity_bytes,
            } => write!(
                formatter,
                "buffer excede a capacidade local: {buffer_id} possui {buffer_bytes} bytes, capacidade {capacity_bytes}"
            ),
            Self::CapacityExhausted {
                buffer_id,
                required_bytes,
                capacity_bytes,
            } => write!(
                formatter,
                "buffers pinados esgotaram a capacidade local: {buffer_id} requer {required_bytes} bytes, capacidade {capacity_bytes}"
            ),
            Self::PinnedBufferReplacement(buffer_id) => write!(
                formatter,
                "não é possível substituir um buffer com consumidores pendentes: {buffer_id}"
            ),
        }
    }
}

impl std::error::Error for BufferStoreError {}

#[cfg(test)]
mod tests {
    use super::{BufferStore, BufferStoreError};

    #[test]
    fn armazena_recupera_e_atualiza_lru() {
        let mut store = BufferStore::new(8);
        store
            .insert_with_pins("a".into(), vec![1, 2, 3, 4], 0)
            .unwrap();
        store
            .insert_with_pins("b".into(), vec![5, 6, 7, 8], 0)
            .unwrap();

        assert_eq!(store.bytes_used(), 8);
        assert_eq!(store.get("a"), Some(vec![1, 2, 3, 4]));

        store
            .insert_with_pins("c".into(), vec![9, 10, 11, 12], 0)
            .unwrap();

        assert!(store.contains("a"));
        assert!(!store.contains("b"));
        assert!(store.contains("c"));
        assert_eq!(store.bytes_used(), 8);
    }

    #[test]
    fn buffer_pinado_nao_pode_ser_evicted() {
        let mut store = BufferStore::new(8);
        store
            .insert_with_pins("a".into(), vec![1, 2, 3, 4], 1)
            .unwrap();

        let error = store
            .insert_with_pins("b".into(), vec![5, 6, 7, 8, 9, 10, 11, 12], 0)
            .unwrap_err();

        assert!(matches!(error, BufferStoreError::CapacityExhausted { .. }));
        assert!(store.contains("a"));
    }

    #[test]
    fn consumidor_libera_pin_para_eviction() {
        let mut store = BufferStore::new(8);
        store
            .insert_with_pins("a".into(), vec![1, 2, 3, 4], 1)
            .unwrap();

        assert_eq!(store.get_for_consumer("a"), Some(vec![1, 2, 3, 4]));
        store
            .insert_with_pins("b".into(), vec![5, 6, 7, 8, 9, 10, 11, 12], 0)
            .unwrap();

        assert!(!store.contains("a"));
        assert!(store.contains("b"));
    }

    #[test]
    fn release_pin_reduz_a_contagem_do_buffer() {
        let mut store = BufferStore::new(8);
        store
            .insert_with_pins("a".into(), vec![1, 2, 3, 4], 2)
            .unwrap();

        assert!(store.release_consumer("a"));
        assert!(store.release_consumer("a"));
        assert!(store.release_consumer("a"));
        assert!(!store.release_consumer("missing"));
    }

    #[test]
    fn rejeita_buffer_maior_que_capacidade() {
        let mut store = BufferStore::new(2);
        assert!(matches!(
            store.insert_with_pins("a".into(), vec![1, 2, 3], 0),
            Err(BufferStoreError::CapacityTooSmall { .. })
        ));
        assert!(!store.contains("a"));
    }

    #[test]
    fn rejeita_substituir_buffer_pinado() {
        let mut store = BufferStore::new(8);
        store
            .insert_with_pins("a".into(), vec![1, 2, 3, 4], 1)
            .unwrap();

        assert!(matches!(
            store.insert_with_pins("a".into(), vec![9, 9, 9, 9], 0),
            Err(BufferStoreError::PinnedBufferReplacement(_))
        ));
        assert_eq!(store.get("a"), Some(vec![1, 2, 3, 4]));
    }
}
