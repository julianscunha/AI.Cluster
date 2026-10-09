use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};

use real_ai_cluster_common::tensor::sha256_checksum;

const MAX_BUFFER_CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
struct StoredBuffer {
    data: Vec<u8>,
    checksum_sha256: String,
    last_access: u64,
    pins: usize,
}

#[derive(Debug)]
struct StagedTransfer {
    buffer_id: String,
    total_bytes: u64,
    checksum_sha256: String,
    pins: usize,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeginTransferResult {
    pub next_offset: u64,
    pub already_complete: bool,
}

#[derive(Debug, Default)]
pub struct BufferStore {
    max_bytes: u64,
    bytes_used: u64,
    reserved_transfer_bytes: u64,
    next_access: u64,
    buffers: HashMap<String, StoredBuffer>,
    transfers: HashMap<String, StagedTransfer>,
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
            .saturating_add(data_len)
            .saturating_add(self.reserved_transfer_bytes);
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

    #[cfg(test)]
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

    pub fn begin_transfer(
        &mut self,
        transfer_id: String,
        buffer_id: String,
        total_bytes: u64,
        checksum_sha256: String,
        pins: usize,
    ) -> Result<BeginTransferResult, BufferStoreError> {
        if transfer_id.is_empty() || buffer_id.is_empty() || checksum_sha256.is_empty() {
            return Err(BufferStoreError::InvalidTransferMetadata(
                "transfer_id, buffer_id e checksum são obrigatórios".into(),
            ));
        }

        if let Some(transfer) = self.transfers.get(&transfer_id) {
            if transfer.buffer_id != buffer_id
                || transfer.total_bytes != total_bytes
                || transfer.checksum_sha256 != checksum_sha256
                || transfer.pins != pins
            {
                return Err(BufferStoreError::TransferConflict(transfer_id));
            }

            return Ok(BeginTransferResult {
                next_offset: transfer.data.len() as u64,
                already_complete: false,
            });
        }

        if total_bytes > self.max_bytes {
            return Err(BufferStoreError::CapacityTooSmall {
                buffer_id,
                buffer_bytes: total_bytes,
                capacity_bytes: self.max_bytes,
            });
        }

        if let Some(existing) = self.buffers.get(&buffer_id) {
            if existing.data.len() as u64 == total_bytes
                && existing.checksum_sha256 == checksum_sha256
            {
                return Ok(BeginTransferResult {
                    next_offset: total_bytes,
                    already_complete: true,
                });
            }
            if existing.pins > 0 {
                return Err(BufferStoreError::PinnedBufferReplacement(buffer_id));
            }
        }

        let existing_len = self
            .buffers
            .get(&buffer_id)
            .map(|buffer| buffer.data.len() as u64)
            .unwrap_or_default();
        let mut projected_bytes = self
            .bytes_used
            .saturating_sub(existing_len)
            .saturating_add(self.reserved_transfer_bytes)
            .saturating_add(total_bytes);
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

        self.reserved_transfer_bytes = self.reserved_transfer_bytes.saturating_add(total_bytes);
        self.transfers.insert(
            transfer_id,
            StagedTransfer {
                buffer_id,
                total_bytes,
                checksum_sha256,
                pins,
                data: Vec::new(),
            },
        );

        Ok(BeginTransferResult {
            next_offset: 0,
            already_complete: false,
        })
    }

