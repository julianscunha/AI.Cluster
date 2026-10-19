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

pub const PROTOCOL_VERSION: &str = "v1";
pub const VERSION_HEADER: &str = "x-ai-cluster-protocol-version";
pub const CORRELATION_HEADER: &str = "x-ai-cluster-correlation-id";
pub const AUTH_HEADER: &str = "x-ai-cluster-auth";

/// Placeholder auth context.
///
/// ponytail: no authentication exists yet; the real token arrives with the
/// security/trust phase. The header is emitted now so callers can rely on it.
pub const AUTH_CONTEXT: &str = "anonymous";

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

/// Stamps every outgoing call with the protocol version, a fresh correlation id
/// and the auth context.
#[derive(Clone, Debug, Default)]
pub struct ClientMetadata;

impl Interceptor for ClientMetadata {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let metadata = request.metadata_mut();
        metadata.insert(VERSION_HEADER, PROTOCOL_VERSION.parse().expect("ascii"));
        metadata.insert(
            CORRELATION_HEADER,
            next_correlation_id().parse().expect("ascii"),
        );
        metadata.insert(AUTH_HEADER, AUTH_CONTEXT.parse().expect("ascii"));
        Ok(request)
    }
}

/// Rejects calls that do not announce the protocol version this build speaks.
#[derive(Clone, Debug, Default)]
pub struct ServerMetadata;

impl Interceptor for ServerMetadata {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        let version = request
            .metadata()
            .get(VERSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<missing>");
        if version == PROTOCOL_VERSION {
            Ok(request)
        } else {
            Err(Status::failed_precondition(format!(
                "protocol version {version} is not supported; this controller speaks {PROTOCOL_VERSION}"
            )))
        }
    }
}

/// Connects to a controller with the metadata interceptors attached.
pub async fn connect_controller(
    dst: impl Into<String>,
) -> Result<MetadataClient, Box<dyn std::error::Error>> {
    let channel = tonic::transport::Endpoint::from_shared(dst.into())?
        .connect()
        .await?;
    Ok(ClusterControlClient::with_interceptor(
        channel,
        ClientMetadata,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_stamps_required_metadata() {
        let mut interceptor = ClientMetadata;
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
            AUTH_CONTEXT
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
        let mut interceptor = ClientMetadata;
        let first = interceptor.call(Request::new(())).unwrap();
        let second = interceptor.call(Request::new(())).unwrap();
        assert_ne!(
            first.metadata().get(CORRELATION_HEADER),
            second.metadata().get(CORRELATION_HEADER)
        );
    }

    #[test]
    fn server_accepts_matching_version_and_rejects_others() {
        let mut stamp = ClientMetadata;
        let stamped = stamp.call(Request::new(())).unwrap();
        assert!(ServerMetadata.call(stamped).is_ok());

        let mut wrong = Request::new(());
        wrong
            .metadata_mut()
            .insert(VERSION_HEADER, "v0".parse().unwrap());
        assert_eq!(
            ServerMetadata.call(wrong).unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );

        // Missing header is rejected too.
        assert!(ServerMetadata.call(Request::new(())).is_err());
    }
}
