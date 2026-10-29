//! Inference API gateway (Fase J1/J3): the HTTP door the Application Plane uses (`inference-api`).
//!
//! Two models sit behind it. `gpt2` (J3, only when `AI_CLUSTER_GPT2_DIR` is set) is a real
//! language model, run by the in-process Inference Engine for `/v1/chat/completions`; its matrix
//! products do not go through the cluster yet. The other, `matmul-diamond-v0`, is a
//! synthetic forward pass — the diamond matrix graph — that really runs on the cluster, and
//! whose output is returned as an "embedding". The vector comes from a hash of the input pushed
//! through matrix multiplications; it carries no meaning. Every response says so, so no client
//! mistakes it for a real model. A real model replaces this by changing what `run_graph` asks
//! the controller for; the HTTP contract, auth and limits stay.

mod cache;
mod llama;
mod metrics;
mod request_manager;
mod tls;

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ai_cluster_common::matrix::Matrix;
use ai_cluster_engine::{
    Executor, LanguageModel, Sampling, Slice, chat::ChatMl, gpt2::Gpt2, llama::Llama,
    manifest::verified_version, tokenizer::Tokenizer,
};
use ai_cluster_protocol::cluster::{
    CancelRequestRequest, EnqueueGraphRequest, GatewayModel, GetOutputRequest, GetTraceRequest,
    NodeShardCapacity, ReportGatewayRequest, ShardCapacityRequest, ShardMatmulRequest, ShardOp,
};
use ai_cluster_protocol::metadata::{
    FAILED_NODE_HEADER, INFERENCE_TOKEN_ENV, INSECURE_ENV, JOIN_TOKEN_ENV, MetadataClient,
    OPERATOR_TOKEN_ENV, connect_inference, connect_operator, inference_token, insecure_allowed,
    join_token, operator_token,
};
use axum::body::{Body, Bytes};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use request_manager::{Admission, Class, Permit, Tenant, Tenants};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::Semaphore;
use tokio::time::Instant;

const MODEL_ID: &str = "matmul-diamond-v0";
/// Edge of the matrices, and so the length of the returned vector.
const DIMENSIONS: u32 = 16;
/// Units in the diamond graph: A, B, C, D.
const GRAPH_UNITS: usize = 4;
const MAX_INPUT_BYTES: usize = 8 * 1024;
const BODY_LIMIT: usize = 64 * 1024;
const DEADLINE: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(50);
/// Requests running at once. Past it the gateway answers 429 instead of queueing without bound.
const MAX_IN_FLIGHT: usize = 4;

const DEFAULT_NEW_TOKENS: u32 = 16;
const MAX_NEW_TOKENS: u32 = 64;
const MAX_PROMPT_TOKENS: usize = 512;
const MAX_MESSAGES: usize = 32;

/// A language model and what it needs to serve a request.
struct Llm {
    id: &'static str,
    /// From the model's `manifest.json`, checked at startup; `None` when it has none.
    version: Option<String>,
    description: &'static str,
    model: Box<dyn LanguageModel + Send + Sync>,
    tokenizer: Tokenizer,
    /// The token that ends a text (`<|endoftext|>`); generation stops at it.
    end_of_text: u32,
    /// Set when the projections run on workers (J5).
    remote: Option<Arc<RemoteStats>>,
    /// The nodes that hold the model's slices; empty when it runs whole in the gateway.
    shards: Vec<String>,
    /// Instruct models take their conversation as ChatML turns; base models just continue text.
    chat: Option<ChatMl>,
}

/// Where the time of a remote generation went, since the last read.
#[derive(Default)]
struct RemoteStats {
    calls: AtomicU64,
    /// Wall time of the whole call: relay, network and worker.
    wall_us: AtomicU64,
    /// The slowest worker's own compute per call, summed: the part that is not transport.
    compute_us: AtomicU64,
    /// Calls that had to be redone on other nodes after one failed.
    retries: AtomicU64,
    /// The running generation's cancel flag (timeout, client gone); a call in flight watches it.
    /// One generation runs at a time, so a single slot is enough.
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

impl RemoteStats {
    fn bind(&self, flag: &Arc<AtomicBool>) {
        *self.cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(flag.clone());
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }
}

/// Binds the generation's cancel flag to the model's remote calls, if it has any.
fn bind_cancel(llm: &Llm, flag: &Arc<AtomicBool>) {
    if let Some(stats) = &llm.remote {
        stats.bind(flag);
    }
}

/// How long a node that failed is left out before it is tried again.
const BENCH: Duration = Duration::from_secs(30);

/// The nodes a model's slices may run on. A node that fails is set aside for [`BENCH`] and its
/// bands go to the others: a worker dying costs a retry, not the request. Any worker can compute
/// any band (it reads it from its own disk), so nothing is copied when bands move.
struct Roster {
    nodes: Vec<String>,
    benched: Mutex<HashMap<String, std::time::Instant>>,
}

impl Roster {
    fn new(nodes: Vec<String>) -> Self {
        Self {
            nodes,
            benched: Mutex::new(HashMap::new()),
        }
    }

    fn live(&self) -> Vec<&str> {
        let mut benched = self.benched.lock().unwrap_or_else(|p| p.into_inner());
        benched.retain(|_, since| since.elapsed() < BENCH);
        self.nodes
            .iter()
            .map(String::as_str)
            .filter(|node| !benched.contains_key(*node))
            .collect()
    }

    fn bench(&self, node: &str) {
        self.benched
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(node.to_string(), std::time::Instant::now());
    }

    /// `slices` with every band of a benched node handed to a live one.
    fn assign(&self, slices: &[Slice]) -> Result<Vec<Slice>, String> {
        let mut live = self.live();
        if live.is_empty() {
            // Everyone failed recently: try them all again rather than refusing for the whole
            // bench time; a node that is really gone fails fast and ends the retries.
            live = self.nodes.iter().map(String::as_str).collect();
        }
        if live.is_empty() {
            return Err("fatias: nenhum worker configurado".into());
        }
        Ok(slices
            .iter()
            .enumerate()
            .map(|(i, slice)| {
                let mut slice = slice.clone();
                if !live.contains(&slice.node.as_str()) {
                    slice.node = live[i % live.len()].to_string();
                }
                slice
            })
            .collect())
    }
}

/// Sends projections to the workers through the controller's resident channels.
struct GrpcExecutor {
    client: MetadataClient,
    runtime: tokio::runtime::Handle,
    subdir: String,
    stats: Arc<RemoteStats>,
    roster: Roster,
}

/// The node ids listed (comma-separated) in the environment variable `var`.
fn shard_nodes(var: &str) -> Vec<String> {
    std::env::var(var)
        .unwrap_or_default()
        .split(',')
        .map(|node| node.trim().to_string())
        .filter(|node| !node.is_empty())
        .collect()
}

fn grpc_executor(
    client: &MetadataClient,
    subdir_var: &str,
    default_subdir: &str,
    nodes: Vec<String>,
) -> (Arc<GrpcExecutor>, Arc<RemoteStats>) {
    let stats = Arc::new(RemoteStats::default());
    let executor = Arc::new(GrpcExecutor {
        client: client.clone(),
        runtime: tokio::runtime::Handle::current(),
        subdir: std::env::var(subdir_var).unwrap_or_else(|_| default_subdir.to_string()),
        stats: stats.clone(),
        roster: Roster::new(nodes),
    });
    (executor, stats)
}

/// Presents this gateway to the controller now and every `GATEWAY_REPORT_SECS`, so the operator's
/// topology shows it: who it is, where clients reach it, which models it serves and which nodes hold
/// their slices. Best effort: a controller that is down or too old only means it is not listed.
const GATEWAY_REPORT_SECS: u64 = 5;
/// Enquanto o modelo carrega: o controller ouve "pronto" e entrega o plano novo sem esperar o ciclo
/// normal (medido: dois ciclos de 15 s eram um terço do tempo até PRONTO).
const GATEWAY_REPORT_LOADING_SECS: u64 = 1;

/// Where the counters survive a restart: `AI_CLUSTER_GATEWAY_STATE`, else a file named for the
/// gateway under `AI_CLUSTER_DATA_DIR`. None when neither is set (counters then start at zero).
fn gateway_state_path(id: &str) -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("AI_CLUSTER_GATEWAY_STATE") {
        return Some(path.into());
    }
    let dir = std::env::var_os("AI_CLUSTER_DATA_DIR")?;
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // O hash do id original separa ids que a limpeza do nome tornaria iguais ("a.b" e "a_b").
    let hash = id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    });
    Some(std::path::PathBuf::from(dir).join(format!("gateway-{safe}-{:08x}.state", hash as u32)))
}