    pub fn append_transfer_chunk(
        &mut self,
        transfer_id: &str,
        offset: u64,
        data: &[u8],
        chunk_checksum_sha256: &str,
    ) -> Result<u64, BufferStoreError> {
        if data.is_empty() || data.len() > MAX_BUFFER_CHUNK_BYTES {
            return Err(BufferStoreError::InvalidChunkSize {
                chunk_bytes: data.len(),
                max_bytes: MAX_BUFFER_CHUNK_BYTES,
            });
        }
        if sha256_checksum(data) != chunk_checksum_sha256 {
            return Err(BufferStoreError::TransferChunkChecksumMismatch(
                transfer_id.to_owned(),
            ));
        }

        let transfer = self
            .transfers
            .get_mut(transfer_id)
            .ok_or_else(|| BufferStoreError::TransferNotFound(transfer_id.to_owned()))?;
        let received = transfer.data.len() as u64;
        let chunk_end = offset
            .checked_add(data.len() as u64)
            .ok_or_else(|| BufferStoreError::TransferExceedsDeclaredLength(transfer_id.to_owned()))?;

        if offset < received {
            if chunk_end <= received
                && transfer.data[offset as usize..chunk_end as usize] == *data
            {
                return Ok(received);
            }
            return Err(BufferStoreError::ConflictingChunk(transfer_id.to_owned()));
        }
        if offset > received {
            return Err(BufferStoreError::TransferOffsetMismatch {
                transfer_id: transfer_id.to_owned(),
                expected_offset: received,
            });
        }
        if chunk_end > transfer.total_bytes {
            return Err(BufferStoreError::TransferExceedsDeclaredLength(
                transfer_id.to_owned(),
            ));
        }

        transfer.data.extend_from_slice(data);
        Ok(transfer.data.len() as u64)
    }

    pub fn commit_transfer(
        &mut self,
        transfer_id: &str,
    ) -> Result<(u64, String), BufferStoreError> {
        let (received, total, expected_checksum) = {
            let transfer = self
                .transfers
                .get(transfer_id)
                .ok_or_else(|| BufferStoreError::TransferNotFound(transfer_id.to_owned()))?;
            (
                transfer.data.len() as u64,
                transfer.total_bytes,
                transfer.checksum_sha256.clone(),
            )
        };

        if received != total {
            return Err(BufferStoreError::TransferIncomplete {
                transfer_id: transfer_id.to_owned(),
                expected_bytes: total,
                received_bytes: received,
            });
        }

        let actual_checksum = {
            let transfer = self.transfers.get(transfer_id).expect("validated transfer");
            sha256_checksum(&transfer.data)
        };
        if actual_checksum != expected_checksum {
            self.abort_transfer(transfer_id);
            return Err(BufferStoreError::TransferChecksumMismatch(
                transfer_id.to_owned(),
            ));
        }

        let transfer = self
            .transfers
            .remove(transfer_id)
            .expect("validated transfer");
        self.reserved_transfer_bytes = self
            .reserved_transfer_bytes
            .saturating_sub(transfer.total_bytes);
        let total_bytes = transfer.total_bytes;

        self.insert_with_pins(transfer.buffer_id, transfer.data, transfer.pins)?;
        Ok((total_bytes, actual_checksum))
    }

    pub fn abort_transfer(&mut self, transfer_id: &str) -> bool {
        let Some(transfer) = self.transfers.remove(transfer_id) else {
            return false;
        };
        self.reserved_transfer_bytes = self
            .reserved_transfer_bytes
            .saturating_sub(transfer.total_bytes);
        true
    }

    pub fn bytes_used(&self) -> u64 {
        self.bytes_used
    }

