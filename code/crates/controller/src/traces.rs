//! Histórico de unidades concluídas em disco (F05): os traces sobrevivem a um reinício do controller.
//!
//! Um arquivo só de acréscimo, com um registro `CompletedRecord` (prost, com prefixo de tamanho)
//! por unidade concluída. No início o controller lê o arquivo, fica com os últimos `limit`
//! registros e reescreve o arquivo limpo; durante a execução só acrescenta. Quando o arquivo
//! passa de `limit` registros ele é rodado para `<arquivo>.1` (um `rename`, sem reescrever nada
//! sob o lock da fila), então o disco guarda no máximo `2 × limit` registros.
//!
//! A leitura para no primeiro registro ilegível (uma cauda truncada por queda, bytes soltos, ou um
//! registro vazio que um arquivo estendido com zeros produz): o que veio antes continua valendo e
//! o que vem depois é descartado. Um `work_id` repetido é ignorado, o que protege contra um
//! arquivo rodado (`.1`) que não conseguiu ser removido e seria lido duas vezes.

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
    /// A rotação não conseguiu reabrir o arquivo: tentar de novo no próximo registro.
    reopen: bool,
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
            // Um registro sem `work_id` é o tamanho zero de um arquivo preenchido com zeros, não uma
            // unidade: tratado como corrupção, para não povoar o histórico de fantasmas.
            Ok(record) if !record.work_id.is_empty() => records.push(record),
            _ => return (records, true),
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
        // O mesmo trabalho não conta duas vezes (ver o doc do módulo).
        let mut seen = std::collections::HashSet::new();
        records.retain(|record| seen.insert(record.work_id.clone()));
        let dropped = records.len().saturating_sub(limit);
        records.drain(..dropped);
        let history: Vec<CompletedWork> = records.iter().cloned().map(Into::into).collect();

        // Reescreve limpo quando algo precisa ser arrumado: cauda truncada, excesso ou o arquivo
        // rodado que já foi incorporado. Do contrário só reabre para acrescentar.
        let needs_rewrite = torn_old || torn_new || dropped > 0 || had_rotated;
        // Reescrita com `fsync` antes do `rename`: uma queda de energia deixa o arquivo antigo ou o
        // novo inteiro, nunca um vazio no lugar do bom.
        let rewrite = |records: &[CompletedRecord]| -> std::io::Result<()> {
            let temp = path.with_extension("tmp");
            let mut bytes = Vec::new();
            for record in records {
                bytes.extend_from_slice(&record.encode_length_delimited_to_vec());
            }
            let mut file = File::create(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temp, &path)
        };
        // Se a reescrita necessária falha, o arquivo continua com o lixo na cauda e acrescentar
        // atrás dele perderia os registros novos: nesse caso o log fica desligado, em vez de mentir.
        let mut usable = true;
        let appended = if needs_rewrite {
            match rewrite(&records) {
                Ok(()) => {
                    if let Err(err) = std::fs::remove_file(&rotated_path)
                        && rotated_path.exists()
                    {
                        println!("remover {} falhou: {err}", rotated_path.display());
                    }
                    records.len()
                }
                Err(err) => {
                    println!(
                        "compactar traces em {} falhou: {err}; sem persistência nesta execução",
                        path.display()
                    );
                    usable = false;
                    0
                }
            }
        } else {
            records.len()
        };
        let file = if usable {
            match OpenOptions::new().create(true).append(true).open(&path) {
                Ok(file) => Some(file),
                Err(err) => {
                    println!(
                        "abrir traces em {} falhou: {err}; sem persistência",
                        path.display()
                    );
                    None
                }
            }
        } else {
            None
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
            state: Mutex::new(State {
                file,
                appended,
                reopen: false,
            }),
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
        // Depois de uma rotação que não conseguiu reabrir o arquivo, tenta de novo a cada registro
        // (uma vez só por falha, no log) em vez de ficar mudo até o reinício.
        if state.file.is_none() && state.reopen {
            state.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
            if state.file.is_some() {
                state.reopen = false;
            }
        }
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
            state.reopen = state.file.is_none();
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
    fn um_arquivo_estendido_com_zeros_nao_vira_unidades_fantasmas() {
        let path = temp_file("zeros");
        let (log, _) = TraceLog::open(path.clone(), 100);
        log.append(&work("w0", 5));
        drop(log);
        // o sistema de arquivos estendeu o arquivo com zeros depois de uma queda
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&[0u8; 64]).unwrap();
        drop(file);
        let (_, loaded) = TraceLog::open(path.clone(), 100);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(loaded.len(), 1, "só a unidade verdadeira");
        assert_eq!(loaded[0].work_id, "w0");
    }

    #[test]
    fn o_arquivo_rodado_que_nao_saiu_nao_duplica_o_historico() {
        let path = temp_file("duplica");
        let (log, _) = TraceLog::open(path.clone(), 100);
        log.append(&work("a", 1));
        log.append(&work("b", 2));
        drop(log);
        // simula um `.1` que ficou para trás com os mesmos registros do arquivo atual
        std::fs::copy(&path, rotated(&path)).unwrap();
        let (_, loaded) = TraceLog::open(path.clone(), 100);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let ids: Vec<_> = loaded.iter().map(|w| w.work_id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
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
