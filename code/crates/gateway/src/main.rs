//! Inference API gateway (Fase J1/J3): the HTTP door the Application Plane uses (docs/23).
//!
//! Two models sit behind it. `gpt2` (J3, only when `AI_CLUSTER_GPT2_DIR` is set) is a real
//! language model, run by the in-process Inference Engine for `/v1/chat/completions`; its matrix
//! products do not go through the cluster yet. The other, `matmul-diamond-v0`, is a
//! synthetic forward pass — the diamond matrix graph — that really runs on the cluster, and
//! whose output is returned as an "embedding". The vector comes from a hash of the input pushed
//! through matrix multiplications; it carries no meaning. Every response says so, so no client
//! mistakes it for a real model. A real model replaces this by changing what `run_graph` asks
//! the controller for; the HTTP contract, auth and limits stay.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ai_cluster_common::matrix::Matrix;
use ai_cluster_engine::{
    Executor, LanguageModel, Slice, chat::ChatMl, gpt2::Gpt2, llama::Llama, tokenizer::Tokenizer,
};
use ai_cluster_protocol::cluster::{
    EnqueueGraphRequest, GetOutputRequest, GetTraceRequest, ShardMatmulRequest, ShardOp,
};
use ai_cluster_protocol::metadata::{
    FAILED_NODE_HEADER, INFERENCE_TOKEN_ENV, INSECURE_ENV, JOIN_TOKEN_ENV, MetadataClient,
    OPERATOR_TOKEN_ENV, connect_inference, connect_operator, inference_token, insecure_allowed,
    join_token, operator_token, same_token,
};
use axum::body::{Body, Bytes};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
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
    description: &'static str,
    model: Box<dyn LanguageModel + Send + Sync>,
    tokenizer: Tokenizer,
    /// The token that ends a text (`<|endoftext|>`); generation stops at it.
    end_of_text: u32,
    /// Set when the projections run on workers (J5).
    remote: Option<Arc<RemoteStats>>,
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

fn load_llama(client: &MetadataClient, spec: &LlamaSpec) -> Result<Option<Llm>, String> {
    let Some(dir) = std::env::var_os(format!("{}_DIR", spec.prefix)) else {
        return Ok(None);
    };
    let dir = std::path::PathBuf::from(dir);
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
                Llama::load_remote(&dir, executor, shards)
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
        description: spec.description,
        model,
        end_of_text: chat.as_ref().map_or(0, |chat| chat.end),
        tokenizer,
        remote,
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
            match self.runtime.block_on(client.shard_matmul(request)) {
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
    api_key: Arc<str>,
    slots: Arc<Semaphore>,
    llms: Arc<Vec<Arc<Llm>>>,
    /// One generation at a time across all models: it is CPU-bound and every thread is already
    /// used by the matmul.
    llm_slot: Arc<Semaphore>,
}

/// The external error model (docs/23 §13): a stable category and a short message. The cause
/// stays in the gateway's own log.
fn error(status: StatusCode, kind: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "type": kind, "message": message } })),
    )
        .into_response()
}

/// Whether the call carries `Authorization: Bearer <api key>`.
fn authenticated(headers: &HeaderMap, key: &str) -> bool {
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    !key.is_empty() && same_token(key.as_bytes(), given.as_bytes())
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
    // Never touches the cluster: the API's health is not the cluster's (docs/23 §21).
    Json(json!({ "api": "green" }))
}