    pub fn reserved_transfer_bytes(&self) -> u64 {
        self.reserved_transfer_bytes
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
    InvalidTransferMetadata(String),
    TransferConflict(String),
    TransferNotFound(String),
    TransferOffsetMismatch {
        transfer_id: String,
        expected_offset: u64,
    },
    InvalidChunkSize {
        chunk_bytes: usize,
        max_bytes: usize,
    },
    TransferChunkChecksumMismatch(String),
    ConflictingChunk(String),
    TransferExceedsDeclaredLength(String),
    TransferIncomplete {
        transfer_id: String,
        expected_bytes: u64,
        received_bytes: u64,
    },
    TransferChecksumMismatch(String),
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
            Self::InvalidTransferMetadata(message) => write!(formatter, "metadados de transferência inválidos: {message}"),
            Self::TransferConflict(transfer_id) => write!(formatter, "transfer_id reutilizado com metadados diferentes: {transfer_id}"),
            Self::TransferNotFound(transfer_id) => write!(formatter, "transferência não encontrada: {transfer_id}"),
            Self::TransferOffsetMismatch { transfer_id, expected_offset } => write!(formatter, "offset inválido para {transfer_id}; esperado {expected_offset}"),
            Self::InvalidChunkSize { chunk_bytes, max_bytes } => write!(formatter, "chunk possui {chunk_bytes} bytes; limite é {max_bytes}"),
            Self::TransferChunkChecksumMismatch(transfer_id) => write!(formatter, "checksum de chunk inválido: {transfer_id}"),
            Self::ConflictingChunk(transfer_id) => write!(formatter, "reenvio conflitante para transferência {transfer_id}"),
            Self::TransferExceedsDeclaredLength(transfer_id) => write!(formatter, "chunk excede tamanho declarado: {transfer_id}"),
            Self::TransferIncomplete { transfer_id, expected_bytes, received_bytes } => write!(formatter, "transferência {transfer_id} incompleta: recebidos {received_bytes} de {expected_bytes} bytes"),
            Self::TransferChecksumMismatch(transfer_id) => write!(formatter, "checksum final incompatível: {transfer_id}"),
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
    fn transferencia_em_chunks_e_idempotente_ao_repetir_chunk() {
        let mut store = BufferStore::new(16);
        let data = b"abcd";
        let checksum = sha256_checksum(data);
        let begin = store
            .begin_transfer("tx-1".into(), "buffer-1".into(), 4, checksum.clone(), 1)
            .unwrap();
        assert_eq!(begin.next_offset, 0);
        assert!(!begin.already_complete);

        let first = b"ab";
        let first_checksum = sha256_checksum(first);
        assert_eq!(
            store
                .append_transfer_chunk("tx-1", 0, first, &first_checksum)
                .unwrap(),
            2
        );
        assert_eq!(
            store
                .append_transfer_chunk("tx-1", 0, first, &first_checksum)
                .unwrap(),
            2
        );
        assert!(matches!(
            store.append_transfer_chunk("tx-1", 4, b"d", &sha256_checksum(b"d")),
            Err(BufferStoreError::TransferOffsetMismatch { expected_offset: 2, .. })
        ));

        let last = b"cd";
        assert_eq!(
            store
                .append_transfer_chunk("tx-1", 2, last, &sha256_checksum(last))
                .unwrap(),
            4
        );
        assert_eq!(store.commit_transfer("tx-1").unwrap(), (4, checksum));
        assert_eq!(store.get("buffer-1"), Some(data.to_vec()));
        assert_eq!(store.reserved_transfer_bytes(), 0);
    }

    #[test]
    fn transfere_reservas_de_capacidade_ate_commit_ou_abort() {
        let mut store = BufferStore::new(8);
        store
            .begin_transfer(
                "tx-1".into(),
                "buffer-1".into(),
                6,
                sha256_checksum(b"123456"),
                1,
            )
            .unwrap();
        assert_eq!(store.reserved_transfer_bytes(), 6);
        assert!(matches!(
            store.insert_with_pins("other".into(), vec![1, 2, 3], 0),
            Err(BufferStoreError::CapacityExhausted { .. })
        ));

        assert!(store.abort_transfer("tx-1"));
        assert_eq!(store.reserved_transfer_bytes(), 0);
        assert!(!store.abort_transfer("tx-1"));
    }

    #[test]
    fn rejeita_checksum_final_incorreto_e_libera_reserva() {
        let mut store = BufferStore::new(8);
        store
            .begin_transfer("tx-1".into(), "buffer-1".into(), 3, sha256_checksum(b"xyz"), 1)
            .unwrap();
        store
            .append_transfer_chunk("tx-1", 0, b"abc", &sha256_checksum(b"abc"))
            .unwrap();

        assert!(matches!(
            store.commit_transfer("tx-1"),
            Err(BufferStoreError::TransferChecksumMismatch(_))
        ));
        assert_eq!(store.reserved_transfer_bytes(), 0);
        assert!(!store.contains("buffer-1"));
    }

    #[test]
    fn nova_chamada_begin_retorna_offset_para_resume() {
        let mut store = BufferStore::new(16);
        let checksum = sha256_checksum(b"abcd");
        store
            .begin_transfer("tx-1".into(), "buffer-1".into(), 4, checksum.clone(), 1)
            .unwrap();
        store
            .append_transfer_chunk("tx-1", 0, b"ab", &sha256_checksum(b"ab"))
            .unwrap();

        let resume = store
            .begin_transfer("tx-1".into(), "buffer-1".into(), 4, checksum, 1)
            .unwrap();
        assert_eq!(resume.next_offset, 2);
        assert!(!resume.already_complete);
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
