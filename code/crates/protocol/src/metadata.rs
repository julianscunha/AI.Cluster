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
/// mTLS (F19), controller side: PEM with the CA that signs client certificates. Set, the controller
/// refuses any connection that does not present one signed by it.
pub const TLS_CLIENT_CA_ENV: &str = "AI_CLUSTER_TLS_CLIENT_CA";
/// mTLS, client side: the certificate and key this process presents to the controller. They come
/// as a pair.
pub const TLS_CLIENT_CERT_ENV: &str = "AI_CLUSTER_TLS_CLIENT_CERT";
pub const TLS_CLIENT_KEY_ENV: &str = "AI_CLUSTER_TLS_CLIENT_KEY";
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
        match (
            std::env::var(TLS_CLIENT_CERT_ENV),
            std::env::var(TLS_CLIENT_KEY_ENV),
        ) {
            (Ok(cert), Ok(key)) => {
                tls = tls.identity(tonic::transport::Identity::from_pem(
                    std::fs::read(cert)?,
                    std::fs::read(key)?,
                ));
            }
            (Err(_), Err(_)) => {}
            _ => {
                return Err(
                    format!("{TLS_CLIENT_CERT_ENV} e {TLS_CLIENT_KEY_ENV} vêm juntos").into(),
                );
            }
        }
        endpoint = endpoint.tls_config(tls)?;
    } else if !insecure_allowed() {
        return Err(format!(
            "endpoint {dst} sem TLS; use https:// ou {INSECURE_ENV}=1 numa rede fechada"
        )
        .into());
    }
    // A half-open connection (Wi-Fi drop, suspended worker) would otherwise look alive until a
    // long-lived stream times out; probing detects it in seconds.
    let endpoint = endpoint
        .http2_keep_alive_interval(std::time::Duration::from_secs(10))
        .keep_alive_timeout(std::time::Duration::from_secs(10))
        .keep_alive_while_idle(true);
    Ok(endpoint.connect().await?)
}

/// Header carrying the per-node credential the controller issued at registration (S3b).
pub const NODE_AUTH_HEADER: &str = "x-ai-cluster-node-auth";

/// Header naming the node on a call whose body is a stream (J5): the credential is checked
/// when the stream opens, so the node has to be known then.
pub const NODE_ID_HEADER: &str = "x-ai-cluster-node-id";

/// Header on a failed `ShardMatmul` naming the node that caused the failure, so the caller can
/// route its slices elsewhere and try again (J5).
pub const FAILED_NODE_HEADER: &str = "x-ai-cluster-failed-node";

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
/// be captured on the wire. Per-node credentials and mTLS (`seguranca`) are the next rungs.
pub const JOIN_TOKEN_ENV: &str = "AI_CLUSTER_JOIN_TOKEN";

/// The join token from the environment; unset or blank is "none".
pub fn join_token() -> Option<String> {
    secret(JOIN_TOKEN_ENV)
}

/// Um segredo do ambiente. `<NOME>_FILE` aponta para um arquivo com o valor (secret do Docker ou
/// do Kubernetes, arquivo 0600): assim o segredo não aparece em `docker inspect` nem em
/// `/proc/<pid>/environ`. O arquivo vence a variável. Um arquivo configurado e ilegível não cai
/// para a variável: devolve "nenhum" (e avisa), e quem exige o segredo recusa subir.
pub fn secret(name: &str) -> Option<String> {
    if let Some(path) = std::env::var_os(format!("{name}_FILE")).filter(|path| !path.is_empty()) {
        return match read_secret_file(std::path::Path::new(&path)) {
            Ok(value) => Some(value),
            Err(why) => {
                eprintln!("aviso: {name}_FILE: {why}");
                None
            }
        };
    }
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// O conteúdo do arquivo de segredo, sem espaços nas pontas. Recusa vazio e, no Unix, arquivo
/// legível por grupo ou outros (um segredo com permissão aberta já vazou).
fn read_secret_file(path: &std::path::Path) -> Result<String, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(format!(
                "{} com permissão {:o}: use 0600 (chmod 600)",
                path.display(),
                mode & 0o777
            ));
        }
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value = text.trim().to_string();
    if value.is_empty() {
        return Err(format!("{} está vazio", path.display()));
    }
    Ok(value)
}

/// Environment variable holding the operator token (Fase S3a). It authorizes the administrative
/// RPCs (approve, enqueue, reserve, release); the join token does not, so a node that holds
/// only the join token cannot approve itself or queue work.
pub const OPERATOR_TOKEN_ENV: &str = "AI_CLUSTER_OPERATOR_TOKEN";

