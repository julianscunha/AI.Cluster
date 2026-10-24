//! gRPC metadata for the per-message fields `protocol/controller-worker.md` requires.
//!
//! The protocol doc asks every message to carry a protocol version, a correlation
//! id and an authentication context. These are cross-cutting call attributes, so
//! they ride as HTTP/2 metadata — the place gRPC keeps exactly this kind of data —
//! instead of being repeated in every protobuf payload.
//!
//! `node_id` already travels in each payload, and idempotency is enforced
//! structurally in the dispatch queue: a work unit leaves the dispatch table on
//! completion, so a duplicate submission has nothing to complete.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tonic::service::Interceptor;
use tonic::{Request, Status};

use crate::cluster::cluster_control_client::ClusterControlClient;
use crate::cluster::link_probe_client::LinkProbeClient;

pub const PROTOCOL_VERSION: &str = "v1";
pub const VERSION_HEADER: &str = "x-ai-cluster-protocol-version";
pub const CORRELATION_HEADER: &str = "x-ai-cluster-correlation-id";
pub const AUTH_HEADER: &str = "x-ai-cluster-auth";
/// Opt-out of transport security (Fase S4): with it set, an `http://` endpoint is allowed and a
/// controller without a certificate starts. Meant for a closed lab network and the test suite;
/// every token then crosses the wire in clear.
pub const INSECURE_ENV: &str = "AI_CLUSTER_INSECURE";
/// PEM file with the CA that signed the controller's certificate, for `https://` endpoints.
pub const TLS_CA_ENV: &str = "AI_CLUSTER_TLS_CA";
/// Name to verify the certificate against when the endpoint is an IP or an alias.
pub const TLS_DOMAIN_ENV: &str = "AI_CLUSTER_TLS_DOMAIN";

/// Whether the operator chose to run without TLS.
pub fn insecure_allowed() -> bool {
    std::env::var(INSECURE_ENV).is_ok_and(|value| matches!(value.trim(), "1" | "true"))
}

/// Opens the channel. `https://` verifies the controller against [`TLS_CA_ENV`]; `http://` is
/// refused unless [`INSECURE_ENV`] is set, so a worker never sends a token in clear by default.
async fn open_channel(
    dst: String,
) -> Result<tonic::transport::Channel, Box<dyn std::error::Error>> {
    let mut endpoint = tonic::transport::Endpoint::from_shared(dst.clone())?;
    if dst.to_ascii_lowercase().starts_with("https://") {
        let ca_path = std::env::var(TLS_CA_ENV)
            .map_err(|_| format!("{TLS_CA_ENV} é obrigatório com endpoint https"))?;
        let mut tls = tonic::transport::ClientTlsConfig::new().ca_certificate(
            tonic::transport::Certificate::from_pem(std::fs::read(&ca_path)?),
        );
        if let Ok(domain) = std::env::var(TLS_DOMAIN_ENV) {
            tls = tls.domain_name(domain);
        }
        endpoint = endpoint.tls_config(tls)?;
    } else if !insecure_allowed() {
        return Err(format!(
            "endpoint {dst} sem TLS; use https:// ou {INSECURE_ENV}=1 numa rede fechada"
        )
        .into());
    }
    Ok(endpoint.connect().await?)
}

/// Header carrying the per-node credential the controller issued at registration (S3b).
pub const NODE_AUTH_HEADER: &str = "x-ai-cluster-node-auth";

/// Ceiling for a single gRPC message, in bytes, in both directions.
///
/// A work unit travels as one message carrying its operands as `repeated double`: a
/// 512x512 unit is two f64 matrices, so its payload is already past the 4MB gRPC
/// default and a worker refuses to decode it — the unit can never be dispatched. The
/// limit is raised rather than the unit split, because units are sent whole today.
/// This is not the placement filter: whether a unit *should* go to a node is
/// `placement::fits`, which reads the node's reported memory. This only stops the
/// transport from rejecting a unit the placement layer already allowed.
///
/// ponytail: 64MB covers units up to ~1024x1024, whose footprint is ~24MB. Stream or
/// chunk the operands when units grow past what a node can hold in RAM anyway.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

/// Environment variable holding the shared join token (Fase S1). The controller refuses to
/// start without it, and every worker and CLI call carries it in [`AUTH_HEADER`].
///
/// ponytail: one shared secret, sent in clear until TLS exists. It closes "anyone on the network
/// can register a node or inject state"; it does not give a node its own identity, and it can
/// be captured on the wire. Per-node credentials and mTLS (docs/19) are the next rungs.
pub const JOIN_TOKEN_ENV: &str = "AI_CLUSTER_JOIN_TOKEN";

