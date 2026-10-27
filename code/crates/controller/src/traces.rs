//! Histórico de unidades concluídas em disco (F05): os traces sobrevivem a um reinício do controller.
//!
//! Um arquivo só de acréscimo, com um registro `CompletedRecord` (prost, com prefixo de tamanho)
//! por unidade concluída. No início o controller lê o arquivo, fica com os últimos `limit`
//! registros e reescreve o arquivo limpo; durante a execução só acrescenta. Quando o arquivo
//! passa de `limit` registros ele é rodado para `<arquivo>.1` (um `rename`, sem reescrever nada
//! sob o lock da fila), então o disco guarda no máximo `2 × limit` registros.
//!
//! Uma cauda truncada por uma queda no meio de uma gravação é descartada na leitura, sem erro: o
//! que veio antes continua valendo.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ai_cluster_protocol::cluster::CompletedRecord;
use prost::Message;

use crate::work::CompletedWork;

impl From<&CompletedWork> for CompletedRecord {
    fn from(work: &CompletedWork) -> Self {
        Self {
            work_id: work.work_id.clone(),
            node_id: work.node_id.clone(),
            request_id: work.request_id.clone(),
            compute_time_us: work.compute_time_us,
            total_latency_us: work.total_latency_us,
            transfer_time_us: work.transfer_time_us,
            predicted_compute_us: work.predicted_compute_us,
            checksum: work.checksum,
            completed_at_unix_ms: work.completed_at_unix_ms,
        }
    }
}

impl From<CompletedRecord> for CompletedWork {
    fn from(record: CompletedRecord) -> Self {
        Self {
            work_id: record.work_id,
            node_id: record.node_id,
            request_id: record.request_id,
            compute_time_us: record.compute_time_us,
            total_latency_us: record.total_latency_us,
            transfer_time_us: record.transfer_time_us,
            predicted_compute_us: record.predicted_compute_us,
            checksum: record.checksum,
            completed_at_unix_ms: record.completed_at_unix_ms,
        }
    }
}

#[derive(Debug)]
struct State {
    file: Option<File>,
    /// Registros acrescentados ao arquivo atual desde que ele foi aberto ou rodado.
    appended: usize,
}

#[derive(Debug)]
pub struct TraceLog {
    path: PathBuf,
    limit: usize,
    state: Mutex<State>,
}

