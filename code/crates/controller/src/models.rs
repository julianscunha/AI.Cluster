//! Orquestração do modelo do llama.cpp (ADR-022, E5 e E6): o que o operador pediu, o plano de workers
//! que o gateway aplica e as chaves de API. O controller guarda só o **hash** das chaves; a chave em
//! claro aparece uma vez, na criação.

use std::path::PathBuf;
use std::sync::Mutex;

use ai_cluster_protocol::cluster::{
    ApiKey, DesiredModel, ModelState, ModelStatus, RpcEndpoint, TenantGrant,
};
use sha2::{Digest, Sha256};

/// Quanto tempo a lista de workers precisa ficar igual antes de o plano mudar: um worker que acabou
/// de entrar ainda está se medindo, e um que oscila não deve reiniciar o modelo a cada batida.
pub const STABLE_MS: u64 = 30_000;
/// O tenant criado com o primeiro `models start`: todos os modelos.
pub const DEFAULT_TENANT: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Wanted {
    model_id: String,
    /// Relativo à pasta de modelos.
    model_path: String,
    context: u64,
    /// RAM que o modelo pede dos workers (do veredito do `start`); 0 = modo só-gateway, sem conferir.
    required_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredTenant {
    name: String,
    sha256: [u8; 32],
    prefix: String,
}

#[derive(Default, Debug)]
struct Inner {
    wanted: Option<Wanted>,
    generation: u64,
    applied: Vec<RpcEndpoint>,
    /// A lista nova que está esperando a janela de estabilidade, e desde quando.
    pending: Option<(Vec<RpcEndpoint>, u64)>,
    /// Os workers de agora não comportam o modelo: o plano para até voltar RAM. O texto é o motivo.
    insufficient: Option<String>,
    tenants: Vec<StoredTenant>,
    running: Option<ModelStatus>,
}

#[derive(Debug)]
pub struct Orchestrator {
    inner: Mutex<Inner>,
    path: Option<PathBuf>,
    stable_ms: u64,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// `sha256` de uma chave de API; o gateway usa a mesma conta para conferir a que o cliente manda.
pub fn key_hash(key: &str) -> [u8; 32] {
    Sha256::digest(key.as_bytes()).into()
}

/// 32 bytes aleatórios em base64url sem preenchimento, com o prefixo `aic_`.
fn new_key() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("sem fonte de entropia para a chave de API");
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::from("aic_");
    for chunk in bytes.chunks(3) {
        let n =
            chunk.iter().fold(0u32, |acc, b| (acc << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

impl Orchestrator {
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut inner = Inner::default();
        if let Some(path) = &path
            && let Ok(text) = std::fs::read_to_string(path)
        {
            for line in text.lines() {
                let fields: Vec<&str> = line.split('\t').collect();
                match fields.as_slice() {
                    ["model", id, model_path, context, generation, rest @ ..] => {
                        if let (Ok(context), Ok(generation)) = (context.parse(), generation.parse())
                        {
                            inner.wanted = Some(Wanted {
                                model_id: id.to_string(),
                                model_path: model_path.to_string(),
                                context,
                                required_bytes: rest
                                    .first()
                                    .and_then(|v| v.parse().ok())
                                    .unwrap_or(0),
                            });
                            inner.generation = generation;
                        }
                    }
                    ["worker", node_id, address] => inner.applied.push(RpcEndpoint {
                        node_id: node_id.to_string(),
                        address: address.to_string(),
                    }),
                    ["generation", generation] => {
                        inner.generation = generation.parse().unwrap_or(inner.generation);
                    }
                    ["tenant", name, sha, prefix] => {
                        if let Some(sha256) = unhex(sha) {
                            inner.tenants.push(StoredTenant {
                                name: name.to_string(),
                                sha256,
                                prefix: prefix.to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        let stable_ms = std::env::var("AI_CLUSTER_PLAN_STABLE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(STABLE_MS);
        Self {
            inner: Mutex::new(inner),
            path,
            stable_ms,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn save(&self, inner: &Inner) {
        let Some(path) = &self.path else { return };
        let mut text = String::new();
        match &inner.wanted {
            Some(w) => text.push_str(&format!(
                "model\t{}\t{}\t{}\t{}\t{}\n",
                w.model_id, w.model_path, w.context, inner.generation, w.required_bytes
            )),
            None => text.push_str(&format!("generation\t{}\n", inner.generation)),
        }
        for worker in &inner.applied {
            text.push_str(&format!("worker\t{}\t{}\n", worker.node_id, worker.address));
        }
        for t in &inner.tenants {
            text.push_str(&format!(
                "tenant\t{}\t{}\t{}\n",
                t.name,
                hex(&t.sha256),
                t.prefix
            ));
        }
        // Escrita atômica e durável, só para o dono: um corte no meio não deixa as chaves pela metade,
        // e o hash de uma chave já entregue ao operador não some em silêncio.
        let tmp = path.with_extension("tmp");
        let written = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        if let Err(error) = written {
            eprintln!("não consegui gravar {}: {error}", path.display());
        }
    }

    /// Pede o modelo. O plano de workers entra na hora (não espera a janela de estabilidade). Devolve
    /// a geração e as chaves criadas: o tenant `default` nasce no primeiro pedido.
    pub fn start(
        &self,
        model_id: &str,
        model_path: &str,
        context: u64,
        required_bytes: u64,
        endpoints: Vec<RpcEndpoint>,
    ) -> (u64, Vec<ApiKey>) {
        let mut inner = self.lock();
        inner.insufficient = None;
        inner.wanted = Some(Wanted {
            model_id: model_id.to_string(),
            model_path: model_path.to_string(),
            context,
            required_bytes,
        });
        inner.generation += 1;
        inner.applied = endpoints;
        inner.pending = None;
        inner.running = None;
        let mut keys = Vec::new();
        if !inner.tenants.iter().any(|t| t.name == DEFAULT_TENANT) {
            let key = new_key();
            inner.tenants.push(StoredTenant {
                name: DEFAULT_TENANT.to_string(),
                sha256: key_hash(&key),
                prefix: key.chars().take(8).collect(),
            });
            keys.push(ApiKey {
                tenant: DEFAULT_TENANT.to_string(),
                key,
            });
        }
        self.save(&inner);
        (inner.generation, keys)
    }

    /// Para o modelo. As chaves continuam valendo para o próximo `start`.
    pub fn stop(&self) -> u64 {
        let mut inner = self.lock();
        inner.wanted = None;
        inner.generation += 1;
        inner.applied.clear();
        inner.pending = None;
        inner.running = None;
        self.save(&inner);
        inner.generation
    }

    /// O modelo que o gateway deve aplicar agora. `endpoints` é a lista atual de workers aptos; ela só
    /// vira plano depois de ficar igual por `STABLE_MS` (e então a geração sobe). `None` = parar.
    pub fn plan(
        &self,
        mut endpoints: Vec<RpcEndpoint>,
        now_ms: u64,
        capacity_bytes: u64,
    ) -> Option<DesiredModel> {
        endpoints.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        let mut inner = self.lock();
        let wanted = inner.wanted.clone()?;
        // Sem RAM para o modelo nos workers de agora (um saiu, ou o pedido cabia só com ele): não
        // adianta carregar e deixar o `rpc-server` morrer por falta de memória. O modelo para e o
        // `status` diz o motivo; quando a RAM volta, o plano entra na hora.
        if wanted.required_bytes > 0 && capacity_bytes < wanted.required_bytes {
            inner.pending = None;
            inner.insufficient = Some(format!(
                "os workers de agora oferecem {:.2} GB e o modelo pede {:.2} GB",
                capacity_bytes as f64 / 1_073_741_824.0,
                wanted.required_bytes as f64 / 1_073_741_824.0
            ));
            return None;
        }
        if inner.insufficient.take().is_some() {
            inner.applied = endpoints.clone();
            inner.pending = None;
            inner.generation += 1;
            inner.running = None;
            self.save(&inner);
        }
        if endpoints == inner.applied {
            inner.pending = None;
        } else {
            match inner.pending.clone() {
                Some((ref list, since)) if *list == endpoints => {
                    if now_ms.saturating_sub(since) >= self.stable_ms {
                        inner.applied = endpoints;
                        inner.pending = None;
                        inner.generation += 1;
                        // O que o gateway reportou era da geração anterior.
                        inner.running = None;
                        self.save(&inner);
                    }
                }
                _ => inner.pending = Some((endpoints, now_ms)),
            }
        }
        let applied = inner.applied.clone();
        Some(DesiredModel {
            generation: inner.generation,
            model_id: wanted.model_id,
            model_path: wanted.model_path,
            context: wanted.context,
            gpu_layers: if applied.is_empty() { 0 } else { 99 },
            endpoints: applied,
        })
    }

    pub fn grants(&self) -> Vec<TenantGrant> {
        self.lock()
            .tenants
            .iter()
            .map(|t| TenantGrant {
                tenant: t.name.clone(),
                key_sha256: t.sha256.to_vec(),
                models: Vec::new(),
            })
            .collect()
    }

    /// O que o gateway reportou; vale só se for da geração em vigor.
    pub fn note_running(&self, status: ModelStatus) {
        let mut inner = self.lock();
        if status.generation == inner.generation {
            inner.running = Some(status);
        }
    }

    /// O pedido atual, o último estado reportado para ele e os prefixos das chaves.
    pub fn status(&self, now_ms: u64) -> (Option<DesiredModel>, Option<ModelStatus>, Vec<String>) {
        let _ = now_ms;
        let inner = self.lock();
        let desired = inner.wanted.as_ref().map(|w| DesiredModel {
            generation: inner.generation,
            model_id: w.model_id.clone(),
            model_path: w.model_path.clone(),
            context: w.context,
            gpu_layers: if inner.applied.is_empty() { 0 } else { 99 },
            endpoints: inner.applied.clone(),
        });
        if let (Some(reason), Some(d)) = (&inner.insufficient, &desired) {
            return (
                desired.clone(),
                Some(ModelStatus {
                    model_id: d.model_id.clone(),
                    state: ModelState::ModelFailed as i32,
                    reason: reason.clone(),
                    generation: d.generation,
                    tokens_per_second: 0.0,
                }),
                inner
                    .tenants
                    .iter()
                    .map(|t| format!("{}:{}", t.name, t.prefix))
                    .collect(),
            );
        }
        let running = inner.running.clone().or_else(|| {
            desired.as_ref().map(|d| ModelStatus {
                model_id: d.model_id.clone(),
                state: ModelState::ModelLoading as i32,
                reason: "o gateway ainda não reportou".into(),
                generation: d.generation,
                tokens_per_second: 0.0,
            })
        });
        let prefixes = inner
            .tenants
            .iter()
            .map(|t| format!("{}:{}", t.name, t.prefix))
            .collect();
        (desired, running, prefixes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(node: &str) -> RpcEndpoint {
        RpcEndpoint {
            node_id: node.into(),
            address: format!("{node}:50061"),
        }
    }

    fn orchestrator() -> Orchestrator {
        let o = Orchestrator::new(None);
        // janela curta nos testes
        Orchestrator {
            stable_ms: 1_000,
            ..o
        }
    }

    #[test]
    fn a_key_is_random_prefixed_and_only_its_hash_is_kept() {
        let (a, b) = (new_key(), new_key());
        assert!(a.starts_with("aic_") && a.len() == 4 + 43, "{a}");
        assert_ne!(a, b);
        assert!(
            a[4..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        let o = orchestrator();
        let (_, keys) = o.start("m", "m.gguf", 0, 0, vec![]);
        assert_eq!(keys.len(), 1);
        let grants = o.grants();
        assert_eq!(grants[0].key_sha256, key_hash(&keys[0].key).to_vec());
        assert!(
            !format!("{grants:?}").contains(&keys[0].key),
            "a chave em claro não pode vazar"
        );
        // Um segundo start não cria outra chave.
        assert!(o.start("m", "m.gguf", 0, 0, vec![]).1.is_empty());
    }

    #[test]
    fn start_and_stop_move_the_generation_and_the_plan() {
        let o = orchestrator();
        assert!(
            o.plan(vec![], 0, u64::MAX).is_none(),
            "sem pedido, nada a aplicar"
        );
        let (g1, _) = o.start(
            "m",
            "dir/m.gguf",
            2048,
            0,
            vec![endpoint("a"), endpoint("b")],
        );
        let plan = o
            .plan(vec![endpoint("b"), endpoint("a")], 10, u64::MAX)
            .unwrap();
        assert_eq!(
            (plan.generation, plan.gpu_layers, plan.endpoints.len()),
            (g1, 99, 2)
        );
        assert_eq!(plan.model_path, "dir/m.gguf");
        let g2 = o.stop();
        assert!(g2 > g1);
        assert!(o.plan(vec![endpoint("a")], 20, u64::MAX).is_none());
    }

    #[test]
    fn a_new_worker_changes_the_plan_only_after_the_stability_window() {
        let o = orchestrator();
        let (g1, _) = o.start("m", "m.gguf", 0, 0, vec![endpoint("a")]);
        // Chega o worker b: ainda vale o plano antigo.
        let early = o
            .plan(vec![endpoint("a"), endpoint("b")], 100, u64::MAX)
            .unwrap();
        assert_eq!((early.generation, early.endpoints.len()), (g1, 1));
        let still = o
            .plan(vec![endpoint("a"), endpoint("b")], 900, u64::MAX)
            .unwrap();
        assert_eq!(still.generation, g1);
        // Estável por 1 s: o plano muda e a geração sobe.
        let late = o
            .plan(vec![endpoint("a"), endpoint("b")], 1_200, u64::MAX)
            .unwrap();
        assert_eq!((late.generation, late.endpoints.len()), (g1 + 1, 2));
    }

    #[test]
    fn a_flapping_worker_never_reaches_the_window() {
        let o = orchestrator();
        let (g1, _) = o.start("m", "m.gguf", 0, 0, vec![endpoint("a")]);
        for t in 0..10 {
            let list = if t % 2 == 0 {
                vec![endpoint("a"), endpoint("b")]
            } else {
                vec![endpoint("a")]
            };
            let plan = o.plan(list, t * 400, u64::MAX).unwrap();
            assert_eq!(plan.generation, g1, "oscilou em t={t}");
        }
    }

    #[test]
    fn the_state_survives_a_restart_without_the_clear_key() {
        let dir = std::env::temp_dir().join(format!("aic-models-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("models.state");
        let first = Orchestrator::new(Some(path.clone()));
        let (generation, keys) = first.start("m", "m.gguf", 4096, 0, vec![endpoint("a")]);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&keys[0].key), "o arquivo guarda só o hash");

        let again = Orchestrator::new(Some(path));
        let (desired, _, prefixes) = again.status(0);
        let desired = desired.unwrap();
        assert_eq!(
            (desired.model_id.as_str(), desired.generation),
            ("m", generation)
        );
        assert_eq!(
            again.grants()[0].key_sha256,
            key_hash(&keys[0].key).to_vec()
        );
        assert!(prefixes[0].starts_with("default:aic_"));
    }

    #[test]
    fn only_the_current_generation_updates_the_status() {
        let o = orchestrator();
        let (generation, _) = o.start("m", "m.gguf", 0, 0, vec![]);
        let status = |g| ModelStatus {
            model_id: "m".into(),
            state: ModelState::ModelReady as i32,
            reason: String::new(),
            generation: g,
            tokens_per_second: 3.5,
        };
        o.note_running(status(generation - 1));
        assert_eq!(
            o.status(0).1.unwrap().state,
            ModelState::ModelLoading as i32
        );
        o.note_running(status(generation));
        assert_eq!(o.status(0).1.unwrap().tokens_per_second, 3.5);
    }
    #[test]
    fn the_applied_plan_survives_a_restart_and_a_new_generation_clears_the_status() {
        let dir = std::env::temp_dir().join(format!("aic-models-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("models.state");
        let first = Orchestrator::new(Some(path.clone()));
        let (generation, _) = first.start("m", "m.gguf", 0, 0, vec![endpoint("a"), endpoint("b")]);
        // Depois do reinício o plano em vigor é o mesmo: a geração não sobe à toa.
        let again = Orchestrator::new(Some(path));
        let plan = again
            .plan(vec![endpoint("a"), endpoint("b")], 5, u64::MAX)
            .unwrap();
        assert_eq!((plan.generation, plan.endpoints.len()), (generation, 2));
        // O estado reportado da geração antiga some quando o plano muda.
        let o = orchestrator();
        let (g, _) = o.start("m", "m.gguf", 0, 0, vec![endpoint("a")]);
        o.note_running(ModelStatus {
            model_id: "m".into(),
            state: ModelState::ModelReady as i32,
            reason: String::new(),
            generation: g,
            tokens_per_second: 9.0,
        });
        o.plan(vec![endpoint("a"), endpoint("b")], 100, u64::MAX);
        let changed = o
            .plan(vec![endpoint("a"), endpoint("b")], 1_500, u64::MAX)
            .unwrap();
        assert_eq!(changed.generation, g + 1);
        assert_eq!(
            o.status(0).1.unwrap().state,
            ModelState::ModelLoading as i32
        );
    }
    #[test]
    fn the_model_stops_when_the_workers_cannot_hold_it_and_returns_when_ram_does() {
        let o = orchestrator();
        let gib = 1u64 << 30;
        let (g1, _) = o.start(
            "m",
            "m.gguf",
            0,
            3 * gib,
            vec![endpoint("a"), endpoint("b")],
        );
        // Com a RAM de dois workers (4 GB) o plano segue.
        assert_eq!(
            o.plan(vec![endpoint("a"), endpoint("b")], 10, 4 * gib)
                .unwrap()
                .generation,
            g1
        );
        // Um worker saiu: 1 GB não comporta 3 GB. O modelo para e o status diz por quê.
        assert!(o.plan(vec![endpoint("a")], 20, gib).is_none());
        let (_, running, _) = o.status(0);
        let running = running.unwrap();
        assert_eq!(running.state, ModelState::ModelFailed as i32);
        assert!(
            running.reason.contains("pede 3.00 GB"),
            "{}",
            running.reason
        );
        // O worker voltou: o plano entra na hora, sem esperar a janela, com geração nova.
        let back = o
            .plan(vec![endpoint("a"), endpoint("b")], 30, 4 * gib)
            .unwrap();
        assert_eq!((back.generation, back.endpoints.len()), (g1 + 1, 2));
        // Sem exigência (modo só-gateway), nunca para por RAM.
        let (g, _) = o.start("m", "m.gguf", 0, 0, vec![]);
        assert_eq!(o.plan(vec![], 40, 0).unwrap().generation, g);
    }
}