/// As chaves que o controller entrega: só as bem formadas (hash de 32 bytes e nome seguro).
fn grants_from(list: &[ai_cluster_protocol::cluster::TenantGrant]) -> Vec<request_manager::Grant> {
    list.iter()
        .filter_map(|grant| {
            let sha256 = <[u8; 32]>::try_from(grant.key_sha256.as_slice()).ok()?;
            let safe = !grant.tenant.is_empty()
                && grant.tenant.len() <= 64
                && grant
                    .tenant
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            safe.then(|| request_manager::Grant {
                name: grant.tenant.clone(),
                sha256,
                models: (!grant.models.is_empty()).then(|| grant.models.clone()),
                quota: (grant.tokens > 0).then_some(grant.tokens),
                ceiling: request_manager::Class::parse(&grant.class),
            })
        })
        .collect()
}

fn announce_gateway(
    client: &MetadataClient,
    llms: &[Arc<Llm>],
    addr: SocketAddr,
    cache: Arc<cache::ResponseCache>,
    tenants: Arc<request_manager::Tenants>,
    llama: Arc<llama::Manager>,
) {
    let id = std::env::var("AI_CLUSTER_GATEWAY_ID").unwrap_or_else(|_| format!("gateway-{addr}"));
    let url = std::env::var("AI_CLUSTER_GATEWAY_URL").unwrap_or_else(|_| format!("http://{addr}"));
    let models: Vec<GatewayModel> = llms
        .iter()
        .map(|llm| GatewayModel {
            model_id: llm.id.to_string(),
            shard_nodes: llm.shards.clone(),
        })
        .collect();
    // O controller recusa para sempre um id ou uma URL fora do padrão; melhor dizer isso uma vez, na
    // partida, do que repetir o erro a cada apresentação.
    let plain = |text: &str, max: usize| {
        !text.is_empty() && text.len() <= max && text.chars().all(|c| c.is_ascii_graphic())
    };
    if !plain(&id, 64) || !(url.is_empty() || plain(&url, 256)) {
        eprintln!(
            "AI_CLUSTER_GATEWAY_ID/URL fora do padrão (ASCII gráfico, id até 64 e URL até 256): o gateway não se apresenta ao controller"
        );
        return;
    }
    // The counters start from the saved base, before the first request is served.
    let state_path = gateway_state_path(&id);
    if let Some(path) = state_path.as_deref()
        && metrics::load_state(path).is_none()
        && path.exists()
    {
        // Um arquivo que existe mas não lê é guardado ao lado, para o próximo salvamento não gravar
        // zeros por cima de uma base que talvez ainda se recupere.
        let bad = path.with_extension("bad");
        eprintln!(
            "estado do gateway em {} ilegível; guardado em {} e os contadores partem de zero",
            path.display(),
            bad.display()
        );
        let _ = std::fs::rename(path, bad);
    }
    if let Some(base) = state_path.as_deref().and_then(metrics::load_state) {
        metrics::restore(&base);
        cache.restore_counts(
            base.cache_hits,
            base.cache_misses,
            base.cache_tokens_avoided,
        );
    }
    let mut client = client.clone();
    tokio::spawn(async move {
        let mut wait = Duration::ZERO;
        let mut failing = false;
        let mut ring = metrics::RateRing::default();
        loop {
            tokio::time::sleep(wait).await;
            // Sampled before the send and even when the send fails, so a controller outage leaves
            // no hole in the rates.
            let now_s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            let mut stats = metrics::stats();
            ring.sample(now_s, stats.requests, stats.generated_tokens);
            (
                stats.cache_hits,
                stats.cache_misses,
                stats.cache_tokens_avoided,
            ) = cache.counts();
            (
                stats.requests_per_second_1m,
                stats.generated_tokens_per_second_1m,
            ) = ring.rates(now_s, 60);
            (
                stats.requests_per_second_5m,
                stats.generated_tokens_per_second_5m,
            ) = ring.rates(now_s, 300);
            if let Some(path) = &state_path {
                metrics::save_state(path, &stats);
            }
            let report = ReportGatewayRequest {
                gateway_id: id.clone(),
                url: url.clone(),
                models: models.clone(),
                stats: Some(stats),
                running: llama.status(),
                llama_version: std::env::var("AI_CLUSTER_RPC_VERSION").unwrap_or_default(),
                llama_slots: llama.slots(),
            };
            // loga só a mudança de estado: um controller fora do ar não enche o log a cada 15 s
            match client.report_gateway(report).await {
                Err(err) => {
                    if !failing {
                        failing = true;
                        eprintln!("não consegui me apresentar ao controller: {err}");
                    }
                }
                Ok(response) => {
                    if failing {
                        failing = false;
                        eprintln!("apresentação ao controller restabelecida");
                    }
                    // O controller entrega as chaves (em hash) e o modelo que quer; o gateway aplica.
                    let reply = response.into_inner();
                    tenants.set_grants(grants_from(&reply.tenants));
                    llama.apply(reply.desired.as_ref()).await;
                }
            }
            let loading = llama.status().is_some_and(|s| {
                s.state == ai_cluster_protocol::cluster::ModelState::ModelLoading as i32
            });
            wait = Duration::from_secs(if loading {
                GATEWAY_REPORT_LOADING_SECS
            } else {
                GATEWAY_REPORT_SECS
            });
        }
    });
}

/// A SmolLM2 checkpoint the gateway can serve, and the environment variables that configure it.
/// `<prefix>_DIR` turns it on; `<prefix>_SHARDS` (comma-separated node ids) puts the seven
/// projections of every layer on those workers, each holding a band of rows resident, instead of
/// running the whole model here; `<prefix>_SUBDIR` names its directory under the workers' model
/// root.
struct LlamaSpec {
    id: &'static str,
    description: &'static str,
    prefix: &'static str,
    default_subdir: &'static str,
    /// ChatML turns and `<|im_end|>` as the end of a reply.
    instruct: bool,
}

const LLAMAS: &[LlamaSpec] = &[
    LlamaSpec {
        id: "smollm2-135m",
        description: "SmolLM2-135M (Llama) no gateway, com as projeções nos workers se houver \
            AI_CLUSTER_SMOLLM2_SHARDS; modelo base, sem template de chat. Só /v1/chat/completions.",
        prefix: "AI_CLUSTER_SMOLLM2",
        default_subdir: "smollm2",
        instruct: false,
    },
    LlamaSpec {
        id: "smollm2-135m-instruct",
        description: "SmolLM2-135M-Instruct (Llama) com template ChatML, com as projeções nos \
            workers se houver AI_CLUSTER_SMOLLM2_INSTRUCT_SHARDS. Só /v1/chat/completions.",
        prefix: "AI_CLUSTER_SMOLLM2_INSTRUCT",
        default_subdir: "smollm2-instruct",
        instruct: true,
    },
];

/// `AI_CLUSTER_REQUIRE_MANIFEST=1`: a model directory without `manifest.json` is refused.
fn manifest_required() -> bool {
    std::env::var("AI_CLUSTER_REQUIRE_MANIFEST")
        .is_ok_and(|value| matches!(value.trim(), "1" | "true"))
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
}

/// Whether every node can hold what the model keeps resident on it: `need` against the slice
/// cache the node reported and, when it reported memory, against its RAM. Fail-closed: an
/// unavailable node, or one with no slice cache (an old worker, or one without a model root),
/// refuses the model instead of failing on the first request.
///
/// The error says whether waiting could help: a node that is not registered yet may still show up,
/// but a node whose cache is too small never fits.
fn check_fit(
    id: &str,
    need: &std::collections::BTreeMap<String, u64>,
    nodes: &[NodeShardCapacity],
) -> Result<(), (String, bool)> {
    for (node, bytes) in need {
        let Some(cap) = nodes.iter().find(|cap| cap.node_id == *node) else {
            let msg = format!("{id}: o controller não respondeu sobre o nó {node}");
            return Err((msg, true));
        };
        if !cap.available {
            let msg = format!("{id}: nó {node} não está registrado, aprovado e vivo");
            return Err((msg, true));
        }
        if cap.shard_cache_bytes == 0 {
            let msg = format!(
                "{id}: nó {node} não reporta cache de fatias (sem AI_CLUSTER_MODEL_DIR ou worker antigo)"
            );
            return Err((msg, false));
        }
        // 80% da RAM: o resto é do sistema, do worker e do pico de carga de uma fatia
        let room = if cap.memory_bytes > 0 {
            cap.shard_cache_bytes.min(cap.memory_bytes / 5 * 4)
        } else {
            cap.shard_cache_bytes
        };
        if *bytes > room {
            let msg = format!(
                "{id}: nó {node} precisa de {} de fatias e aceita {} (faltam {}); suba AI_CLUSTER_SHARD_CACHE_MB no worker ou acrescente nós ao *_SHARDS do modelo",
                gib(*bytes),
                gib(room),
                gib(bytes - room)
            );
            return Err((msg, false));
        }
    }
    Ok(())
}

