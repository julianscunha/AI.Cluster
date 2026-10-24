//! Inference API gateway (Fase J1): the HTTP door the Application Plane uses (docs/23).
//!
//! There is no language model behind it yet. The one "model" is `matmul-diamond-v0`: a
//! synthetic forward pass — the diamond matrix graph — that really runs on the cluster, and
//! whose output is returned as an "embedding". The vector comes from a hash of the input pushed
//! through matrix multiplications; it carries no meaning. Every response says so, so no client
//! mistakes it for a real model. A real model replaces this by changing what `run_graph` asks
//! the controller for; the HTTP contract, auth and limits stay.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ai_cluster_protocol::cluster::{EnqueueGraphRequest, GetOutputRequest, GetTraceRequest};
use ai_cluster_protocol::metadata::{
    INSECURE_ENV, JOIN_TOKEN_ENV, MetadataClient, OPERATOR_TOKEN_ENV, connect_operator,
    insecure_allowed, join_token, operator_token, same_token,
};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
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

#[derive(Clone)]
struct Gateway {
    client: MetadataClient,
    api_key: Arc<str>,
    slots: Arc<Semaphore>,
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
    Json(json!({
        "object": "list",
        "data": [{
            "id": MODEL_ID,
            "object": "model",
            "owned_by": "ai-cluster",
            "kind": "synthetic-matmul",
            "description": "Forward pass sintético (grafo de multiplicação de matrizes) executado no \
                cluster. Não é um modelo de linguagem e o vetor não tem semântica.",
        }],
    }))
    .into_response()
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

    match run_graph(gateway.client.clone(), seed_of(&request.input)).await {
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("AI_CLUSTER_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or("AI_CLUSTER_API_KEY é obrigatório; o gateway não sobe sem chave")?;
    // The key faces applications; the join and operator tokens face the cluster. Reusing one
    // would let an application's key act as a node or as the operator.
    if Some(&api_key) == join_token().as_ref() || Some(&api_key) == operator_token().as_ref() {
        return Err(format!(
            "AI_CLUSTER_API_KEY deve ser diferente de {JOIN_TOKEN_ENV} e de {OPERATOR_TOKEN_ENV}"
        )
        .into());
    }
    if operator_token().is_none() {
        return Err(
            format!("{OPERATOR_TOKEN_ENV} é obrigatório: o gateway fala como operador").into(),
        );
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
    let client = connect_operator(controller.clone()).await?;
    let gateway = Gateway {
        client,
        api_key: api_key.into(),
        slots: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/v1/embeddings", post(embeddings))
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