/// The operator token from the environment; unset or blank is "none".
pub fn operator_token() -> Option<String> {
    secret(OPERATOR_TOKEN_ENV)
}

/// Environment variable holding the inference token (J5). It authorizes `ShardMatmul` and
/// nothing else, so a gateway can run models on the workers without holding the operator
/// token that approves nodes and queues arbitrary work. Optional.
pub const INFERENCE_TOKEN_ENV: &str = "AI_CLUSTER_INFERENCE_TOKEN";

/// The inference token from the environment; unset or blank is "none".
pub fn inference_token() -> Option<String> {
    secret(INFERENCE_TOKEN_ENV)
}

/// Environment variable holding the recovery token (F19, ADR-017). It is what a node in network
/// recovery carries (in clear, in the boot script on its card): it may register, heartbeat, report
/// health and leave, and nothing else, so a leak does not let anyone take work or results. Optional.
pub const RECOVERY_TOKEN_ENV: &str = "AI_CLUSTER_RECOVERY_TOKEN";

/// The recovery token from the environment; unset or blank is "none".
pub fn recovery_token() -> Option<String> {
    secret(RECOVERY_TOKEN_ENV)
}

/// Marker the interceptor leaves in a request's extensions when the caller proved the
/// inference token.
#[derive(Clone, Copy, Debug)]
pub struct Inference;

/// Refuses the call unless it came in with the operator or the inference token.
pub fn require_inference<T>(request: &Request<T>) -> Result<(), Status> {
    if request.extensions().get::<Inference>().is_some() || require_operator(request).is_ok() {
        Ok(())
    } else {
        Err(Status::permission_denied(
            "esta operação exige o token de operador ou de inferência",
        ))
    }
}

/// Marker the interceptor leaves when the caller proved the recovery token.
#[derive(Clone, Copy, Debug)]
pub struct Recovery;

/// Suffix every node id registered with the recovery token must carry, so that token can never
/// speak for (or replace) a real node.
pub const RECOVERY_NODE_SUFFIX: &str = "-rec";

/// Whether the call came in with the recovery token.
pub fn is_recovery<T>(request: &Request<T>) -> bool {
    request.extensions().get::<Recovery>().is_some()
}

/// A recovery call may speak only for a node id ending in [`RECOVERY_NODE_SUFFIX`].
pub fn require_recovery_scope<T>(request: &Request<T>, node_id: &str) -> Result<(), Status> {
    if is_recovery(request) && !node_id.ends_with(RECOVERY_NODE_SUFFIX) {
        return Err(Status::permission_denied(
            "o token de recuperação só vale para nós com sufixo -rec",
        ));
    }
    Ok(())
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
#[derive(Clone, Default)]
pub struct ClientMetadata {
    token: String,
    credential: NodeCredential,
}

/// The credential a node received at registration, shared with the interceptor of every client
/// cloned from the connection, so one `set` after `RegisterNode` stamps all later calls.
#[derive(Clone, Default)]
pub struct NodeCredential(std::sync::Arc<std::sync::Mutex<String>>);

// Secrets stay out of `{:?}`: a log line or a failed assert must not print them.
impl std::fmt::Debug for NodeCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NodeCredential(<redigido>)")
    }
}

impl std::fmt::Debug for ClientMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClientMetadata(<redigido>)")
    }
}

impl std::fmt::Debug for ServerMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServerMetadata(<redigido>)")
    }
}

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
#[derive(Clone)]
pub struct ServerMetadata {
    token: std::sync::Arc<str>,
    operator: std::sync::Arc<str>,
    inference: std::sync::Arc<str>,
    recovery: std::sync::Arc<str>,
}

impl ServerMetadata {
    pub fn new(token: &str, operator: &str) -> Self {
        Self {
            token: token.into(),
            operator: operator.into(),
            inference: "".into(),
            recovery: "".into(),
        }
    }

    /// Also accepts the recovery token; pair it with [`recovery_gate`], which keeps it to the calls
    /// a node in recovery needs.
    pub fn with_recovery(mut self, recovery: &str) -> Self {
        self.recovery = recovery.into();
        self
    }

    /// Also accepts the inference token; pair it with [`inference_gate`], which keeps it to
    /// `ShardMatmul`.
    pub fn with_inference(mut self, inference: &str) -> Self {
        self.inference = inference.into();
        self
    }
}

/// Path of `ShardMatmul`, the one call the inference token may make.
const SHARD_MATMUL_PATH: &str = "/ai.cluster.v1.ClusterControl/ShardMatmul";