/// Asks the controller what the shard nodes accept and refuses the model if they cannot hold it.
/// Nodes that are still registering are given up to 30 s, so the start order does not matter.
async fn admit(
    client: &MetadataClient,
    id: &str,
    need: &std::collections::BTreeMap<String, u64>,
) -> Result<(), String> {
    if need.is_empty() {
        return Ok(());
    }
    let node_ids: Vec<String> = need.keys().cloned().collect();
    let mut last = String::new();
    for attempt in 0..15 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let mut client = client.clone();
        match client
            .shard_capacity(ShardCapacityRequest {
                node_ids: node_ids.clone(),
            })
            .await
        {
            Ok(response) => match check_fit(id, need, &response.into_inner().nodes) {
                Ok(()) => return Ok(()),
                Err((err, false)) => return Err(err),
                Err((err, true)) => last = err,
            },
            Err(err) => last = format!("{id}: capacidade dos nós: {}", err.message()),
        }
    }
    Err(last)
}

fn load_llama(client: &MetadataClient, spec: &LlamaSpec) -> Result<Option<Llm>, String> {
    let Some(dir) = std::env::var_os(format!("{}_DIR", spec.prefix)) else {
        return Ok(None);
    };
    let dir = std::path::PathBuf::from(dir);
    let version =
        verified_version(&dir, manifest_required()).map_err(|err| format!("{}: {err}", spec.id))?;
    let shards = shard_nodes(&format!("{}_SHARDS", spec.prefix));
    let (model, remote): (Box<dyn LanguageModel + Send + Sync>, _) = if shards.is_empty() {
        (
            Box::new(Llama::load(&dir).map_err(|err| format!("{}: {err}", spec.id))?),
            None,
        )
    } else {
        let (executor, stats) = grpc_executor(
            client,
            &format!("{}_SUBDIR", spec.prefix),
            spec.default_subdir,
            shards.clone(),
        );
        (
            Box::new(
                Llama::load_remote(&dir, executor, shards.clone())
                    .map_err(|err| format!("{}: {err}", spec.id))?,
            ),
            Some(stats),
        )
    };
    let tokenizer =
        Tokenizer::open(&dir).map_err(|err| format!("{}: tokenizador: {err}", spec.id))?;
    let chat = if spec.instruct {
        Some(
            ChatMl::for_tokenizer(&tokenizer)
                .ok_or_else(|| format!("{}: o vocabulário não tem os tokens ChatML", spec.id))?,
        )
    } else {
        None
    };
    Ok(Some(Llm {
        id: spec.id,
        version,
        description: spec.description,
        model,
        end_of_text: chat.as_ref().map_or(0, |chat| chat.end),
        tokenizer,
        remote,
        shards,
        chat,
    }))
}

impl Executor for GrpcExecutor {
    fn matmul(&self, x: &Matrix, slices: &[Slice]) -> Result<Vec<Matrix>, String> {
        let started = std::time::Instant::now();
        let mut client = self.client.clone();
        let mut attempts = 0;
        let response = loop {
            let request = ShardMatmulRequest {
                rows: x.rows,
                k: x.cols,
                x: x.data.clone(),
                ops: self
                    .roster
                    .assign(slices)?
                    .into_iter()
                    .map(|s| ShardOp {
                        node_id: s.node,
                        model_subdir: self.subdir.clone(),
                        tensor: s.tensor,
                        row_start: s.row_start,
                        row_end: s.row_end,
                        by_columns: s.by_columns,
                    })
                    .collect(),
            };
            // Called from a blocking thread (generation runs in `spawn_blocking`), so blocking on
            // the runtime handle is safe.
            // Dropping the call when the generation is cancelled aborts the RPC, and the controller
            // forgets its operations: a timeout does not wait out a slow worker.
            if self.stats.cancelled() {
                return Err("fatias: geração cancelada".into());
            }
            let call = async {
                tokio::select! {
                    result = client.shard_matmul(request) => Some(result),
                    () = async {
                        while !self.stats.cancelled() {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    } => None,
                }
            };
            let Some(result) = self.runtime.block_on(call) else {
                return Err("fatias: geração cancelada".into());
            };
            match result {
                Ok(response) => break response.into_inner(),
                Err(status) => {
                    let blamed = status
                        .metadata()
                        .get(FAILED_NODE_HEADER)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string);
                    let retryable = matches!(
                        status.code(),
                        tonic::Code::Unavailable
                            | tonic::Code::DeadlineExceeded
                            | tonic::Code::FailedPrecondition
                    );
                    match blamed {
                        // Every retry benches one more node, so this ends after the last of them.
                        Some(node) if retryable && attempts < self.roster.nodes.len() => {
                            eprintln!(
                                "worker {node} falhou ({:?}); refazendo nos demais",
                                status.message()
                            );
                            self.roster.bench(&node);
                            self.stats.retries.fetch_add(1, Ordering::Relaxed);
                            attempts += 1;
                        }
                        _ => return Err(format!("fatias: {}", status.message())),
                    }
                }
            }
        };
        self.stats.calls.fetch_add(1, Ordering::Relaxed);
        self.stats
            .wall_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        self.stats.compute_us.fetch_add(
            response
                .outputs
                .iter()
                .map(|o| o.compute_us)
                .max()
                .unwrap_or(0),
            Ordering::Relaxed,
        );
        Ok(response
            .outputs
            .into_iter()
            .map(|o| Matrix {
                rows: o.rows,
                cols: o.cols,
                data: o.data,
            })
            .collect())
    }
}

#[derive(Clone)]
struct Gateway {
    /// Operator connection, for the synthetic embeddings graph; absent when the gateway was
    /// given only the inference token.
    client: Option<MetadataClient>,
    tenants: Arc<Tenants>,
    cache: Arc<cache::ResponseCache>,
    slots: Arc<Semaphore>,
    llms: Arc<Vec<Arc<Llm>>>,
    /// Modelo servido pelo llama-server (ADR-022); `None` sem `AI_CLUSTER_LLAMA_MODEL`.
    llama: Arc<llama::Manager>,
    /// A fila do llama-server: tantas gerações ao mesmo tempo quantas as vagas (`-np`).
    llama_admission: Arc<Admission>,
    /// One generation at a time across all models, handed out by priority class: it is CPU-bound
    /// and every thread is already used by the matmul.
    admission: Arc<Admission>,
}

/// The external error model (`inference-api` §13): a stable category and a short message. The cause
/// stays in the gateway's own log.
fn error(status: StatusCode, kind: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "type": kind, "message": message } })),
    )
        .into_response()
}

fn unauthorized() -> Response {
    error(
        StatusCode::UNAUTHORIZED,
        "AUTHENTICATION_FAILED",
        "chave de API ausente ou inválida",
    )
}

/// FNV-1a, 64 bits: the seed of a request. Written out because `DefaultHasher` makes no promise
/// across Rust versions, and the same input must give the same vector tomorrow.
fn seed_of(input: &str) -> u64 {
    input.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

async fn health() -> impl IntoResponse {
    // Never touches the cluster: the API's health is not the cluster's (`inference-api` §21).
    Json(json!({ "api": "green" }))
}

/// Generation metrics in Prometheus text, behind the same bearer key as the API (F05).
async fn inference_metrics(State(gateway): State<Gateway>, headers: HeaderMap) -> Response {
    let Some(tenant) = gateway.tenants.authenticate(&headers) else {
        return unauthorized();
    };
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        metrics::render() + &gateway.cache.render() + &gateway.tenants.render(&tenant),
    )
        .into_response()
}

async fn models(State(gateway): State<Gateway>, headers: HeaderMap) -> Response {
    let Some(tenant) = gateway.tenants.authenticate(&headers) else {
        return unauthorized();
    };
    let mut data = Vec::new();
    if tenant.allows(MODEL_ID) {
        data.push(json!({
            "id": MODEL_ID,
            "object": "model",
            "owned_by": "ai-cluster",
            "kind": "synthetic-matmul",
            "description": "Forward pass sintético (grafo de multiplicação de matrizes) executado no \
                cluster. Não é um modelo de linguagem e o vetor não tem semântica.",
        }));
    }
    if let Some(llama) = gateway.llama.current().filter(|l| tenant.allows(&l.id)) {
        data.push(json!({
            "id": llama.id,
            "object": "model",
            "owned_by": "ai-cluster",
            "kind": "language-model",
            "engine": "llama.cpp",
            "context": llama.context,
            "state": match llama.state() {
                llama::State::Loading => "loading",
                llama::State::Ready => "ready",
                llama::State::Failed(_) => "failed",
            },
            "tokens_per_second": llama.tokens_per_second(),
        }));
    }
    for llm in gateway.llms.iter().filter(|llm| tenant.allows(llm.id)) {
        data.push(json!({
            "id": llm.id,
            "object": "model",
            "owned_by": "ai-cluster",
            "kind": "language-model",
            "version": llm.version,
            "description": llm.description,
        }));
    }
    Json(json!({ "object": "list", "data": data })).into_response()
}

