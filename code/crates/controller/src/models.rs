//! Orquestração do modelo do llama.cpp (ADR-022, E5 e E6): o que o operador pediu, o plano de workers
//! que o gateway aplica e as chaves de API. O controller guarda só o **hash** das chaves; a chave em
//! claro aparece uma vez, na criação.

use std::path::PathBuf;
use std::sync::Mutex;

use ai_cluster_protocol::cluster::{
    ApiKey, DesiredModel, KeyInfo, ModelState, ModelStatus, RpcEndpoint, TenantGrant,
};
use sha2::{Digest, Sha256};

/// Quanto tempo a lista de workers precisa ficar igual antes de o plano mudar: um worker que acabou
/// de entrar ainda está se medindo, e um que oscila não deve reiniciar o modelo a cada batida.
pub const STABLE_MS: u64 = 30_000;
/// Um worker que pode servir o `rpc-server`: onde chegar nele, quão rápida é a CPU dele (a pontuação de
/// computação do registro) e quanta RAM ele oferece.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub endpoint: RpcEndpoint,
    pub speed: f64,
    pub usable_bytes: u64,
}

/// Escolhe quais workers carregam o modelo e quanto de cada um. Cada worker que entra soma um salto de
/// rede por token, então um lento só entra quando a RAM dos mais rápidos não basta: com o modelo
/// cabendo no PC, o Pi ao lado o faria perder a maior parte da velocidade (medido: 38 tokens/s no PC
/// sozinho, 7 com 6% das camadas no Pi). Os mais rápidos entram primeiro, até somar `required` bytes;
/// a divisão é proporcional à velocidade, limitada pelo que cada um comporta. Devolve os endpoints
/// por `node_id` e as proporções na mesma ordem. Sem exigência de RAM (modo só-gateway) devolve todos,
/// sem proporção.
pub fn select(mut candidates: Vec<Candidate>, required: u64) -> (Vec<RpcEndpoint>, Vec<f64>) {
    if required == 0 {
        candidates.sort_by(|a, b| a.endpoint.node_id.cmp(&b.endpoint.node_id));
        return (
            candidates.into_iter().map(|c| c.endpoint).collect(),
            Vec::new(),
        );
    }
    let speed = |c: &Candidate| {
        if c.speed.is_finite() && c.speed > 1e-6 {
            c.speed
        } else {
            1e-6
        }
    };
    candidates.sort_by(|a, b| {
        speed(b)
            .total_cmp(&speed(a))
            .then_with(|| a.endpoint.node_id.cmp(&b.endpoint.node_id))
    });
    let mut chosen = Vec::new();
    let mut held: u64 = 0;
    for candidate in candidates {
        if held >= required {
            break;
        }
        held = held.saturating_add(candidate.usable_bytes);
        chosen.push(candidate);
    }
    // Proporcional à velocidade; quem passa do que comporta fica no teto e o resto vai para os demais.
    let cap = |c: &Candidate| (c.usable_bytes as f64 / required as f64).min(1.0);
    let mut share = vec![0.0f64; chosen.len()];
    let mut fixed = vec![false; chosen.len()];
    for _ in 0..=chosen.len() {
        let free: f64 = (0..chosen.len())
            .filter(|i| !fixed[*i])
            .map(|i| speed(&chosen[i]))
            .sum();
        let left = 1.0
            - (0..chosen.len())
                .filter(|i| fixed[*i])
                .map(|i| share[i])
                .sum::<f64>();
        if free <= 0.0 || left <= 0.0 {
            break;
        }
        let mut changed = false;
        for i in 0..chosen.len() {
            if fixed[i] {
                continue;
            }
            share[i] = left * speed(&chosen[i]) / free;
            if share[i] > cap(&chosen[i]) {
                share[i] = cap(&chosen[i]);
                fixed[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut pairs: Vec<(RpcEndpoint, f64)> = chosen
        .into_iter()
        .zip(share)
        .map(|(c, s)| (c.endpoint, s))
        .collect();
    pairs.sort_by(|a, b| a.0.node_id.cmp(&b.0.node_id));
    let total: f64 = pairs.iter().map(|(_, s)| *s).sum();
    let split = if total > 0.0 {
        pairs
            .iter()
            .map(|(_, s)| (s / total * 1000.0).round() / 1000.0)
            .collect()
    } else {
        Vec::new()
    };
    (pairs.into_iter().map(|(e, _)| e).collect(), split)
}

/// A divisão só é enviada completa: uma proporção por worker e alguma maior que zero.
fn usable_split(endpoints: &[RpcEndpoint], split: &[f64]) -> Vec<f64> {
    if split.len() == endpoints.len() && split.iter().any(|share| *share > 0.0) {
        split.to_vec()
    } else {
        Vec::new()
    }
}

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
    /// Vazio = todos os modelos.
    models: Vec<String>,
    /// Tokens por janela; 0 = sem cota.
    tokens: u64,
    /// Classe máxima de prioridade; vazio = a maior.
    class: String,
}

/// Quantos tenants o controller guarda.
const MAX_TENANTS: usize = 64;
const CLASSES: [&str; 5] = ["background", "batch", "normal", "interactive", "realtime"];

/// Nome seguro para rótulo de métrica e para o arquivo de estado.
fn valid_tenant_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[derive(Default, Debug)]
struct Inner {
    wanted: Option<Wanted>,
    generation: u64,
    applied: Vec<RpcEndpoint>,
    /// Proporção de camadas de cada worker de `applied` (vazio = automático).
    applied_split: Vec<f64>,
    /// A lista nova que está esperando a janela de estabilidade, e desde quando.
    pending: Option<(Vec<RpcEndpoint>, Vec<f64>, u64)>,
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
                    ["worker", node_id, address, rest @ ..] => {
                        inner.applied.push(RpcEndpoint {
                            node_id: node_id.to_string(),
                            address: address.to_string(),
                        });
                        if let Some(share) = rest.first().and_then(|v| v.parse::<f64>().ok()) {
                            inner.applied_split.push(share);
                        }
                    }
                    ["generation", generation] => {
                        inner.generation = generation.parse().unwrap_or(inner.generation);
                    }
                    ["tenant", name, sha, prefix, rest @ ..] => {
                        if let Some(sha256) = unhex(sha) {
                            inner.tenants.push(StoredTenant {
                                name: name.to_string(),
                                sha256,
                                prefix: prefix.to_string(),
                                models: rest
                                    .first()
                                    .map(|m| {
                                        m.split(',')
                                            .filter(|m| !m.is_empty())
                                            .map(str::to_string)
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                tokens: rest.get(1).and_then(|v| v.parse().ok()).unwrap_or(0),
                                class: rest.get(2).map(|c| c.to_string()).unwrap_or_default(),
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
        for (i, worker) in inner.applied.iter().enumerate() {
            let share = inner.applied_split.get(i).copied().unwrap_or(0.0);
            text.push_str(&format!(
                "worker\t{}\t{}\t{share}\n",
                worker.node_id, worker.address
            ));
        }
        for t in &inner.tenants {
            text.push_str(&format!(
                "tenant\t{}\t{}\t{}\t{}\t{}\t{}\n",
                t.name,
                hex(&t.sha256),
                t.prefix,
                t.models.join(","),
                t.tokens,
                t.class
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
        split: Vec<f64>,
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
        inner.applied_split = split;
        inner.pending = None;
        inner.running = None;
        let mut keys = Vec::new();
        if !inner.tenants.iter().any(|t| t.name == DEFAULT_TENANT) {
            let key = new_key();
            inner.tenants.push(StoredTenant {
                name: DEFAULT_TENANT.to_string(),
                sha256: key_hash(&key),
                prefix: key.chars().take(8).collect(),
                models: Vec::new(),
                tokens: 0,
                class: String::new(),
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
    pub fn plan(&self, candidates: Vec<Candidate>, now_ms: u64) -> Option<DesiredModel> {
        let capacity_bytes = candidates
            .iter()
            .map(|c| c.usable_bytes)
            .fold(0, u64::saturating_add);
        let mut inner = self.lock();
        let wanted = inner.wanted.clone()?;
        let (endpoints, split) = select(candidates, wanted.required_bytes);
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
            inner.applied_split = split.clone();
            inner.pending = None;
            inner.generation += 1;
            inner.running = None;
            self.save(&inner);
        }
        if endpoints == inner.applied {
            inner.pending = None;
        } else {
            match inner.pending.clone() {
                Some((ref list, _, since)) if *list == endpoints => {
                    if now_ms.saturating_sub(since) >= self.stable_ms {
                        inner.applied = endpoints;
                        inner.applied_split = split;
                        inner.pending = None;
                        inner.generation += 1;
                        // O que o gateway reportou era da geração anterior.
                        inner.running = None;
                        self.save(&inner);
                    }
                }
                _ => inner.pending = Some((endpoints, split, now_ms)),
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
            split: usable_split(&inner.applied, &inner.applied_split),
        })
    }

    pub fn grants(&self) -> Vec<TenantGrant> {
        self.lock()
            .tenants
            .iter()
            .map(|t| TenantGrant {
                tenant: t.name.clone(),
                key_sha256: t.sha256.to_vec(),
                models: t.models.clone(),
                tokens: t.tokens,
                class: t.class.clone(),
            })
            .collect()
    }

    /// Cria a chave de um tenant novo, ou (com `rotate`) troca a de um que existe: a antiga deixa de
    /// valer na próxima apresentação do gateway. A chave em claro volta só aqui.
    pub fn add_key(
        &self,
        tenant: &str,
        models: Vec<String>,
        tokens: u64,
        class: &str,
        rotate: bool,
    ) -> Result<ApiKey, String> {
        if !valid_tenant_name(tenant) {
            return Err("nome de tenant inválido: use letras, dígitos, '_' e '-' (até 64)".into());
        }
        if !class.is_empty() && !CLASSES.contains(&class) {
            return Err(format!("classe inválida: use {}", CLASSES.join(", ")));
        }
        if models.len() > 32
            || models.iter().any(|m| {
                m.is_empty()
                    || m.len() > 64
                    || !m
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
            })
        {
            return Err("lista de modelos inválida".into());
        }
        let mut inner = self.lock();
        let position = inner.tenants.iter().position(|t| t.name == tenant);
        match (position, rotate) {
            (Some(_), false) => {
                return Err(format!(
                    "o tenant {tenant} já existe: use `model key rotate`"
                ));
            }
            (None, true) => return Err(format!("o tenant {tenant} não existe")),
            (None, false) if inner.tenants.len() >= MAX_TENANTS => {
                return Err("tenants demais".into());
            }
            _ => {}
        }
        let key = new_key();
        let stored = StoredTenant {
            name: tenant.to_string(),
            sha256: key_hash(&key),
            prefix: key.chars().take(8).collect(),
            // Girar a chave sem pedir limites novos mantém os que o tenant já tinha.
            models: match position {
                Some(i) if models.is_empty() => inner.tenants[i].models.clone(),
                _ => models,
            },
            tokens: match position {
                Some(i) if tokens == 0 => inner.tenants[i].tokens,
                _ => tokens,
            },
            class: match position {
                Some(i) if class.is_empty() => inner.tenants[i].class.clone(),
                _ => class.to_string(),
            },
        };
        match position {
            Some(i) => inner.tenants[i] = stored,
            None => inner.tenants.push(stored),
        }
        self.save(&inner);
        Ok(ApiKey {
            tenant: tenant.to_string(),
            key,
        })
    }

    /// Tira o tenant; a chave dele para de valer na próxima apresentação do gateway.
    pub fn remove_key(&self, tenant: &str) -> bool {
        let mut inner = self.lock();
        let before = inner.tenants.len();
        inner.tenants.retain(|t| t.name != tenant);
        let removed = inner.tenants.len() != before;
        if removed {
            self.save(&inner);
        }
        removed
    }

    /// Os tenants, com o prefixo da chave e os limites (nunca a chave).
    pub fn list_keys(&self) -> Vec<KeyInfo> {
        self.lock()
            .tenants
            .iter()
            .map(|t| KeyInfo {
                tenant: t.name.clone(),
                prefix: t.prefix.clone(),
                models: t.models.clone(),
                tokens: t.tokens,
                class: t.class.clone(),
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
            split: usable_split(&inner.applied, &inner.applied_split),
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

    /// Workers iguais, com RAM de sobra: sem exigência de RAM o plano usa todos.
    fn cands(names: &[&str]) -> Vec<Candidate> {
        names
            .iter()
            .map(|name| Candidate {
                endpoint: endpoint(name),
                speed: 1.0,
                usable_bytes: 1 << 40,
            })
            .collect()
    }

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
        let (_, keys) = o.start("m", "m.gguf", 0, 0, vec![], vec![]);
        assert_eq!(keys.len(), 1);
        let grants = o.grants();
        assert_eq!(grants[0].key_sha256, key_hash(&keys[0].key).to_vec());
        assert!(
            !format!("{grants:?}").contains(&keys[0].key),
            "a chave em claro não pode vazar"
        );
        // Um segundo start não cria outra chave.
        assert!(o.start("m", "m.gguf", 0, 0, vec![], vec![]).1.is_empty());
    }

    #[test]
    fn start_and_stop_move_the_generation_and_the_plan() {
        let o = orchestrator();
        assert!(
            o.plan(cands(&[]), 0).is_none(),
            "sem pedido, nada a aplicar"
        );
        let (g1, _) = o.start(
            "m",
            "dir/m.gguf",
            2048,
            0,
            vec![endpoint("a"), endpoint("b")],
            vec![],
        );
        let plan = o.plan(cands(&["b", "a"]), 10).unwrap();
        assert_eq!(
            (plan.generation, plan.gpu_layers, plan.endpoints.len()),
            (g1, 99, 2)
        );
        assert_eq!(plan.model_path, "dir/m.gguf");
        let g2 = o.stop();
        assert!(g2 > g1);
        assert!(o.plan(cands(&["a"]), 20).is_none());
    }

    #[test]
    fn a_new_worker_changes_the_plan_only_after_the_stability_window() {
        let o = orchestrator();
        let (g1, _) = o.start("m", "m.gguf", 0, 0, vec![endpoint("a")], vec![]);
        // Chega o worker b: ainda vale o plano antigo.
        let early = o.plan(cands(&["a", "b"]), 100).unwrap();
        assert_eq!((early.generation, early.endpoints.len()), (g1, 1));
        let still = o.plan(cands(&["a", "b"]), 900).unwrap();
        assert_eq!(still.generation, g1);
        // Estável por 1 s: o plano muda e a geração sobe.
        let late = o.plan(cands(&["a", "b"]), 1_200).unwrap();
        assert_eq!((late.generation, late.endpoints.len()), (g1 + 1, 2));
    }

    #[test]
    fn a_flapping_worker_never_reaches_the_window() {
        let o = orchestrator();
        let (g1, _) = o.start("m", "m.gguf", 0, 0, vec![endpoint("a")], vec![]);
        for t in 0..10 {
            let list = if t % 2 == 0 {
                cands(&["a", "b"])
            } else {
                cands(&["a"])
            };
            let plan = o.plan(list, t * 400).unwrap();
            assert_eq!(plan.generation, g1, "oscilou em t={t}");
        }
    }

    #[test]
    fn the_state_survives_a_restart_without_the_clear_key() {
        let dir = std::env::temp_dir().join(format!("aic-models-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("models.state");
        let first = Orchestrator::new(Some(path.clone()));
        let (generation, keys) = first.start("m", "m.gguf", 4096, 0, vec![endpoint("a")], vec![]);
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
        let (generation, _) = o.start("m", "m.gguf", 0, 0, vec![], vec![]);
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
        let (generation, _) = first.start(
            "m",
            "m.gguf",
            0,
            0,
            vec![endpoint("a"), endpoint("b")],
            vec![],
        );
        // Depois do reinício o plano em vigor é o mesmo: a geração não sobe à toa.
        let again = Orchestrator::new(Some(path));
        let plan = again.plan(cands(&["a", "b"]), 5).unwrap();
        assert_eq!((plan.generation, plan.endpoints.len()), (generation, 2));
        // O estado reportado da geração antiga some quando o plano muda.
        let o = orchestrator();
        let (g, _) = o.start("m", "m.gguf", 0, 0, vec![endpoint("a")], vec![]);
        o.note_running(ModelStatus {
            model_id: "m".into(),
            state: ModelState::ModelReady as i32,
            reason: String::new(),
            generation: g,
            tokens_per_second: 9.0,
        });
        o.plan(cands(&["a", "b"]), 100);
        let changed = o.plan(cands(&["a", "b"]), 1_500).unwrap();
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
            vec![],
        );
        // Com a RAM de dois workers (2 GB cada) o plano segue.
        let two = || vec![candidate("a", 1.0, 2.0), candidate("b", 1.0, 2.0)];
        assert_eq!(o.plan(two(), 10).unwrap().generation, g1);
        // Um worker saiu: 1 GB não comporta 3 GB. O modelo para e o status diz por quê.
        assert!(o.plan(vec![candidate("a", 1.0, 1.0)], 20).is_none());
        let (_, running, _) = o.status(0);
        let running = running.unwrap();
        assert_eq!(running.state, ModelState::ModelFailed as i32);
        assert!(
            running.reason.contains("pede 3.00 GB"),
            "{}",
            running.reason
        );
        // O worker voltou: o plano entra na hora, sem esperar a janela, com geração nova.
        let back = o.plan(two(), 30).unwrap();
        assert_eq!((back.generation, back.endpoints.len()), (g1 + 1, 2));
        // Sem exigência (modo só-gateway), nunca para por RAM.
        let (g, _) = o.start("m", "m.gguf", 0, 0, vec![], vec![]);
        assert_eq!(o.plan(vec![], 40).unwrap().generation, g);
    }

    fn candidate(node: &str, speed: f64, gib: f64) -> Candidate {
        Candidate {
            endpoint: endpoint(node),
            speed,
            usable_bytes: (gib * (1u64 << 30) as f64) as u64,
        }
    }

    #[test]
    fn a_slow_worker_stays_out_while_the_fast_ones_hold_the_model() {
        let gib = 1u64 << 30;
        let (workers, split) = select(
            vec![candidate("pi", 0.02, 0.8), candidate("pc", 1.1, 3.0)],
            gib, // 1 GB cabe no PC
        );
        assert_eq!(
            workers
                .iter()
                .map(|w| w.node_id.as_str())
                .collect::<Vec<_>>(),
            ["pc"]
        );
        assert_eq!(split, vec![1.0]);
    }

    #[test]
    fn a_slow_worker_joins_when_the_ram_of_the_fast_ones_is_not_enough() {
        let gib = 1u64 << 30;
        let (workers, split) = select(
            vec![candidate("pi", 0.02, 0.8), candidate("pc", 1.1, 3.0)],
            3 * gib + gib / 2, // 3,5 GB: o PC (3 GB) sozinho não basta
        );
        assert_eq!(
            workers
                .iter()
                .map(|w| w.node_id.as_str())
                .collect::<Vec<_>>(),
            ["pc", "pi"]
        );
        // O PC é limitado pelo que comporta (3/3,5 do modelo); o resto vai para o Pi.
        assert!((split[0] - 3.0 / 3.5).abs() < 0.01, "{split:?}");
        assert!((split[1] - 0.5 / 3.5).abs() < 0.01, "{split:?}");
        assert!((split.iter().sum::<f64>() - 1.0).abs() < 0.01);
    }

    #[test]
    fn similar_workers_split_by_speed_and_never_past_their_ram() {
        let gib = 1u64 << 30;
        // Dois PCs, um duas vezes mais rápido; o modelo (10 GB) pede os dois: 2/3 e 1/3.
        let (workers, split) = select(
            vec![candidate("a", 2.0, 8.0), candidate("b", 1.0, 8.0)],
            10 * gib,
        );
        assert_eq!(workers.len(), 2);
        assert!(
            (split[0] - 2.0 / 3.0).abs() < 0.01 && (split[1] - 1.0 / 3.0).abs() < 0.01,
            "{split:?}"
        );
        // O mais rápido tem pouca RAM: fica no teto dele e o outro leva o resto.
        let (_, capped) = select(
            vec![candidate("a", 2.0, 2.0), candidate("b", 1.0, 8.0)],
            6 * gib,
        );
        assert!((capped[0] - 2.0 / 6.0).abs() < 0.01, "{capped:?}");
        assert!((capped[1] - 4.0 / 6.0).abs() < 0.01, "{capped:?}");
    }

    #[test]
    fn selection_is_stable_and_handles_missing_scores() {
        let gib = 1u64 << 30;
        let a = select(
            vec![candidate("x", 0.0, 4.0), candidate("y", f64::NAN, 4.0)],
            gib,
        );
        let b = select(
            vec![candidate("y", f64::NAN, 4.0), candidate("x", 0.0, 4.0)],
            gib,
        );
        assert_eq!(a.0, b.0, "a ordem de chegada não muda o plano");
        // Sem exigência (só-gateway): todos, sem proporção.
        let (all, none) = select(vec![candidate("b", 1.0, 1.0), candidate("a", 1.0, 1.0)], 0);
        assert_eq!(all.len(), 2);
        assert!(none.is_empty());
        assert!(select(Vec::new(), gib).0.is_empty());
    }
    #[test]
    fn keys_are_added_rotated_listed_and_removed_with_their_limits() {
        let o = orchestrator();
        let acme = o
            .add_key("acme", vec!["m1".into()], 1000, "batch", false)
            .unwrap();
        assert!(acme.key.starts_with("aic_"));
        // Repetir o nome sem `rotate` falha; girar o que não existe também.
        assert!(
            o.add_key("acme", vec![], 0, "", false)
                .unwrap_err()
                .contains("já existe")
        );
        assert!(
            o.add_key("novo", vec![], 0, "", true)
                .unwrap_err()
                .contains("não existe")
        );
        // Nome, classe e modelos inválidos.
        for (name, class, models) in [
            ("com espaço", "", vec![]),
            ("", "", vec![]),
            ("ok", "turbo", vec![]),
            ("ok", "", vec!["id ruim".to_string()]),
        ] {
            assert!(
                o.add_key(name, models, 0, class, false).is_err(),
                "{name:?} {class:?}"
            );
        }
        // Girar troca a chave e mantém os limites que o tenant tinha.
        let rotated = o.add_key("acme", vec![], 0, "", true).unwrap();
        assert_ne!(rotated.key, acme.key);
        let info = o
            .list_keys()
            .into_iter()
            .find(|k| k.tenant == "acme")
            .unwrap();
        assert_eq!(
            (info.tokens, info.class.as_str(), info.models.len()),
            (1000, "batch", 1)
        );
        let grant = o.grants().into_iter().find(|g| g.tenant == "acme").unwrap();
        assert_eq!(grant.key_sha256, key_hash(&rotated.key).to_vec());
        assert_ne!(
            grant.key_sha256,
            key_hash(&acme.key).to_vec(),
            "a chave antiga deixou de valer"
        );
        assert_eq!((grant.tokens, grant.class.as_str()), (1000, "batch"));
        assert!(o.remove_key("acme"));
        assert!(!o.remove_key("acme"));
        assert!(o.list_keys().is_empty());
    }

    #[test]
    fn tenants_and_their_limits_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("aic-models-keys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("models.state");
        let first = Orchestrator::new(Some(path.clone()));
        let key = first
            .add_key(
                "acme",
                vec!["a".into(), "b".into()],
                42,
                "interactive",
                false,
            )
            .unwrap();
        let again = Orchestrator::new(Some(path));
        let grant = again
            .grants()
            .into_iter()
            .find(|g| g.tenant == "acme")
            .unwrap();
        assert_eq!(grant.key_sha256, key_hash(&key.key).to_vec());
        assert_eq!(grant.models, vec!["a".to_string(), "b".to_string()]);
        assert_eq!((grant.tokens, grant.class.as_str()), (42, "interactive"));
    }
}