/// Path of `ShardCapacity`, the other call the inference token may make: how much slice memory
/// the nodes it names accept.
const SHARD_CAPACITY_PATH: &str = "/ai.cluster.v1.ClusterControl/ShardCapacity";
const REPORT_GATEWAY_PATH: &str = "/ai.cluster.v1.ClusterControl/ReportGateway";

/// The calls a node in network recovery may make: show up, stay alive, say how it is, leave.
const RECOVERY_PATHS: &[&str] = &[
    "/ai.cluster.v1.ClusterControl/RegisterNode",
    "/ai.cluster.v1.ClusterControl/Heartbeat",
    "/ai.cluster.v1.ClusterControl/ReportHealth",
    "/ai.cluster.v1.ClusterControl/LeaveNode",
];

/// Wraps a service so a caller presenting a scoped token reaches only `allowed` paths and gets
/// `PermissionDenied` on every other. Fail-closed by construction: a new RPC is out of reach of
/// that token until someone says otherwise here.
#[derive(Clone)]
pub struct ScopeGate<S> {
    inner: S,
    token: std::sync::Arc<str>,
    allowed: &'static [&'static str],
}

/// The inference token reaches `ShardMatmul` and `ShardCapacity` only.
pub fn inference_gate<S>(inner: S, token: &str) -> ScopeGate<S> {
    scope_gate(
        inner,
        token,
        &[SHARD_MATMUL_PATH, SHARD_CAPACITY_PATH, REPORT_GATEWAY_PATH],
    )
}

/// The recovery token reaches [`RECOVERY_PATHS`] only.
pub fn recovery_gate<S>(inner: S, token: &str) -> ScopeGate<S> {
    scope_gate(inner, token, RECOVERY_PATHS)
}

fn scope_gate<S>(inner: S, token: &str, allowed: &'static [&'static str]) -> ScopeGate<S> {
    ScopeGate {
        inner,
        token: token.into(),
        allowed,
    }
}

impl<S: tonic::server::NamedService> tonic::server::NamedService for ScopeGate<S> {
    const NAME: &'static str = S::NAME;
}

impl<S, B> tonic::codegen::Service<tonic::codegen::http::Request<B>> for ScopeGate<S>
where
    S: tonic::codegen::Service<
            tonic::codegen::http::Request<B>,
            Response = tonic::codegen::http::Response<tonic::body::Body>,
        > + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = tonic::codegen::BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: tonic::codegen::http::Request<B>) -> Self::Future {
        let presented = request
            .headers()
            .get(AUTH_HEADER)
            .map_or(&b""[..], |value| value.as_bytes());
        if !self.token.is_empty()
            && same_token(self.token.as_bytes(), presented)
            && !self.allowed.contains(&request.uri().path())
        {
            return Box::pin(async {
                Ok(Status::permission_denied("este token não vale para esta chamada").into_http())
            });
        }
        // The clone that was polled ready is the one that serves the call.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(inner.call(request))
    }
}

/// The credential a call presents, or empty when it carries none.
pub fn node_credential<T>(request: &Request<T>) -> &[u8] {
    request
        .metadata()
        .get(NODE_AUTH_HEADER)
        .map_or(&b""[..], |value| value.as_bytes())
}