#[derive(Deserialize)]
struct EmbeddingRequest {
    model: String,
    input: String,
}

/// Why a run did not produce a vector.
enum Failure {
    /// The graph did not finish inside `DEADLINE` — usually no worker is up.
    Timeout,
    /// A unit failed, or its result is gone or unusable.
    Failed,
    /// The controller's queue is full.
    Busy,
    /// The controller could not be reached or refused the call.
    Internal(String),
}

fn internal(err: tonic::Status) -> Failure {
    if err.code() == tonic::Code::ResourceExhausted {
        Failure::Busy
    } else {
        Failure::Internal(err.to_string())
    }
}

/// Runs the diamond graph for `seed` on the cluster and returns the request id and the first
/// row of the final unit's output.
///
/// ponytail: a request that times out leaves its graph in the controller's queue — there is no
/// cancellation yet (`request-manager` §8). The concurrency cap bounds how fast that can pile up.
/// Add a cancel RPC to the controller when abandoned graphs start to cost real capacity.
async fn run_graph(
    mut client: MetadataClient,
    seed: u64,
    priority: u32,
) -> Result<(String, Vec<f64>), Failure> {
    let request_id = client
        .enqueue_graph(EnqueueGraphRequest {
            size: DIMENSIONS,
            seed,
            request_id: String::new(),
            keep_result: true,
            priority,
        })
        .await
        .map_err(internal)?
        .into_inner()
        .request_id;
    // However this ends (timeout, a failed poll, the client leaving and the future being dropped),
    // what the graph has not started is taken off the controller's queue.
    let mut abandon = AbandonGraph {
        client: client.clone(),
        request_id: request_id.clone(),
        armed: true,
    };

    let deadline = Instant::now() + DEADLINE;
    loop {
        let trace = client
            .get_trace(GetTraceRequest {
                request_id: request_id.clone(),
            })
            .await
            .map_err(internal)?
            .into_inner();
        if trace.units.iter().any(|unit| unit.status == "failed") {
            return Err(Failure::Failed);
        }
        if trace.units.len() == GRAPH_UNITS
            && trace.units.iter().all(|unit| unit.status == "completed")
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err(Failure::Timeout);
        }
        tokio::time::sleep(POLL).await;
    }

    let output = client
        .get_output(GetOutputRequest {
            request_id: request_id.clone(),
        })
        .await
        .map_err(internal)?
        .into_inner();
    if !output.found || output.matrix_c.len() < DIMENSIONS as usize {
        return Err(Failure::Failed);
    }
    let embedding = output.matrix_c[..DIMENSIONS as usize].to_vec();
    // JSON has no NaN or infinity: `json!` would turn them into null and answer 200 with a hole.
    if !embedding.iter().all(|value| value.is_finite()) {
        return Err(Failure::Failed);
    }
    abandon.armed = false;
    Ok((request_id, embedding))
}

/// Asks the controller to cancel a graph nobody is waiting for any more, unless disarmed.
struct AbandonGraph {
    client: MetadataClient,
    request_id: String,
    armed: bool,
}

impl Drop for AbandonGraph {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut client = self.client.clone();
        let request_id = std::mem::take(&mut self.request_id);
        tokio::spawn(async move {
            let _ = client
                .cancel_request(CancelRequestRequest { request_id })
                .await;
        });
    }
}

async fn embeddings(
    State(gateway): State<Gateway>,
    headers: HeaderMap,
    body: Result<Json<EmbeddingRequest>, JsonRejection>,
) -> Response {
    let Some(tenant) = gateway.tenants.authenticate(&headers) else {
        return unauthorized();
    };
    let request = match body {
        Ok(Json(request)) => request,
        // Keeps the rejection's own status: 413 past the body limit, 415 for a wrong type.
        Err(rejection) => {
            return error(
                rejection.status(),
                "INVALID_REQUEST",
                "corpo inválido: esperado JSON com `model` e `input` (texto)",
            );
        }
    };
    if request.model != MODEL_ID {
        return error(
            StatusCode::NOT_FOUND,
            "MODEL_NOT_FOUND",
            "modelo desconhecido; veja GET /v1/models",
        );
    }
    if !tenant.allows(MODEL_ID) {
        return model_forbidden();
    }
    if request.input.is_empty() || request.input.len() > MAX_INPUT_BYTES {
        return error(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "`input` deve ter de 1 a 8192 bytes",
        );
    }
    if let Err(retry) = tenant.check_quota(gateway.tenants.window) {
        return quota_exceeded(retry);
    }
    // Backpressure with an explicit policy (`inference-api` §22): reject and say when to retry.
    let Ok(_slot) = gateway.slots.clone().try_acquire_owned() else {
        tenant.note_rejected();
        let mut response = error(
            StatusCode::TOO_MANY_REQUESTS,
            "CAPACITY_UNAVAILABLE",
            "capacidade esgotada; tente de novo em instantes",
        );
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, "1".parse().expect("ascii"));
        return response;
    };

    let Some(client) = gateway.client.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "EMBEDDINGS_DISABLED",
            "embeddings exigem o token de operador, que este gateway não tem",
        );
    };
    let priority = tenant.class_for(requested_class(&headers)) as u32;
    match run_graph(client, seed_of(&request.input), priority).await {
        Ok((request_id, embedding)) => {
            // Roughly four bytes to a token, at least one: embeddings spend the quota too.
            tenant.charge(request.input.len().div_ceil(4).max(1) as u64);
            (
            [
                ("x-request-id", request_id.clone()),
                ("x-model-id", MODEL_ID.to_string()),
            ],
            Json(json!({
                "object": "list",
                "model": MODEL_ID,
                "request_id": request_id,
                "synthetic": true,
                "note": "vetor sem semântica: forward pass sintético de multiplicação de matrizes",
                "data": [{ "object": "embedding", "index": 0, "embedding": embedding }],
                "usage": { "prompt_tokens": 0, "total_tokens": 0 },
            })),
        )
            .into_response()
        }
        Err(Failure::Timeout) => error(
            StatusCode::GATEWAY_TIMEOUT,
            "TIMEOUT",
            "o cluster não terminou a tempo",
        ),
        Err(Failure::Busy) => {
            let mut response = error(
                StatusCode::TOO_MANY_REQUESTS,
                "CAPACITY_UNAVAILABLE",
                "capacidade esgotada; tente de novo em instantes",
            );
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, "5".parse().expect("ascii"));
            response
        }
        Err(Failure::Failed) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MODEL_UNAVAILABLE",
            "a execução falhou no cluster",
        ),
        Err(Failure::Internal(cause)) => {
            eprintln!("erro falando com o controller: {cause}");
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "SERVICE_UNAVAILABLE",
                "controller indisponível",
            )
        }
    }
}

#[derive(Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: Option<u32>,
    #[serde(default)]
    stream: bool,
    /// 0 (o padrão) é guloso e determinístico; até 2. Acima de 0 a resposta é sorteada.
    temperature: Option<f64>,
    /// Massa de probabilidade mantida no sorteio, em (0, 1].
    top_p: Option<f64>,
    /// Semente do sorteio; a mesma semente repete a resposta. Sem ela, cada pedido sorteia a sua.
    seed: Option<u64>,
    /// Quantas respostas alternativas, de 1 a [`MAX_CHOICES`]; só sem `stream`.
    n: Option<u32>,
    stream_options: Option<StreamOptions>,
}

#[derive(Deserialize)]
struct StreamOptions {
    /// Manda um último pedaço com o `usage` antes do `[DONE]`.
    #[serde(default)]
    include_usage: bool,
}

/// Teto de `n`: cada resposta é uma geração inteira no único slot.
const MAX_CHOICES: u32 = 4;

/// Semente para o pedido que não trouxe uma: o relógio com um contador, para dois pedidos no mesmo
/// instante divergirem.
fn fresh_seed() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    nanos
        ^ COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn invalid(message: &str) -> Response {
    error(StatusCode::BAD_REQUEST, "INVALID_REQUEST", message)
}