/// The join token from the environment; unset or blank is "none".
pub fn join_token() -> Option<String> {
    std::env::var(JOIN_TOKEN_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Environment variable holding the operator token (Fase S3a). It authorizes the administrative
/// RPCs (approve, enqueue, reserve, release); the join token does not, so a node that holds
/// only the join token cannot approve itself or queue work.
pub const OPERATOR_TOKEN_ENV: &str = "AI_CLUSTER_OPERATOR_TOKEN";

/// The operator token from the environment; unset or blank is "none".
pub fn operator_token() -> Option<String> {
    std::env::var(OPERATOR_TOKEN_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Marker the interceptor leaves in a request's extensions when the caller proved the
/// operator token.
#[derive(Clone, Copy, Debug)]
pub struct Operator;

/// Refuses the call unless it came in with the operator token.
pub fn require_operator<T>(request: &Request<T>) -> Result<(), Status> {
    if request.extensions().get::<Operator>().is_some() {
        Ok(())
    } else {
        Err(Status::permission_denied(
            "esta operação exige o token de operador",
        ))
    }
}

static CORRELATION_SEQ: AtomicU64 = AtomicU64::new(0);

/// Timestamp plus a process-local counter: unique enough across processes to
/// trace a call without pulling in a uuid dependency.
///
/// ponytail: a real trace id arrives with tracing; this is a readable stand-in.
fn next_correlation_id() -> String {
    let count = CORRELATION_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0);
    format!("c-{nanos:x}-{count:x}")
}

/// A controller client whose calls carry the required protocol metadata.
pub type MetadataClient = ClusterControlClient<
    tonic::service::interceptor::InterceptedService<tonic::transport::Channel, ClientMetadata>,
>;

/// Client of the throughput-probe service (C2), a separate service so the server can cap its
/// messages without lowering the limit work units need.
pub type ProbeClient = LinkProbeClient<
    tonic::service::interceptor::InterceptedService<tonic::transport::Channel, ClientMetadata>,
>;

/// Stamps every outgoing call with the protocol version, a fresh correlation id
/// and the auth context.
#[derive(Clone, Debug, Default)]
pub struct ClientMetadata {
    token: String,
    credential: NodeCredential,
}

/// The credential a node received at registration, shared with the interceptor of every client
/// cloned from the connection, so one `set` after `RegisterNode` stamps all later calls.
#[derive(Clone, Debug, Default)]
pub struct NodeCredential(std::sync::Arc<std::sync::Mutex<String>>);

impl NodeCredential {
    pub fn set(&self, value: &str) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = value.to_string();
    }

    fn get(&self) -> String {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl ClientMetadata {
    /// Reads the join token once. Without one the header goes out empty and the controller
    /// answers `Unauthenticated`, which is the honest error for a node that was never given it.
    pub fn from_env() -> Self {
        Self::with_token(join_token().unwrap_or_default())
    }

    /// The operator token, for the CLI. Same fail-visible behavior as [`Self::from_env`].
    pub fn operator_from_env() -> Self {
        Self::with_token(operator_token().unwrap_or_default())
    }

    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            credential: NodeCredential::default(),
        }
    }
}

impl Interceptor for ClientMetadata {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let metadata = request.metadata_mut();
        metadata.insert(VERSION_HEADER, PROTOCOL_VERSION.parse().expect("ascii"));
        metadata.insert(
            CORRELATION_HEADER,
            next_correlation_id().parse().expect("ascii"),
        );
        // A token that is not a valid header value cannot be sent; the call then goes out
        // without it and is refused, rather than panicking the client.
        if let Ok(value) = self.token.parse() {
            metadata.insert(AUTH_HEADER, value);
        }
        let credential = self.credential.get();
        if !credential.is_empty()
            && let Ok(value) = credential.parse()
        {
            metadata.insert(NODE_AUTH_HEADER, value);
        }
        Ok(request)
    }
}

/// Rejects calls that lack the join or operator token (the latter also marks the request as
/// [`Operator`]) or do not announce the protocol version this build
/// speaks. The token check comes first, so an unauthenticated caller learns nothing else.
#[derive(Clone, Debug)]
pub struct ServerMetadata {
    token: std::sync::Arc<str>,
    operator: std::sync::Arc<str>,
}

impl ServerMetadata {
    pub fn new(token: &str, operator: &str) -> Self {
        Self {
            token: token.into(),
            operator: operator.into(),
        }
    }
}

/// The credential a call presents, or empty when it carries none.
pub fn node_credential<T>(request: &Request<T>) -> &[u8] {
    request
        .metadata()
        .get(NODE_AUTH_HEADER)
        .map_or(&b""[..], |value| value.as_bytes())
}

/// Equal-length byte comparison without an early exit, so timing does not reveal how much of
/// a guess matched. The length itself is not secret.
pub fn same_token(expected: &[u8], given: &[u8]) -> bool {
    expected.len() == given.len()
        && expected
            .iter()
            .zip(given)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

impl Interceptor for ServerMetadata {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let given = request
            .metadata()
            .get(AUTH_HEADER)
            .map_or(&b""[..], |value| value.as_bytes());
        // Both comparisons always run, so timing does not say which token a guess resembled.
        let is_operator = !self.operator.is_empty() && same_token(self.operator.as_bytes(), given);
        let is_node = !self.token.is_empty() && same_token(self.token.as_bytes(), given);
        if !is_operator && !is_node {
            return Err(Status::unauthenticated("token ausente ou inválido"));
        }
        if is_operator {
            request.extensions_mut().insert(Operator);
        }
        let version = request
            .metadata()
            .get(VERSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<missing>");
        if version == PROTOCOL_VERSION {
            Ok(request)
        } else {
            Err(Status::failed_precondition(format!(
                "versão de protocolo {version} não é suportada; este controller fala {PROTOCOL_VERSION}"
            )))
        }
    }
}

/// Connects to a controller with the metadata interceptors attached.
///
/// The message-size ceiling is raised past the gRPC default so a worker can receive a
/// work unit whose operands exceed 4MB — see [`MAX_MESSAGE_BYTES`].
///
/// Returns the handle a node sets with the credential from `RegisterNode` (S3b).
pub async fn connect_node(
    dst: impl Into<String>,
) -> Result<(MetadataClient, NodeCredential), Box<dyn std::error::Error>> {
    let metadata = ClientMetadata::from_env();
    let credential = metadata.credential.clone();
    Ok((connect_with(dst, metadata).await?, credential))
}

/// Connects as the operator (the CLI): same channel, the operator token on every call.
pub async fn connect_operator(
    dst: impl Into<String>,
) -> Result<MetadataClient, Box<dyn std::error::Error>> {
    connect_with(dst, ClientMetadata::operator_from_env()).await
}

async fn connect_with(
    dst: impl Into<String>,
    metadata: ClientMetadata,
) -> Result<MetadataClient, Box<dyn std::error::Error>> {
    let channel = open_channel(dst.into()).await?;
    Ok(ClusterControlClient::with_interceptor(channel, metadata)
        .max_decoding_message_size(MAX_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES))
}

/// Connects the probe client to the same controller address.
pub async fn connect_probe(
    dst: impl Into<String>,
) -> Result<ProbeClient, Box<dyn std::error::Error>> {
    let channel = open_channel(dst.into()).await?;
    Ok(LinkProbeClient::with_interceptor(
        channel,
        ClientMetadata::from_env(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_stamps_required_metadata() {
        let mut interceptor = ClientMetadata::with_token("secret");
        let request = interceptor
            .call(Request::new(()))
            .expect("stamping succeeds");
        let metadata = request.metadata();
        assert_eq!(
            metadata.get(VERSION_HEADER).unwrap().to_str().unwrap(),
            PROTOCOL_VERSION
        );
        assert_eq!(
            metadata.get(AUTH_HEADER).unwrap().to_str().unwrap(),
            "secret"
        );
        assert!(
            metadata
                .get(CORRELATION_HEADER)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("c-")
        );
    }

    #[test]
    fn correlation_ids_differ_between_calls() {
        let mut interceptor = ClientMetadata::with_token("secret");
        let first = interceptor.call(Request::new(())).unwrap();
        let second = interceptor.call(Request::new(())).unwrap();
        assert_ne!(
            first.metadata().get(CORRELATION_HEADER),
            second.metadata().get(CORRELATION_HEADER)
        );
    }

    #[test]
    fn server_accepts_matching_version_and_rejects_others() {
        let mut stamp = ClientMetadata::with_token("secret");
        let mut server = ServerMetadata::new("secret", "op-secret");
        let stamped = stamp.call(Request::new(())).unwrap();
        assert!(server.call(stamped).is_ok());

        let mut wrong = Request::new(());
        wrong
            .metadata_mut()
            .insert(VERSION_HEADER, "v0".parse().unwrap());
        wrong
            .metadata_mut()
            .insert(AUTH_HEADER, "secret".parse().unwrap());
        assert_eq!(
            server.call(wrong).unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );

        // Missing header is rejected too.
        assert!(server.call(Request::new(())).is_err());
    }

    #[test]
    fn server_refuses_a_missing_wrong_or_empty_token() {
        let mut server = ServerMetadata::new("secret", "op-secret");
        let with = |token: &str| {
            ClientMetadata::with_token(token)
                .call(Request::new(()))
                .unwrap()
        };
        for bad in ["wrong", "secre", "secrets", ""] {
            assert_eq!(
                server.call(with(bad)).unwrap_err().code(),
                tonic::Code::Unauthenticated,
                "token {bad:?}"
            );
        }
        // A server built with an empty token accepts nobody, not everybody.
        let mut open = ServerMetadata::new("", "");
        assert_eq!(
            open.call(with("")).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );
    }

    #[test]
    fn only_the_operator_token_marks_the_request_as_operator() {
        let mut server = ServerMetadata::new("secret", "op-secret");
        let with = |token: &str| {
            ClientMetadata::with_token(token)
                .call(Request::new(()))
                .unwrap()
        };
        let node = server.call(with("secret")).unwrap();
        assert!(require_operator(&node).is_err());
        let operator = server.call(with("op-secret")).unwrap();
        assert!(require_operator(&operator).is_ok());
        // With no operator token configured, an empty guess must not become the operator.
        let mut none = ServerMetadata::new("secret", "");
        assert!(none.call(with("")).is_err());
    }
}