/// Impressão digital de uma credencial: 8 bytes do SHA-256 em hexadecimal. O nó a mostra e o operador a
/// confere no `approve`; não permite recuperar a credencial.
pub fn credential_fingerprint(credential: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(credential.as_bytes())[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
        let is_inference =
            !self.inference.is_empty() && same_token(self.inference.as_bytes(), given);
        let is_recovery = !self.recovery.is_empty() && same_token(self.recovery.as_bytes(), given);
        if !is_operator && !is_node && !is_inference && !is_recovery {
            return Err(Status::unauthenticated("token ausente ou inválido"));
        }
        if is_operator {
            request.extensions_mut().insert(Operator);
        }
        if is_inference {
            request.extensions_mut().insert(Inference);
        }
        if is_recovery {
            request.extensions_mut().insert(Recovery);
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

/// Connects with the inference token on every call (the gateway's model traffic).
pub async fn connect_inference(
    dst: impl Into<String>,
) -> Result<MetadataClient, Box<dyn std::error::Error>> {
    connect_with(
        dst,
        ClientMetadata::with_token(inference_token().unwrap_or_default()),
    )
    .await
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
    fn segredo_em_arquivo_exige_permissao_fechada_e_conteudo() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("aic-secret-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("token");
        std::fs::write(&file, "  s3gredo\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_secret_file(&file).unwrap(), "s3gredo");

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_secret_file(&file).unwrap_err().contains("0600"));

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&file, " \n").unwrap();
        assert!(read_secret_file(&file).unwrap_err().contains("vazio"));
        assert!(read_secret_file(&dir.join("falta")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

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

    /// Answers 200 to anything that reaches it.
    #[derive(Clone)]
    struct Reached;

    impl tonic::codegen::Service<tonic::codegen::http::Request<()>> for Reached {
        type Response = tonic::codegen::http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(
            &mut self,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: tonic::codegen::http::Request<()>) -> Self::Future {
            std::future::ready(Ok(tonic::codegen::http::Response::new(
                tonic::body::Body::empty(),
            )))
        }
    }

    #[tokio::test]
    async fn the_gate_lets_the_inference_token_reach_only_shard_matmul() {
        use tonic::codegen::Service;
        let mut gate = inference_gate(Reached, "inf");
        let mut call = async |token: &str, path: &str| {
            let request = tonic::codegen::http::Request::builder()
                .uri(path)
                .header(AUTH_HEADER, token)
                .body(())
                .unwrap();
            let response = gate.call(request).await.unwrap();
            // A denial is a gRPC status in the headers; a pass-through has none.
            response.headers().get("grpc-status").is_none()
        };
        assert!(call("inf", SHARD_MATMUL_PATH).await);
        assert!(call("inf", SHARD_CAPACITY_PATH).await);
        assert!(call("inf", REPORT_GATEWAY_PATH).await);
        for path in [
            "/ai.cluster.v1.ClusterControl/ListNodes",
            "/ai.cluster.v1.ClusterControl/ListGateways",
            "/ai.cluster.v1.ClusterControl/ShardChannel",
            "/ai.cluster.v1.LinkProbe/Probe",
            "/ai.cluster.v1.ClusterControl/ShardMatmul/",
        ] {
            assert!(!call("inf", path).await, "{path}");
        }
        // Other tokens are not the gate's business (the interceptor judges them).
        assert!(call("op-secret", "/ai.cluster.v1.ClusterControl/ListNodes").await);
    }

    #[tokio::test]
    async fn the_gate_lets_the_recovery_token_only_show_the_node_up() {
        use tonic::codegen::Service;
        let mut gate = recovery_gate(Reached, "rec");
        let mut call = async |token: &str, path: &str| {
            let request = tonic::codegen::http::Request::builder()
                .uri(path)
                .header(AUTH_HEADER, token)
                .body(())
                .unwrap();
            let response = gate.call(request).await.unwrap();
            response.headers().get("grpc-status").is_none()
        };
        for path in RECOVERY_PATHS {
            assert!(call("rec", path).await, "{path}");
        }
        for path in [
            "/ai.cluster.v1.ClusterControl/RequestWork",
            "/ai.cluster.v1.ClusterControl/SubmitWorkResult",
            "/ai.cluster.v1.ClusterControl/SetRelease",
            "/ai.cluster.v1.ClusterControl/ShardMatmul",
            "/ai.cluster.v1.ClusterControl/RegisterNode/",
            "/ai.cluster.v1.LinkProbe/Probe",
        ] {
            assert!(!call("rec", path).await, "{path}");
        }
        assert!(call("secret", "/ai.cluster.v1.ClusterControl/RequestWork").await);
    }

    #[test]
    fn the_recovery_token_is_never_operator_or_inference() {
        let mut server = ServerMetadata::new("secret", "op-secret").with_recovery("rec");
        let with = |token: &str| {
            ClientMetadata::with_token(token)
                .call(Request::new(()))
                .unwrap()
        };
        let recovery = server.call(with("rec")).unwrap();
        assert!(require_operator(&recovery).is_err());
        assert!(require_inference(&recovery).is_err());
        let wrong = ClientMetadata::with_token("rec2")
            .call(Request::new(()))
            .unwrap();
        assert!(server.call(wrong).is_err());
    }

    #[test]
    fn the_inference_token_marks_only_inference_and_never_operator() {
        let mut server = ServerMetadata::new("secret", "op-secret").with_inference("inf");
        let with = |token: &str| {
            ClientMetadata::with_token(token)
                .call(Request::new(()))
                .unwrap()
        };
        let inference = server.call(with("inf")).unwrap();
        assert!(require_inference(&inference).is_ok());
        assert!(require_operator(&inference).is_err());
        // The operator also passes the inference check; a node (join token) does not.
        assert!(require_inference(&server.call(with("op-secret")).unwrap()).is_ok());
        assert!(require_inference(&server.call(with("secret")).unwrap()).is_err());
        // Without the token configured, "inf" is just a wrong token.
        let mut plain = ServerMetadata::new("secret", "op-secret");
        assert!(plain.call(with("inf")).is_err());
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