fn rotated(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

/// Os registros de um arquivo, em ordem, e se a leitura parou antes do fim (cauda truncada ou lixo).
fn read_records(path: &Path) -> (Vec<CompletedRecord>, bool) {
    let Ok(bytes) = std::fs::read(path) else {
        return (Vec::new(), false);
    };
    let mut slice = bytes.as_slice();
    let mut records = Vec::new();
    while !slice.is_empty() {
        match CompletedRecord::decode_length_delimited(&mut slice) {
            Ok(record) => records.push(record),
            Err(_) => return (records, true),
        }
    }
    (records, false)
}

impl TraceLog {
    /// Abre (e cria, se preciso) o arquivo e devolve o histórico que ele guardava, no máximo os
    /// últimos `limit` registros. Uma falha de disco não derruba o controller: o log fica
    /// desligado e o histórico em memória segue valendo.
    pub fn open(path: PathBuf, limit: usize) -> (Self, Vec<CompletedWork>) {
        let rotated_path = rotated(&path);
        let (mut records, torn_old) = read_records(&rotated_path);
        let (current, torn_new) = read_records(&path);
        let had_rotated = rotated_path.exists();
        records.extend(current);
        let dropped = records.len().saturating_sub(limit);
        records.drain(..dropped);
        let history: Vec<CompletedWork> = records.iter().cloned().map(Into::into).collect();

        // Reescreve limpo quando algo precisa ser arrumado: cauda truncada, excesso ou o arquivo
        // rodado que já foi incorporado. Do contrário só reabre para acrescentar.
        let needs_rewrite = torn_old || torn_new || dropped > 0 || had_rotated;
        let appended = if needs_rewrite {
            let temp = path.with_extension("tmp");
            let mut bytes = Vec::new();
            for record in &records {
                bytes.extend_from_slice(&record.encode_length_delimited_to_vec());
            }
            match std::fs::write(&temp, &bytes).and_then(|()| std::fs::rename(&temp, &path)) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&rotated_path);
                    records.len()
                }
                Err(err) => {
                    println!("compactar traces em {} falhou: {err}", path.display());
                    0
                }
            }
        } else {
            records.len()
        };
        let file = OpenOptions::new().create(true).append(true).open(&path);
        let file = match file {
            Ok(file) => Some(file),
            Err(err) => {
                println!(
                    "abrir traces em {} falhou: {err}; sem persistência",
                    path.display()
                );
                None
            }
        };
        if !history.is_empty() {
            println!(
                "traces carregados de {}: {} unidade(s) concluída(s)",
                path.display(),
                history.len()
            );
        }
        let log = Self {
            path,
            limit: limit.max(1),
            state: Mutex::new(State { file, appended }),
        };
        (log, history)
    }

    /// Acrescenta uma unidade concluída. Sem `fsync`: um registro perdido numa queda do host é só
    /// um trace a menos, e a cauda truncada é descartada na leitura.
    pub fn append(&self, work: &CompletedWork) {
        let bytes = CompletedRecord::from(work).encode_length_delimited_to_vec();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(file) = state.file.as_mut() else {
            return;
        };
        if let Err(err) = file.write_all(&bytes) {
            println!("gravar trace em {} falhou: {err}", self.path.display());
            return;
        }
        state.appended += 1;
        if state.appended >= self.limit {
            // Gira o arquivo: o atual vira `.1` (substituindo o anterior) e um novo começa vazio.
            state.file = None;
            let rolled = std::fs::rename(&self.path, rotated(&self.path));
            state.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
            state.appended = 0;
            if let Err(err) = rolled {
                println!("girar traces em {} falhou: {err}", self.path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(id: &str, at: u64) -> CompletedWork {
        CompletedWork {
            work_id: id.to_string(),
            node_id: "n".to_string(),
            request_id: "req".to_string(),
            compute_time_us: 10,
            total_latency_us: 30,
            transfer_time_us: 20,
            predicted_compute_us: 12,
            checksum: 1.5,
            completed_at_unix_ms: at,
        }
    }

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aic-traces-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("traces.log")
    }

    #[test]
    fn os_registros_voltam_iguais_depois_de_um_reinicio() {
        let path = temp_file("volta");
        let (log, history) = TraceLog::open(path.clone(), 100);
        assert!(history.is_empty());
        for n in 0..3 {
            log.append(&work(&format!("w{n}"), 1_000 + n));
        }
        drop(log);
        let (_, loaded) = TraceLog::open(path.clone(), 100);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[2], work("w2", 1_002));
    }

    #[test]
    fn uma_cauda_truncada_e_descartada_sem_perder_o_que_veio_antes() {
        let path = temp_file("cauda");
        let (log, _) = TraceLog::open(path.clone(), 100);
        log.append(&work("w0", 1));
        log.append(&work("w1", 2));
        drop(log);
        // uma queda no meio de uma gravação: bytes soltos no fim
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&[0x80, 0x01, 0x0a, 0xff]).unwrap();
        drop(file);
        let (log, loaded) = TraceLog::open(path.clone(), 100);
        assert_eq!(loaded.len(), 2);
        // e o arquivo foi arrumado: acrescentar agora não fica atrás do lixo
        log.append(&work("w2", 3));
        drop(log);
        let (_, again) = TraceLog::open(path.clone(), 100);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(again.len(), 3);
        assert_eq!(again[2].work_id, "w2");
    }

    #[test]
    fn so_os_ultimos_registros_do_limite_ficam_e_o_arquivo_gira() {
        let path = temp_file("gira");
        let (log, _) = TraceLog::open(path.clone(), 3);
        for n in 0..7 {
            log.append(&work(&format!("w{n}"), n));
        }
        // 7 registros com limite 3: girou duas vezes; o disco guarda no máximo 2 × limite
        assert!(rotated(&path).exists());
        drop(log);
        let (_, loaded) = TraceLog::open(path.clone(), 3);
        let ids: Vec<_> = loaded.iter().map(|w| w.work_id.as_str()).collect();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(ids, ["w4", "w5", "w6"], "ficam os mais novos, em ordem");
    }
}