/// A request that passed validation and holds the one generation slot.
struct Prepared {
    llm: Arc<Llm>,
    prompt: Vec<u32>,
    new_tokens: usize,
    stream: bool,
    sampling: Sampling,
    /// Quantas respostas (`n`); 1 quando em `stream`.
    choices: usize,
    include_usage: bool,
    /// `None` when the answer comes from the cache and no generation runs.
    slot: Option<Permit>,
    tenant: Arc<Tenant>,
    cache: Arc<cache::ResponseCache>,
    cached: Option<Vec<u32>>,
}

/// Runs one generation inside the blocking thread, or replays a cached one, and does the
/// bookkeeping both need: the tenant is charged for prompt plus visible tokens, the sampler
/// records a real generation, and a finished real generation goes into the cache. `on_token`
/// sees every token, the stop token included, as `generate_with` hands them out.
fn generate_or_replay(
    prepared: &Prepared,
    worker: &Llm,
    flag: &AtomicBool,
    sampling: &Sampling,
    on_token: &mut dyn FnMut(u32),
) -> Result<Vec<u32>, String> {
    let stop = worker.end_of_text;
    let prompt = &prepared.prompt;
    if let Some(tokens) = &prepared.cached {
        let visible = tokens.iter().filter(|&&token| token != stop).count();
        for &token in tokens {
            on_token(token);
        }
        prepared.tenant.charge((prompt.len() + visible) as u64);
        return Ok(tokens.clone());
    }
    let mut sampler = metrics::Sampler::start();
    let generated = worker.model.generate_sampled(
        prompt,
        prepared.new_tokens,
        Some(stop),
        flag,
        sampling,
        &mut |token| {
            if token != stop {
                sampler.token();
            }
            on_token(token);
        },
    );
    let stopped = generated
        .as_ref()
        .is_ok_and(|tokens| tokens.last() == Some(&stop));
    prepared
        .tenant
        .charge((prompt.len() + sampler.tokens()) as u64);
    sampler.finish(
        generated.is_ok(),
        prompt.len(),
        stopped,
        worker.model.kv_bytes_per_position(),
    );
    // Só a geração gulosa é reproduzível; uma sorteada não vai ao cache.
    if sampling.is_greedy()
        && let Ok(tokens) = &generated
    {
        prepared.cache.put(
            &prepared.tenant.name,
            worker.id,
            &worker.version,
            prompt,
            prepared.new_tokens,
            tokens,
        );
    }
    generated
}

/// The class a call asks for in `x-priority`, if it names one.
fn requested_class(headers: &HeaderMap) -> Option<Class> {
    headers
        .get("x-priority")
        .and_then(|value| value.to_str().ok())
        .and_then(Class::parse)
}

fn model_forbidden() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "MODEL_FORBIDDEN",
        "esta chave não tem acesso ao modelo; veja GET /v1/models",
    )
}

fn quota_exceeded(retry_after_secs: u64) -> Response {
    let mut response = error(
        StatusCode::TOO_MANY_REQUESTS,
        "QUOTA_EXCEEDED",
        "a cota de tokens desta chave acabou; tente de novo quando a janela renovar",
    );
    response.headers_mut().insert(
        header::RETRY_AFTER,
        retry_after_secs.to_string().parse().expect("ascii"),
    );
    response
}

static NEXT_CHAT_ID: AtomicU64 = AtomicU64::new(1);

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Authentication, validation, the generation slot and the prompt. Instruct models get their
/// ChatML turns; a base model was never trained on chat turns, so the message contents are
/// joined by newlines and it continues the text. The reply is then a continuation, not an answer.
// The error is the HTTP response itself, sent straight back; boxing it would only add noise.
#[allow(clippy::result_large_err)]
async fn prepare(
    gateway: &Gateway,
    headers: &HeaderMap,
    body: Result<Json<ChatRequest>, JsonRejection>,
) -> Result<Prepared, Response> {
    let Some(tenant) = gateway.tenants.authenticate(headers) else {
        return Err(unauthorized());
    };
    let request = match body {
        Ok(Json(request)) => request,
        Err(rejection) => {
            return Err(error(
                rejection.status(),
                "INVALID_REQUEST",
                "corpo inválido: esperado JSON com `model` e `messages` (role e content)",
            ));
        }
    };
    let Some(llm) = gateway
        .llms
        .iter()
        .find(|llm| llm.id == request.model)
        .cloned()
    else {
        return Err(error(
            StatusCode::NOT_FOUND,
            "MODEL_NOT_FOUND",
            "modelo desconhecido; veja GET /v1/models",
        ));
    };
    if !tenant.allows(llm.id) {
        return Err(model_forbidden());
    }
    let total: usize = request.messages.iter().map(|m| m.content.len()).sum();
    if request.messages.is_empty()
        || request.messages.len() > MAX_MESSAGES
        || total == 0
        || total > MAX_INPUT_BYTES
        || request
            .messages
            .iter()
            .any(|m| !matches!(m.role.as_str(), "system" | "user" | "assistant"))
    {
        return Err(invalid(
            "`messages` deve ter de 1 a 32 mensagens (role system, user ou assistant) e de 1 a 8192 bytes de texto",
        ));
    }
    let new_tokens = request
        .max_tokens
        .unwrap_or(DEFAULT_NEW_TOKENS)
        .clamp(1, MAX_NEW_TOKENS) as usize;
    let temperature = request.temperature.unwrap_or(0.0);
    let top_p = request.top_p.unwrap_or(1.0);
    let choices = request.n.unwrap_or(1);
    if !temperature.is_finite()
        || !(0.0..=2.0).contains(&temperature)
        || !top_p.is_finite()
        || !(top_p > 0.0 && top_p <= 1.0)
        || !(1..=MAX_CHOICES).contains(&choices)
        || (request.stream && choices != 1)
    {
        return Err(invalid(
            "`temperature` deve estar em [0, 2], `top_p` em (0, 1] e `n` de 1 a 4 (só 1 com `stream`)",
        ));
    }
    let sampling = Sampling {
        temperature,
        top_p,
        seed: request.seed.unwrap_or_else(fresh_seed),
    };
    if let Err(retry) = tenant.check_quota(gateway.tenants.window) {
        return Err(quota_exceeded(retry));
    }
    let built = match &llm.chat {
        Some(chat) => {
            let turns: Vec<(&str, &str)> = request
                .messages
                .iter()
                .map(|m| (m.role.as_str(), m.content.as_str()))
                .collect();
            chat.prompt(&llm.tokenizer, &turns)
        }
        None => {
            let text = request
                .messages
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            llm.tokenizer.encode(&text)
        }
    };
    let prompt = match built {
        Ok(prompt) if !prompt.is_empty() && prompt.len() <= MAX_PROMPT_TOKENS => prompt,
        _ => return Err(invalid("o texto deve ter de 1 a 512 tokens")),
    };
    // A request this tenant already had answered is served from memory and never queues for the
    // generation slot. The input is capped at MAX_INPUT_BYTES above, so tokenizing it first is cheap.
    // Só o guloso é repetível: uma resposta sorteada nem lê nem grava o cache. Com `n` > 1 as
    // respostas são independentes, então também não.
    let cached = if sampling.is_greedy() && choices == 1 {
        gateway
            .cache
            .get(&tenant.name, llm.id, &llm.version, &prompt, new_tokens)
    } else {
        None
    };
    let slot = if cached.is_some() {
        None
    } else {
        // A request waits in line by class, up to the gateway's limit, and is refused past it.
        let class = tenant.class_for(requested_class(headers));
        let Ok(slot) = gateway.admission.acquire(class).await else {
            tenant.note_rejected();
            let mut response = error(
                StatusCode::TOO_MANY_REQUESTS,
                "CAPACITY_UNAVAILABLE",
                "capacidade esgotada; tente de novo em instantes",
            );
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, "5".parse().expect("ascii"));
            return Err(response);
        };
        // Checked again now that the wait is over: a burst that queued behind an empty quota would
        // otherwise all be let through, each spending up to a full generation.
        if let Err(retry) = tenant.check_quota(gateway.tenants.window) {
            return Err(quota_exceeded(retry));
        }
        Some(slot)
    };
    Ok(Prepared {
        llm,
        prompt,
        new_tokens,
        stream: request.stream,
        sampling,
        choices: choices as usize,
        include_usage: request
            .stream_options
            .is_some_and(|options| options.include_usage),
        slot,
        tenant,
        cache: gateway.cache.clone(),
        cached,
    })
}