async fn models(State(gateway): State<Gateway>, headers: HeaderMap) -> Response {
    if !authenticated(&headers, &gateway.api_key) {
        return unauthorized();
    }
    let mut data = vec![json!({
        "id": MODEL_ID,
        "object": "model",
        "owned_by": "ai-cluster",
        "kind": "synthetic-matmul",
        "description": "Forward pass sintético (grafo de multiplicação de matrizes) executado no \
            cluster. Não é um modelo de linguagem e o vetor não tem semântica.",
    })];
    for llm in gateway.llms.iter() {
        data.push(json!({
            "id": llm.id,
            "object": "model",
            "owned_by": "ai-cluster",
            "kind": "language-model",
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
/// cancellation yet (docs/23 §8). The concurrency cap bounds how fast that can pile up.
async fn run_graph(mut client: MetadataClient, seed: u64) -> Result<(String, Vec<f64>), Failure> {
    let request_id = client
        .enqueue_graph(EnqueueGraphRequest {
            size: DIMENSIONS,
            seed,
            request_id: String::new(),
            keep_result: true,
        })
        .await
        .map_err(internal)?
        .into_inner()
        .request_id;

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
    Ok((request_id, embedding))
}

async fn embeddings(
    State(gateway): State<Gateway>,
    headers: HeaderMap,
    body: Result<Json<EmbeddingRequest>, JsonRejection>,
) -> Response {
    if !authenticated(&headers, &gateway.api_key) {
        return unauthorized();
    }
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
    if request.input.is_empty() || request.input.len() > MAX_INPUT_BYTES {
        return error(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "`input` deve ter de 1 a 8192 bytes",
        );
    }
    // Backpressure with an explicit policy (docs/23 §22): reject and say when to retry.
    let Ok(_slot) = gateway.slots.clone().try_acquire_owned() else {
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
    match run_graph(client, seed_of(&request.input)).await {
        Ok((request_id, embedding)) => (
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
            .into_response(),
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
    slot: tokio::sync::OwnedSemaphorePermit,
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
fn prepare(
    gateway: &Gateway,
    headers: &HeaderMap,
    body: Result<Json<ChatRequest>, JsonRejection>,
) -> Result<Prepared, Response> {
    if !authenticated(headers, &gateway.api_key) {
        return Err(unauthorized());
    }
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
    // The slot comes first so a busy gateway spends no CPU tokenizing for a request it refuses.
    let Ok(slot) = gateway.llm_slot.clone().try_acquire_owned() else {
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
    Ok(Prepared {
        llm,
        prompt,
        new_tokens,
        stream: request.stream,
        slot,
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
    match prepare(&gateway, &headers, body) {
        Ok(prepared) if prepared.stream => stream_chat(prepared),
        Ok(prepared) => complete_chat(prepared).await,
        Err(response) => response,
    }
}

/// Sets the flag when dropped, whether the request finished or its future was dropped.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

async fn complete_chat(prepared: Prepared) -> Response {
    let Prepared {
        llm,
        prompt,
        new_tokens,
        slot,
        ..
    } = prepared;
    let prompt_tokens = prompt.len();
    // The permit moves into the blocking task, so the slot frees only when the generation really
    // ends. A timeout sets `cancel`, which stops it at the next token.
    let cancel = Arc::new(AtomicBool::new(false));
    // If the client disconnects, axum drops this future: the guard still cancels the generation.
    let _cancel_on_drop = CancelOnDrop(cancel.clone());
    let worker = llm.clone();
    let flag = cancel.clone();
    let task = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        worker
            .model
            .generate(&prompt, new_tokens, Some(worker.end_of_text), &flag)
    });
    let generated = match tokio::time::timeout(DEADLINE, task).await {
        Ok(Ok(Ok(tokens))) => tokens,
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
    let id = format!("chatcmpl-{}", NEXT_CHAT_ID.fetch_add(1, Ordering::Relaxed));
    (
        [
            ("x-request-id", id.clone()),
            ("x-model-id", llm.id.to_string()),
        ],
        Json(json!({
            "id": id,
            "object": "chat.completion",
            "created": unix_seconds(),
            "model": llm.id,
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": content },
                "finish_reason": if stopped { "stop" } else { "length" },
            }],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": visible.len(),
                "total_tokens": prompt_tokens + visible.len(),
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
fn stream_chat(prepared: Prepared) -> Response {
    let Prepared {
        llm,
        prompt,
        new_tokens,
        slot,
        ..
    } = prepared;
    let id = format!("chatcmpl-{}", NEXT_CHAT_ID.fetch_add(1, Ordering::Relaxed));
    let created = unix_seconds();
    let model_id = llm.id;
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
        let _slot = slot;
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
        let generated =
            worker
                .model
                .generate_with(&prompt, new_tokens, Some(stop), &flag, &mut |token| {
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
    response
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("AI_CLUSTER_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or("AI_CLUSTER_API_KEY é obrigatório; o gateway não sobe sem chave")?;
    // The key faces applications; the join and operator tokens face the cluster. Reusing one
    // would let an application's key act as a node or as the operator.
    if [join_token(), operator_token(), inference_token()]
        .iter()
        .any(|token| token.as_ref() == Some(&api_key))
    {
        return Err(format!(
            "AI_CLUSTER_API_KEY deve ser diferente de {JOIN_TOKEN_ENV}, {OPERATOR_TOKEN_ENV} e {INFERENCE_TOKEN_ENV}"
        )
        .into());
    }
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
    // The HTTP leg has no TLS yet, so the key would cross the network in clear: loopback only,
    // unless the operator accepts that explicitly.
    if !addr.ip().is_loopback() && !insecure_allowed() {
        return Err(format!(
            "{addr} não é loopback e a perna HTTP não tem TLS; use {INSECURE_ENV}=1 numa rede fechada"
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
        // AI_CLUSTER_GPT2_SHARDS: see the SmolLM2 block below; GPT-2's slices are column bands.
        let shards = shard_nodes("AI_CLUSTER_GPT2_SHARDS");
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
        llms.push(Arc::new(Llm {
            id: "gpt2",
            description: "GPT-2 small (124M) no gateway, com as projeções nos workers se houver \
                AI_CLUSTER_GPT2_SHARDS; modelo base, sem template de chat. Só /v1/chat/completions.",
            model,
            tokenizer: Tokenizer::open(&dir).map_err(|err| format!("gpt2: tokenizador: {err}"))?,
            end_of_text: 50256,
            remote,
            chat: None,
        }));
    }
    for spec in LLAMAS {
        if let Some(llm) = load_llama(&client, spec)? {
            llms.push(Arc::new(llm));
        }
    }
    let gateway = Gateway {
        client: operator_client,
        api_key: api_key.into(),
        slots: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
        llms: Arc::new(llms),
        llm_slot: Arc::new(Semaphore::new(1)),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/v1/embeddings", post(embeddings))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(gateway);
    println!("gateway em http://{addr} (modelo {MODEL_ID}, controller {controller})");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn only_the_exact_bearer_key_passes() {
        let with = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::AUTHORIZATION, value.parse().unwrap());
            headers
        };
        assert!(authenticated(&with("Bearer k"), "k"));
        for bad in ["Bearer K", "Bearer ", "k", "Basic k", "Bearer kk"] {
            assert!(!authenticated(&with(bad), "k"), "{bad:?}");
        }
        assert!(!authenticated(&HeaderMap::new(), "k"));
        assert!(!authenticated(&with("Bearer "), ""));
    }
}