/// Logs where the time of a remote generation went; no-op for a local model.
fn log_remote(llm: &Llm, tokens: usize) {
    if let Some(stats) = &llm.remote {
        let calls = stats.calls.swap(0, Ordering::Relaxed);
        let wall = stats.wall_us.swap(0, Ordering::Relaxed);
        let compute = stats.compute_us.swap(0, Ordering::Relaxed);
        let retries = stats.retries.swap(0, Ordering::Relaxed);
        eprintln!(
            "remoto: {tokens} tokens, {calls} chamadas, {} ms nas chamadas ({} us cada), {} ms de cálculo nos workers, {retries} refeitas",
            wall / 1000,
            wall.checked_div(calls).unwrap_or(0),
            compute / 1000
        );
    }
}

async fn chat_completions(
    State(gateway): State<Gateway>,
    headers: HeaderMap,
    body: Result<Json<ChatRequest>, JsonRejection>,
) -> Response {
    match prepare(&gateway, &headers, body).await {
        Ok(prepared) if prepared.stream => stream_chat(prepared),
        Ok(prepared) => complete_chat(prepared).await,
        Err(response) => response,
    }
}

/// Antes do caminho do motor próprio: um pedido para o modelo do llama-server segue por aqui, com a
/// mesma autenticação, cota e fila, e o corpo vai inteiro ao servidor (chamada de ferramentas incluída).
async fn llama_route(
    State(gateway): State<Gateway>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(llama) = gateway.llama.current() else {
        return next.run(request).await;
    };
    let (parts, body) = request.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, BODY_LIMIT).await else {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "INVALID_REQUEST",
            "corpo grande demais",
        );
    };
    let model = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| v["model"].as_str().map(str::to_owned));
    if model.as_deref() != Some(llama.id.as_str()) {
        return next
            .run(axum::extract::Request::from_parts(
                parts,
                axum::body::Body::from(bytes),
            ))
            .await;
    }
    llama_chat(&gateway, llama, parts.headers, bytes).await
}

async fn llama_chat(
    gateway: &Gateway,
    llama: Arc<llama::Llama>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(tenant) = gateway.tenants.authenticate(&headers) else {
        return unauthorized();
    };
    if !tenant.allows(&llama.id) {
        return model_forbidden();
    }
    match llama.state() {
        llama::State::Ready => {}
        llama::State::Loading => {
            let mut response = error(
                StatusCode::SERVICE_UNAVAILABLE,
                "MODEL_LOADING",
                "o modelo está carregando; tente de novo em instantes",
            );
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, "10".parse().expect("ascii"));
            return response;
        }
        llama::State::Failed(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "MODEL_UNAVAILABLE",
                "o modelo não está disponível",
            );
        }
    }
    if let Err(retry) = tenant.check_quota(gateway.tenants.window) {
        return quota_exceeded(retry);
    }
    let class = tenant.class_for(requested_class(&headers));
    let Ok(permit) = gateway.llama_admission.acquire(class).await else {
        tenant.note_rejected();
        let mut response = error(
            StatusCode::TOO_MANY_REQUESTS,
            "CAPACITY_UNAVAILABLE",
            "capacidade esgotada; tente de novo em instantes",
        );
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, "5".parse().expect("ascii"));
        return response;
    };
    if let Err(retry) = tenant.check_quota(gateway.tenants.window) {
        return quota_exceeded(retry);
    }
    llama.forward(body, tenant, permit).await
}

/// Sets the flag when dropped, whether the request finished or its future was dropped.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

async fn complete_chat(mut prepared: Prepared) -> Response {
    let llm = prepared.llm.clone();
    let prompt_tokens = prepared.prompt.len();
    // The permit moves into the blocking task, so the slot frees only when the generation really
    // ends. A timeout sets `cancel`, which stops it at the next token.
    let cancel = Arc::new(AtomicBool::new(false));
    // If the client disconnects, axum drops this future: the guard still cancels the generation.
    let _cancel_on_drop = CancelOnDrop(cancel.clone());
    let worker = llm.clone();
    let flag = cancel.clone();
    let task = tokio::task::spawn_blocking(move || {
        let _slot = prepared.slot.take();
        bind_cancel(&worker, &flag);
        // `n` respostas, uma geração cada, no mesmo slot; a semente anda um passo por resposta.
        // Guloso dá sempre a mesma resposta: gera uma vez e repete.
        let runs = if prepared.sampling.is_greedy() {
            1
        } else {
            prepared.choices
        };
        let mut all = (0..runs)
            .map(|index| {
                let sampling = Sampling {
                    seed: prepared.sampling.seed.wrapping_add(index as u64),
                    ..prepared.sampling
                };
                generate_or_replay(&prepared, &worker, &flag, &sampling, &mut |_| {})
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(first) = all.first().cloned() {
            all.resize(prepared.choices, first);
        }
        Ok::<_, String>(all)
    });
    let all = match tokio::time::timeout(DEADLINE, task).await {
        Ok(Ok(Ok(all))) => all,
        Err(_) => {
            cancel.store(true, Ordering::Relaxed);
            log_remote(&llm, 0);
            return error(
                StatusCode::GATEWAY_TIMEOUT,
                "TIMEOUT",
                "a geração não terminou a tempo",
            );
        }
        Ok(failed) => {
            eprintln!("geração falhou: {failed:?}");
            log_remote(&llm, 0);
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "MODEL_UNAVAILABLE",
                "a geração falhou",
            );
        }
    };
    let mut choices = Vec::with_capacity(all.len());
    let mut completion_tokens = 0;
    for (index, generated) in all.iter().enumerate() {
        log_remote(&llm, generated.len());
        let stopped = generated.last() == Some(&llm.end_of_text);
        let visible = &generated[..generated.len() - usize::from(stopped)];
        let Ok(content) = llm.tokenizer.decode(visible) else {
            eprintln!("decodificação falhou para {visible:?}");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "MODEL_UNAVAILABLE",
                "a geração falhou",
            );
        };
        // O token de fim também foi gerado: conta no uso, como na API de referência.
        completion_tokens += generated.len();
        choices.push(json!({
            "index": index,
            "message": { "role": "assistant", "content": content },
            "finish_reason": if stopped { "stop" } else { "length" },
        }));
    }
    let id = format!("chatcmpl-{}", NEXT_CHAT_ID.fetch_add(1, Ordering::Relaxed));
    (
        [
            ("x-request-id", id.clone()),
            ("x-model-id", llm.id.to_string()),
            ("x-model-version", llm.version.clone().unwrap_or_default()),
        ],
        Json(json!({
            "id": id,
            "object": "chat.completion",
            "created": unix_seconds(),
            "model": llm.id,
            "choices": choices,
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens,
            },
        })),
    )
        .into_response()
}

/// The part of `full` not yet sent, advancing `sent`. A text that ends in U+FFFD may be a
/// character whose last bytes have not been generated yet, so it is held back until `done`.
fn new_text<'a>(full: &'a str, sent: &mut usize, done: bool) -> &'a str {
    let ready = if done {
        full
    } else {
        full.strip_suffix('\u{FFFD}').unwrap_or(full)
    };
    let fresh = ready.get(*sent..).unwrap_or("");
    *sent = (*sent).max(ready.len());
    fresh
}

/// How long a streaming client may leave its buffer full before the gateway gives up on it. A
/// reader that stops reading would otherwise hold the only generation slot forever.
const STREAM_STALL: Duration = Duration::from_secs(10);

type EventTx = tokio::sync::mpsc::Sender<Result<Bytes, Infallible>>;

/// Queues `item` from the blocking generation thread; false if the client left or stalled.
fn push_blocking(tx: &EventTx, mut item: Result<Bytes, Infallible>) -> bool {
    let started = std::time::Instant::now();
    loop {
        match tx.try_send(item) {
            Ok(()) => return true,
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => return false,
            Err(tokio::sync::mpsc::error::TrySendError::Full(back)) => {
                if started.elapsed() > STREAM_STALL {
                    return false;
                }
                item = back;
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// Queues `item` from async code, giving up after [`STREAM_STALL`].
async fn put(tx: &EventTx, item: Result<Bytes, Infallible>) {
    let _ = tokio::time::timeout(STREAM_STALL, tx.send(item)).await;
}

/// `stream: true`: server-sent events in the OpenAI shape, one `chat.completion.chunk` per piece
/// of text, then `[DONE]`. The headers go out at once, so a failure after that arrives as an
/// `error` event instead of a status code.
fn stream_chat(mut prepared: Prepared) -> Response {
    let llm = prepared.llm.clone();
    let (prompt_tokens, include_usage) = (prepared.prompt.len(), prepared.include_usage);
    let id = format!("chatcmpl-{}", NEXT_CHAT_ID.fetch_add(1, Ordering::Relaxed));
    let created = unix_seconds();
    let model_id = llm.id;
    let model_version = llm.version.clone();
    let event = {
        let (id, model) = (id.clone(), llm.id);
        move |delta: serde_json::Value, finish: serde_json::Value| -> Result<Bytes, Infallible> {
            Ok(format!(
                "data: {}\n\n",
                json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": model,
                    "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }],
                })
            )
            .into())
        }
    };
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let cancel = Arc::new(AtomicBool::new(false));
    let (worker, flag, out, content) = (llm.clone(), cancel.clone(), tx.clone(), event.clone());
    let task = tokio::task::spawn_blocking(move || {
        let _slot = prepared.slot.take();
        bind_cancel(&worker, &flag);
        let stop = worker.end_of_text;
        let (mut tokens, mut sent) = (Vec::new(), 0);
        let send = |text: &str| {
            if !text.is_empty()
                && !push_blocking(&out, content(json!({ "content": text }), json!(null)))
            {
                // The client went away or stopped reading: stop generating and free the slot.
                flag.store(true, Ordering::Relaxed);
            }
        };
        let sampling = prepared.sampling;
        let generated = generate_or_replay(&prepared, &worker, &flag, &sampling, &mut |token| {
            if token == stop {
                return;
            }
            tokens.push(token);
            if let Ok(text) = worker.tokenizer.decode(&tokens) {
                send(new_text(&text, &mut sent, false));
            }
        });
        // Whatever was held back waiting for the rest of a character.
        if let Ok(text) = worker.tokenizer.decode(&tokens) {
            send(new_text(&text, &mut sent, true));
        }
        generated
    });
    let usage_id = id.clone();
    tokio::spawn(async move {
        put(&tx, event(json!({ "role": "assistant" }), json!(null))).await;
        let failure = match tokio::time::timeout(DEADLINE, task).await {
            Ok(Ok(Ok(generated))) => {
                log_remote(&llm, generated.len());
                let stopped = generated.last() == Some(&llm.end_of_text);
                put(
                    &tx,
                    event(json!({}), json!(if stopped { "stop" } else { "length" })),
                )
                .await;
                if include_usage {
                    // Último pedaço, sem `choices`, só com o uso (como `stream_options.include_usage`).
                    let usage = json!({
                        "id": usage_id,
                        "object": "chat.completion.chunk",
                        "created": created,
                        "model": llm.id,
                        "choices": [],
                        "usage": {
                            "prompt_tokens": prompt_tokens,
                            "completion_tokens": generated.len(),
                            "total_tokens": prompt_tokens + generated.len(),
                        },
                    });
                    put(
                        &tx,
                        Ok(format!(
                            "data: {usage}

"
                        )
                        .into()),
                    )
                    .await;
                }
                None
            }
            Err(_) => {
                cancel.store(true, Ordering::Relaxed);
                log_remote(&llm, 0);
                Some(("TIMEOUT", "a geração não terminou a tempo"))
            }
            // Cancelled because the client left: there is nobody to tell.
            Ok(Ok(Err(_))) if tx.is_closed() => None,
            Ok(failed) => {
                eprintln!("geração falhou: {failed:?}");
                log_remote(&llm, 0);
                Some(("MODEL_UNAVAILABLE", "a geração falhou"))
            }
        };
        if let Some((kind, message)) = failure {
            let body = json!({ "error": { "type": kind, "message": message } });
            put(&tx, Ok(format!("data: {body}\n\n").into())).await;
        }
        put(&tx, Ok("data: [DONE]\n\n".into())).await;
    });
    let mut response = Response::new(Body::from_stream(
        tonic::codegen::tokio_stream::wrappers::ReceiverStream::new(rx),
    ));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        "text/event-stream".parse().expect("ascii"),
    );
    headers.insert(header::CACHE_CONTROL, "no-cache".parse().expect("ascii"));
    headers.insert("x-request-id", id.parse().expect("ascii"));
    headers.insert("x-model-id", model_id.parse().expect("ascii"));
    if let Some(version) = model_version.and_then(|version| version.parse().ok()) {
        headers.insert("x-model-version", version);
    }
    response
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Who may call: the single `AI_CLUSTER_API_KEY` (tenant "default", every model, no quota) and/or
    // the tenants of `AI_CLUSTER_TENANTS` (F11): `name=key[,models=a|b][,tokens=N][,class=c];...`.
    let mut list = Vec::new();
    if let Some(key) = ai_cluster_protocol::metadata::secret("AI_CLUSTER_API_KEY") {
        list.push(Tenant::new("default", &key));
    }
    if let Ok(spec) = std::env::var("AI_CLUSTER_TENANTS") {
        list.extend(request_manager::parse_tenants(&spec)?);
    }
    if list.is_empty() {
        return Err(
            "AI_CLUSTER_API_KEY ou AI_CLUSTER_TENANTS é obrigatório; o gateway não sobe sem chave"
                .into(),
        );
    }
    // The keys face applications; the join and operator tokens face the cluster. Reusing one
    // would let an application's key act as a node or as the operator.
    let cluster_tokens = [join_token(), operator_token(), inference_token()];
    for (index, tenant) in list.iter().enumerate() {
        if cluster_tokens
            .iter()
            .any(|token| token.as_deref() == Some(tenant.key()))
        {
            return Err(format!(
                "a chave de '{}' deve ser diferente de {JOIN_TOKEN_ENV}, {OPERATOR_TOKEN_ENV} e {INFERENCE_TOKEN_ENV}",
                tenant.name
            )
            .into());
        }
        if list[..index]
            .iter()
            .any(|other| other.key() == tenant.key())
        {
            return Err(format!("a chave de '{}' repete a de outro tenant", tenant.name).into());
        }
    }
    let env_number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(default)
    };
    let tenants = Arc::new(Tenants::new(
        list,
        Duration::from_secs(env_number("AI_CLUSTER_QUOTA_WINDOW_SECS", 3600)),
    ));
    let admission = Admission::new(
        env_number("AI_CLUSTER_QUEUE_MAX", 8) as usize,
        Duration::from_secs(env_number("AI_CLUSTER_QUEUE_WAIT_SECS", 10)),
    );
    // The operator token runs the synthetic embeddings graph; the inference token is enough for
    // the language models. With both, the models still use the narrower one.
    if operator_token().is_none() && inference_token().is_none() {
        return Err(format!(
            "{OPERATOR_TOKEN_ENV} (embeddings) ou {INFERENCE_TOKEN_ENV} (modelos) é obrigatório"
        )
        .into());
    }

    let addr: SocketAddr = std::env::var("AI_CLUSTER_GATEWAY_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()?;
    // Com certificado e chave (PEM) a perna HTTP é HTTPS. Sem eles a chave de API cruzaria a rede
    // em claro: só em loopback, a menos que o operador aceite isso explicitamente.
    let tls = match (
        std::env::var("AI_CLUSTER_GATEWAY_TLS_CERT"),
        std::env::var("AI_CLUSTER_GATEWAY_TLS_KEY"),
    ) {
        (Ok(cert), Ok(key)) => Some(tls::load_config(&cert, &key)?),
        (Err(_), Err(_)) => None,
        _ => {
            return Err(
                "AI_CLUSTER_GATEWAY_TLS_CERT e AI_CLUSTER_GATEWAY_TLS_KEY vêm juntos".into(),
            );
        }
    };
    if tls.is_none() && !addr.ip().is_loopback() && !insecure_allowed() {
        return Err(format!(
            "{addr} não é loopback e a perna HTTP não tem TLS; configure AI_CLUSTER_GATEWAY_TLS_CERT/KEY ou use {INSECURE_ENV}=1 numa rede fechada"
        )
        .into());
    }

    let controller = std::env::var("AI_CLUSTER_CONTROLLER")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".to_string());
    let operator_client = if operator_token().is_some() {
        Some(connect_operator(controller.clone()).await?)
    } else {
        None
    };
    let client = if inference_token().is_some() {
        connect_inference(controller.clone()).await?
    } else {
        operator_client
            .clone()
            .ok_or("sem token para falar com o controller")?
    };
    // Optional: without a model directory the gateway serves only the synthetic model. Each
    // model that is configured loads at startup, and a broken directory stops the gateway.
    let mut llms = Vec::new();
    if let Some(dir) = std::env::var_os("AI_CLUSTER_GPT2_DIR") {
        let dir = std::path::PathBuf::from(dir);
        let version =
            verified_version(&dir, manifest_required()).map_err(|err| format!("gpt2: {err}"))?;
        // AI_CLUSTER_GPT2_SHARDS: see the SmolLM2 block below; GPT-2's slices are column bands.
        let shards = shard_nodes("AI_CLUSTER_GPT2_SHARDS");
        let shards_for_report = shards.clone();
        let (model, remote): (Box<dyn LanguageModel + Send + Sync>, _) = if shards.is_empty() {
            (
                Box::new(Gpt2::load(&dir).map_err(|err| format!("gpt2: {err}"))?),
                None,
            )
        } else {
            let (executor, stats) =
                grpc_executor(&client, "AI_CLUSTER_GPT2_SUBDIR", "gpt2", shards.clone());
            (
                Box::new(
                    Gpt2::load_remote(&dir, executor, shards)
                        .map_err(|err| format!("gpt2: {err}"))?,
                ),
                Some(stats),
            )
        };
        admit(&client, "gpt2", &model.resident_bytes()).await?;
        llms.push(Arc::new(Llm {
            id: "gpt2",
            version,
            description: "GPT-2 small (124M) no gateway, com as projeções nos workers se houver \
                AI_CLUSTER_GPT2_SHARDS; modelo base, sem template de chat. Só /v1/chat/completions.",
            model,
            tokenizer: Tokenizer::open(&dir).map_err(|err| format!("gpt2: tokenizador: {err}"))?,
            end_of_text: 50256,
            remote,
            shards: shards_for_report,
            chat: None,
        }));
    }
    for spec in LLAMAS {
        if let Some(llm) = load_llama(&client, spec)? {
            admit(&client, llm.id, &llm.model.resident_bytes()).await?;
            llms.push(Arc::new(llm));
        }
    }
    let response_cache = Arc::new(cache::ResponseCache::new(env_number(
        "AI_CLUSTER_CACHE_ENTRIES",
        256,
    ) as usize));
    let llama_base = llama::Base::from_env()?;
    let llama = Arc::new(match llama::from_env()? {
        Some(config) => llama::Manager::fixed(llama_base, config)?,
        None => llama::Manager::new(llama_base),
    });
    announce_gateway(
        &client,
        &llms,
        addr,
        response_cache.clone(),
        tenants.clone(),
        llama.clone(),
    );
    let llama_admission = Admission::with_capacity(
        llama.slots() as usize,
        env_number("AI_CLUSTER_QUEUE_MAX", 8) as usize,
        Duration::from_secs(env_number("AI_CLUSTER_QUEUE_WAIT_SECS", 10)),
    );
    let gateway = Gateway {
        client: operator_client,
        tenants,
        cache: response_cache,
        slots: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
        llms: Arc::new(llms),
        llama,
        llama_admission,
        admission,
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(inference_metrics))
        .route("/v1/models", get(models))
        .route("/v1/embeddings", post(embeddings))
        .route(
            "/v1/chat/completions",
            post(chat_completions).layer(axum::middleware::from_fn_with_state(
                gateway.clone(),
                llama_route,
            )),
        )
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(gateway);
    println!(
        "{}",
        ai_cluster_common::brand::banner(
            "gateway",
            env!("CARGO_PKG_VERSION"),
            &format!("HTTP em {addr}")
        )
    );
    let scheme = if tls.is_some() { "https" } else { "http" };
    println!("gateway em {scheme}://{addr} (modelo {MODEL_ID}, controller {controller})");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    match tls {
        Some(config) => tls::serve(listener, app, config).await?,
        None => axum::serve(listener, app).await?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    fn cap(node: &str, available: bool, cache: u64, memory: u64) -> NodeShardCapacity {
        NodeShardCapacity {
            node_id: node.into(),
            available,
            shard_cache_bytes: cache,
            memory_bytes: memory,
        }
    }

    #[test]
    fn admissao_diz_quanto_falta() {
        let need = std::collections::BTreeMap::from([("w1".to_string(), 3u64 << 30)]);
        assert!(check_fit("m", &need, &[cap("w1", true, 4 << 30, 8 << 30)]).is_ok());
        // o cache não comporta: diz o tamanho e quanto falta, e esperar não adianta
        let (err, retry) = check_fit("m", &need, &[cap("w1", true, 2 << 30, 8 << 30)]).unwrap_err();
        assert!(!retry);
        assert!(
            err.contains("precisa de 3.0 GB") && err.contains("faltam 1.0 GB"),
            "{err}"
        );
        // a RAM do nó manda quando é menor que o cache, com 20% de folga para o resto do sistema
        assert!(check_fit("m", &need, &[cap("w1", true, 4 << 30, 2 << 30)]).is_err());
        assert!(check_fit("m", &need, &[cap("w1", true, 4 << 30, 3 << 30)]).is_err());
        assert!(check_fit("m", &need, &[cap("w1", true, 4 << 30, 4 << 30)]).is_ok());
        // memória não reportada (0) não bloqueia
        assert!(check_fit("m", &need, &[cap("w1", true, 4 << 30, 0)]).is_ok());
    }

    #[test]
    fn admissao_falha_fechada() {
        let need = std::collections::BTreeMap::from([("w1".to_string(), 1u64)]);
        // nó que ainda não apareceu: vale esperar
        assert!(check_fit("m", &need, &[]).unwrap_err().1);
        assert!(
            check_fit("m", &need, &[cap("w1", false, 1 << 30, 0)])
                .unwrap_err()
                .1
        );
        // worker antigo ou sem raiz de modelos (cache 0): não adianta esperar
        assert!(
            !check_fit("m", &need, &[cap("w1", true, 0, 8 << 30)])
                .unwrap_err()
                .1
        );
        // outro nó na resposta não vale pelo w1
        assert!(check_fit("m", &need, &[cap("w2", true, 1 << 30, 0)]).is_err());
    }

    use super::*;

    #[test]
    fn a_benched_node_hands_its_bands_to_the_live_ones() {
        let band = |node: &str, start: u32| Slice {
            node: node.into(),
            tensor: "t".into(),
            row_start: start,
            row_end: start + 1,
            by_columns: false,
        };
        let roster = Roster::new(vec!["a".into(), "b".into(), "c".into()]);
        let slices = [band("a", 0), band("b", 1), band("c", 2)];
        let nodes = |s: Vec<Slice>| s.into_iter().map(|s| s.node).collect::<Vec<_>>();
        assert_eq!(nodes(roster.assign(&slices).unwrap()), ["a", "b", "c"]);

        roster.bench("b");
        // b's band moves to a live node (chosen by position); a and c keep theirs.
        let moved = nodes(roster.assign(&slices).unwrap());
        assert_eq!((moved[0].as_str(), moved[2].as_str()), ("a", "c"));
        assert!(moved[1] == "a" || moved[1] == "c");
        // The band itself does not change, only who computes it.
        assert_eq!(roster.assign(&slices).unwrap()[1].row_start, 1);

        roster.bench("a");
        roster.bench("c");
        // With nobody live, every node is tried again.
        assert_eq!(nodes(roster.assign(&slices).unwrap()), ["a", "b", "c"]);
        assert!(Roster::new(vec![]).assign(&slices).is_err(), "sem nós");
    }

    #[test]
    fn remote_calls_see_the_generation_being_cancelled() {
        let stats = RemoteStats::default();
        assert!(!stats.cancelled(), "sem geração ligada");
        let flag = Arc::new(AtomicBool::new(false));
        stats.bind(&flag);
        assert!(!stats.cancelled());
        flag.store(true, Ordering::Relaxed);
        assert!(stats.cancelled());
    }

    #[test]
    fn streamed_text_waits_for_a_whole_character() {
        let mut sent = 0;
        assert_eq!(new_text("ol", &mut sent, false), "ol");
        // "á" is two bytes; generated one at a time, the first decodes to U+FFFD and is held.
        assert_eq!(new_text("ol\u{FFFD}", &mut sent, false), "");
        assert_eq!(new_text("olá", &mut sent, false), "á");
        assert_eq!(new_text("olá!", &mut sent, false), "!");
        // Nothing new, or a string shorter than what went out, sends nothing.
        assert_eq!(new_text("olá!", &mut sent, false), "");
        assert_eq!(new_text("ol", &mut sent, false), "");
        // At the end the held character is sent as it is.
        let mut sent = 0;
        assert_eq!(new_text("ok\u{FFFD}", &mut sent, true), "ok\u{FFFD}");
    }

    #[test]
    fn the_seed_is_stable_and_depends_on_the_whole_input() {
        assert_eq!(seed_of("olá"), seed_of("olá"));
        assert_ne!(seed_of("olá"), seed_of("olá "));
        // FNV-1a of the empty string is the offset basis; a drift here would change every
        // vector ever returned.
        assert_eq!(seed_of(""), 0xcbf2_9ce4_8422_2325);
    }
}
