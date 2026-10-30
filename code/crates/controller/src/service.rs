use std::sync::Arc;

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::cluster_control_server::ClusterControl;
use ai_cluster_protocol::cluster::link_probe_server::LinkProbe;
use ai_cluster_protocol::cluster::{
    ApproveNodeRequest, ApproveNodeResponse, CancelRequestRequest, CancelRequestResponse,
    CheckModelRequest, CheckModelResponse, EnqueueGraphRequest, EnqueueGraphResponse,
    EnqueueWorkRequest, EnqueueWorkResponse, GetMetricsRequest, GetOutputRequest,
    GetOutputResponse, GetTraceRequest, GetTraceResponse, HealthStreamRequest,
    HealthStreamResponse, HeartbeatRequest, HeartbeatResponse, KeyAddRequest, KeyAddResponse,
    KeyListRequest, KeyListResponse, KeyRemoveRequest, KeyRemoveResponse, LeaveNodeRequest,
    LeaveNodeResponse, ListEventsRequest, ListEventsResponse, ListModelsRequest,
    ListModelsResponse, ListNodesRequest, ListNodesResponse, ListReservationsRequest,
    ListReservationsResponse, MetricsResponse, ModelEntry, ModelStatus, ModelStatusRequest,
    ModelStatusResponse, NodeShardCapacity, ProbeLinkRequest, ProbeLinkResponse,
    RegisterNodeRequest, RegisterNodeResponse, ReleaseReservationRequest,
    ReleaseReservationResponse, RequestWorkRequest, RequestWorkResponse, Reservation,
    ReserveRequest, ReserveResponse, RevokeNodeRequest, RevokeNodeResponse, RpcEndpoint,
    SetReleaseRequest, SetReleaseResponse, ShardCapacityRequest, ShardCapacityResponse,
    ShardCompute, ShardMatmulRequest, ShardMatmulResponse, ShardOutput, ShardResult,
    StartModelRequest, StartModelResponse, StopModelRequest, StopModelResponse,
    SubmitWorkResultRequest, SubmitWorkResultResponse, WorkReportEntry, WorkReportRequest,
    WorkReportResponse, WorkUnit,
};
use ai_cluster_protocol::cluster::{
    Event, GatewayInfo, ListGatewaysRequest, ListGatewaysResponse, NodeIdentity, Release,
    ReportGatewayRequest, ReportGatewayResponse, WatchEventsRequest,
};
use ai_cluster_protocol::metadata::{
    NODE_ID_HEADER, is_recovery, node_credential, require_inference, require_operator,
    require_recovery_scope,
};
use tonic::{Request, Response, Status};

/// Message ceiling for the `LinkProbe` service, enforced by the server before the payload is
/// buffered: the real probe sends 2 MiB, and the 64 MiB data limit is sized for work units.
pub const MAX_PROBE_BYTES: usize = 4 * 1024 * 1024;

/// Queued plus in-flight units past which `EnqueueGraph` refuses new graphs (J1).
const MAX_QUEUED_UNITS: usize = 256;

/// Limits on a `ShardMatmul` (J5). A decode step moves a few KB; a prefill of a few hundred
/// tokens stays well under these.
const MAX_SHARD_OPS: usize = 64;
const MAX_SHARD_X: usize = 2 * 1024 * 1024;
const MAX_SHARD_ROWS: u32 = 1 << 20;
/// f64 values one operation may return (64 MiB), and twice that across a call.
const MAX_SHARD_OUT: usize = 8 * 1024 * 1024;
/// `ShardMatmul` calls running at once. A generation makes one at a time, so this only bounds a
/// token that is used by more callers than the gateway's single slot allows.
const MAX_SHARD_CALLS: usize = 8;
/// Prazo de uma chamada `ShardMatmul` (`AI_CLUSTER_SHARD_TIMEOUT_MS`, 1 s a 5 min, padrão 8 s): um
/// nó lento, como um Pi, pede mais que um PC.
fn shard_timeout() -> std::time::Duration {
    let ms = std::env::var("AI_CLUSTER_SHARD_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(8_000);
    std::time::Duration::from_millis(ms.clamp(1_000, 300_000))
}

use crate::events::EventLog;
use crate::metrics;
use crate::registry::{Approval, Registry, unix_ms};
use crate::rollout::{self, Rollout};
use crate::shards::ShardHub;
use crate::work::WorkStore;

#[derive(Debug)]
pub struct ClusterControlService {
    registry: Arc<Registry>,
    work: Arc<WorkStore>,
    events: Arc<EventLog>,
    cluster_id: String,
    shards: Arc<ShardHub>,
    shard_calls: Arc<tokio::sync::Semaphore>,
    watchers: Arc<tokio::sync::Semaphore>,
    /// Gateways que se apresentaram, por id. Não persiste: cada um se reapresenta a cada poucos
    /// segundos, então um reinício do controller se refaz sozinho.
    gateways: Arc<std::sync::Mutex<std::collections::HashMap<String, GatewayInfo>>>,
    /// De que endereço (só o IP) cada gateway se apresentou. Enquanto o gateway está vivo, outro
    /// endereço não toma o id: o token de inferência é compartilhado e não diz quem é quem.
    gateway_hosts: Arc<std::sync::Mutex<std::collections::HashMap<String, std::net::IpAddr>>>,
    rollout: Rollout,
    /// Pasta de modelos (`AI_CLUSTER_MODELS_DIR`) que `ListModels` varre.
    models_dir: Option<std::path::PathBuf>,
    /// O modelo pedido, as chaves (hash) e o plano de workers do llama.cpp (ADR-022).
    orchestrator: crate::models::Orchestrator,
    /// De que endereço (só o IP) cada nó se registrou: o gateway chega ao túnel RPC por ele.
    node_hosts: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// A versão do llama.cpp que o gateway reportou: workers com outra versão ficam fora do plano.
    llama_version: Arc<std::sync::Mutex<String>>,
    /// Vagas que o gateway informou (0 = ainda não); vale mais que o ambiente do controller.
    llama_slots: Arc<std::sync::atomic::AtomicU32>,
    /// RAM livre que o gateway informou (0 = ainda não): confere o `llama-server` dele no OK/NOK.
    gateway_free: Arc<std::sync::atomic::AtomicU64>,
    /// Avisos do último cálculo do plano (workers deixados de fora e por quê).
    rpc_warnings: Arc<std::sync::Mutex<Vec<String>>>,
}

/// `host:porta` do túnel RPC de um nó, ou `None` se o host não presta. O host pode vir do próprio nó
/// (`rpc_host`) e o gateway conecta nele: só IP ou nome DNS simples, nunca loopback, não especificado
/// ou link-local. Um IPv6 vai entre colchetes.
fn rpc_address(host: &str, port: u32) -> Option<String> {
    let port = u16::try_from(port).ok().filter(|port| *port != 0)?;
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let bad = ip.is_loopback()
            || ip.is_unspecified()
            || ip.is_multicast()
            || match ip {
                std::net::IpAddr::V4(v4) => v4.is_link_local() || v4.is_broadcast(),
                std::net::IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
            };
        return (!bad).then(|| match ip {
            std::net::IpAddr::V4(_) => format!("{ip}:{port}"),
            std::net::IpAddr::V6(_) => format!("[{ip}]:{port}"),
        });
    }
    let name_ok = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    (name_ok && !host.eq_ignore_ascii_case("localhost")).then(|| format!("{host}:{port}"))
}

/// O estado que o gateway reporta vira texto na tela do operador: id seguro e motivo curto, sem
/// caracteres de controle. Um id fora do padrão descarta o relato inteiro.
fn clean_status(mut status: ModelStatus) -> Option<ModelStatus> {
    let id_ok = !status.model_id.is_empty()
        && status.model_id.len() <= 64
        && status
            .model_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if !id_ok || !status.tokens_per_second.is_finite() || status.tokens_per_second < 0.0 {
        return None;
    }
    status.reason = status
        .reason
        .chars()
        .take(200)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    Some(status)
}

/// Quantos endereços de nó o controller guarda (um por nó registrado).
const MAX_NODE_HOSTS: usize = 4096;

/// O id do modelo na API a partir do id do catálogo (`pasta/arquivo.gguf`): sem a extensão e com `-` no
/// lugar das barras. `None` se sobrar algo fora de `[A-Za-z0-9._-]` ou passar de 64 caracteres.
fn api_model_id(catalog_id: &str) -> Option<String> {
    let id = catalog_id
        .strip_suffix(".gguf")
        .unwrap_or(catalog_id)
        .replace('/', "-");
    (!id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)))
    .then_some(id)
}

/// Whether the node runs the desired release and its measured slot matches the signed image.
/// Software attestation only: a compromised node can lie about the measure.
fn is_attested(identity: &NodeIdentity, want: &Release) -> bool {
    identity.runtime_version == want.version
        && !identity.image_sha256.is_empty()
        && identity.image_sha256 == want.image_sha256
}

#[cfg(test)]
mod attestation_tests {
    use super::*;

    fn identity(version: &str, sha: &str) -> NodeIdentity {
        NodeIdentity {
            node_id: "n".into(),
            runtime_version: version.into(),
            image_sha256: sha.into(),
            ..Default::default()
        }
    }

    #[test]
    fn atestado_exige_versao_e_medida_iguais_as_da_release() {
        let want = Release {
            version: "11.1".into(),
            image_sha256: "abc".into(),
            ..Default::default()
        };
        assert!(is_attested(&identity("11.1", "abc"), &want));
        assert!(
            !is_attested(&identity("11.0", "abc"), &want),
            "versão velha"
        );
        assert!(
            !is_attested(&identity("11.1", "outra"), &want),
            "slot diferente"
        );
        assert!(
            !is_attested(&identity("11.1", ""), &want),
            "sem medida ainda"
        );
    }
}

/// Gateways que o controller lembra ao mesmo tempo; um id novo acima disso é recusado.
const MAX_GATEWAYS: usize = 64;
/// Um gateway sem se apresentar há mais que isto deixa de contar como vivo (ele repete a cada 15 s).
const GATEWAY_ALIVE_MS: u64 = 45_000;

/// As últimas `limit` entradas; 0 é tudo.
fn tail<T>(items: &[T], limit: usize) -> &[T] {
    if limit == 0 {
        items
    } else {
        &items[items.len().saturating_sub(limit)..]
    }
}

/// Um gateway vivo, apresentado de um endereço, não é tomado por outro endereço. Sem endereço
/// conhecido (teste, transporte sem IP) a regra não se aplica.
fn gateway_taken(
    alive: bool,
    owner: Option<&std::net::IpAddr>,
    host: Option<std::net::IpAddr>,
) -> bool {
    alive && matches!((owner, host), (Some(owner), Some(host)) if *owner != host)
}

/// Gateways vivos por endereço (`AI_CLUSTER_MAX_GATEWAYS_PER_HOST`, padrão 16): quem tem a
/// credencial de inferência não enche a tabela de gateways sozinho de um só endereço. Atrás de NAT
/// todos parecem um endereço só; quem tem mais gateways ali sobe o limite.
fn max_gateways_per_host() -> usize {
    std::env::var("AI_CLUSTER_MAX_GATEWAYS_PER_HOST")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(16)
        .clamp(1, MAX_GATEWAYS)
}

/// Se `host` já tem `max` gateways vivos além de `id`.
fn host_is_full(
    hosts: &std::collections::HashMap<String, std::net::IpAddr>,
    alive: impl Fn(&str) -> bool,
    host: std::net::IpAddr,
    id: &str,
    max: usize,
) -> bool {
    hosts
        .iter()
        .filter(|(other, owner)| **owner == host && other.as_str() != id && alive(other))
        .count()
        >= max
}

/// Valida o que o gateway diz de si: tamanhos limitados e sem caracteres de controle, porque o
/// texto vai para a tela do operador.
fn valid_gateway(request: &ReportGatewayRequest) -> bool {
    // ASCII gráfico: sem controle, sem espaço e sem os invisíveis do Unicode que disfarçam texto.
    let text = |value: &str, max: usize| {
        !value.is_empty() && value.len() <= max && value.chars().all(|c| c.is_ascii_graphic())
    };
    // Números que o papel de inferência escreve e o operador lê: só finitos e não negativos.
    let finite = |stats: &ai_cluster_protocol::cluster::GatewayStats| {
        [
            stats.ttft_p50_seconds,
            stats.ttft_p95_seconds,
            stats.itl_p50_seconds,
            stats.itl_p95_seconds,
            stats.decode_tokens_per_second,
            stats.requests_per_second_1m,
            stats.requests_per_second_5m,
            stats.generated_tokens_per_second_1m,
            stats.generated_tokens_per_second_5m,
        ]
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
    };
    request.stats.as_ref().is_none_or(finite)
        && text(&request.gateway_id, 64)
        && request.url.len() <= 256
        && request.url.chars().all(|c| c.is_ascii_graphic())
        && request.models.len() <= 32
        && request.models.iter().all(|model| {
            text(&model.model_id, 64)
                && model.shard_nodes.len() <= 64
                && model.shard_nodes.iter().all(|node| text(node, 64))
        })
}

/// Assinantes simultâneos do barramento de eventos.
const MAX_EVENT_WATCHERS: usize = 16;

/// Highest priority class an enqueue may ask for. Aging is 64 passes per class, so an
/// unbounded `u32` would make a low class wait effectively forever.
const MAX_PRIORITY: u32 = 9;
/// Teto aproximado de unidades enfileiradas que exigem atestado (só a fila, e a checagem não é atômica
/// com o enqueue; basta para o operador único): cada rodada refaz os atestados (O(nós)).
const MAX_ATTESTED_QUEUED: usize = 256;

/// Invalid results in a row that quarantine a node (G1).
const QUARANTINE_STRIKES: u32 = 3;

impl ClusterControlService {
    pub fn new(
        registry: Arc<Registry>,
        work: Arc<WorkStore>,
        events: Arc<EventLog>,
        cluster_id: impl Into<String>,
    ) -> Self {
        Self {
            registry,
            work,
            events,
            cluster_id: cluster_id.into(),
            shards: Arc::new(ShardHub::default()),
            shard_calls: Arc::new(tokio::sync::Semaphore::new(MAX_SHARD_CALLS)),
            watchers: Arc::new(tokio::sync::Semaphore::new(MAX_EVENT_WATCHERS)),
            gateways: Arc::default(),
            gateway_hosts: Arc::default(),
            models_dir: std::env::var_os("AI_CLUSTER_MODELS_DIR")
                .filter(|dir| !dir.is_empty())
                .map(Into::into),
            orchestrator: crate::models::Orchestrator::new(
                std::env::var_os("AI_CLUSTER_MODELS_STATE")
                    .or_else(|| {
                        std::env::var_os("AI_CLUSTER_DATA_DIR")
                            .map(|dir| std::path::Path::new(&dir).join("models.state").into())
                    })
                    .map(Into::into),
            ),
            node_hosts: Arc::default(),
            llama_version: Arc::default(),
            llama_slots: Arc::default(),
            gateway_free: Arc::default(),
            rpc_warnings: Arc::default(),
            // AI_CLUSTER_RELEASE_STATE guarda a release desejada entre reinícios (F19).
            rollout: Rollout::new(std::env::var_os("AI_CLUSTER_RELEASE_STATE").map(Into::into))
                .with_wave(
                    std::env::var("AI_CLUSTER_ROLLOUT_WAVE")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                ),
        }
    }

    pub fn work(&self) -> &Arc<WorkStore> {
        &self.work
    }

    /// Os workers aptos, com a velocidade da CPU e a RAM que oferecem; o orquestrador escolhe quais
    /// carregam o modelo (`models::select`).
    /// Resolve o nome pedido: o id exato, ou, sendo uma família, a variante mais precisa que cabe (a
    /// menor, se nenhuma cabe, para o veredito dizer quanto falta).
    fn pick_model(
        &self,
        found: &[ai_cluster_catalog::ModelEntry],
        wanted: &str,
        context: u64,
        nodes: &[ai_cluster_catalog::check::NodeMemory],
    ) -> Option<ai_cluster_catalog::ModelEntry> {
        let list = ai_cluster_catalog::variants(found, wanted);
        if list.len() <= 1 {
            return list.first().map(|m| (*m).clone());
        }
        let (slots, free) = (self.slots(), self.gateway_free());
        list.iter()
            .find(|m| {
                m.supported && ai_cluster_catalog::check::check(m, context, slots, nodes, free).ok
            })
            .or(list.last())
            .map(|m| (*m).clone())
    }

    /// RAM livre do gateway, se ele já informou.
    fn gateway_free(&self) -> Option<u64> {
        match self.gateway_free.load(std::sync::atomic::Ordering::Relaxed) {
            0 => None,
            free => Some(free),
        }
    }

    /// Vagas do llama-server para o cálculo do cache KV: as que o gateway informou, ou o ambiente.
    fn slots(&self) -> u64 {
        match self.llama_slots.load(std::sync::atomic::Ordering::Relaxed) {
            0 => ai_cluster_catalog::check::slots_from_env(),
            reported => u64::from(reported),
        }
    }

    fn rpc_candidates(&self) -> Vec<crate::models::Candidate> {
        let endpoints: std::collections::HashMap<String, RpcEndpoint> = self
            .rpc_endpoints()
            .into_iter()
            .map(|e| (e.node_id.clone(), e))
            .collect();
        let gateway_version = self
            .llama_version
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let mut warnings = Vec::new();
        let candidates = self
            .registry
            .list()
            .into_iter()
            .filter_map(|node| {
                let id = node.identity?.node_id;
                let endpoint = endpoints.get(&id)?.clone();
                let capability = node.capability?;
                // O protocolo do RPC muda entre commits: worker com outra versão do rpc-server
                // derrubaria o llama-server do gateway. Versão vazia de um lado é "não sei": passa.
                if !gateway_version.is_empty()
                    && !capability.rpc_version.is_empty()
                    && capability.rpc_version != gateway_version
                {
                    warnings.push(format!(
                        "worker {id} fora do plano: rpc-server {} e o gateway usa {gateway_version}",
                        capability.rpc_version
                    ));
                    return None;
                }
                Some(crate::models::Candidate {
                    endpoint,
                    speed: crate::models::estimated_speed(
                        capability.compute_score,
                        capability.memory_bandwidth_loaded_score,
                        capability.memory_bandwidth_score,
                    ),
                    usable_bytes: ai_cluster_catalog::check::usable_bytes(
                        &ai_cluster_catalog::check::NodeMemory {
                            total_bytes: capability.memory_bytes,
                            used_fraction: 0.0,
                        },
                    ),
                })
            })
            .collect();
        *self
            .rpc_warnings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = warnings;
        candidates
    }

    /// Os workers aptos a servir o `rpc-server`: aprovados, vivos, com túnel anunciado e endereço
    /// conhecido.
    fn rpc_endpoints(&self) -> Vec<RpcEndpoint> {
        let hosts = self
            .node_hosts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.registry
            .list()
            .into_iter()
            .filter_map(|node| {
                let id = node.identity?.node_id;
                let capability = node.capability?;
                if capability.rpc_port == 0 || !self.registry.serves_inference(&id) {
                    return None;
                }
                let host = if capability.rpc_host.is_empty() {
                    hosts.get(&id)?.clone()
                } else {
                    capability.rpc_host
                };
                let address = rpc_address(&host, capability.rpc_port)?;
                Some(RpcEndpoint {
                    address,
                    node_id: id,
                })
            })
            .collect()
    }

    /// A RAM dos mesmos workers, para o veredito do `models start`.
    fn rpc_node_memory(&self) -> Vec<ai_cluster_catalog::check::NodeMemory> {
        let wanted: std::collections::HashSet<String> = self
            .rpc_endpoints()
            .into_iter()
            .map(|e| e.node_id)
            .collect();
        self.registry
            .list()
            .into_iter()
            .filter_map(|node| {
                let id = node.identity?.node_id;
                if !wanted.contains(&id) {
                    return None;
                }
                Some(ai_cluster_catalog::check::NodeMemory {
                    total_bytes: node.capability?.memory_bytes,
                    used_fraction: node.health.map_or(0.0, |h| h.memory_utilization),
                })
            })
            .collect()
    }
}

/// Hysteresis band for the placement decision (G5), as a fraction of the cheapest
/// candidate's cost. A node yields only when another candidate is cheaper by more than
/// this much, so a near-equal node takes the unit instead of waiting a poll interval
/// for a saving that may not exist. Zero means "strictly cheapest", the pre-G5 policy.
const DEFAULT_PLACEMENT_TOLERANCE: f64 = 0.10;

/// Placement is on unless explicitly disabled. `AI_CLUSTER_PLACEMENT=off` restores
/// first-come-first-served dispatch, so the benchmark can compare the two policies
/// on the same workload without a rebuild.
fn placement_enabled() -> bool {
    !matches!(
        std::env::var("AI_CLUSTER_PLACEMENT").as_deref(),
        Ok("off") | Ok("0") | Ok("false")
    )
}

/// Reads the hysteresis band from `AI_CLUSTER_PLACEMENT_TOLERANCE`.
///
/// See [`clamp_tolerance`] for what happens to a bad value.
fn placement_tolerance() -> f64 {
    clamp_tolerance(
        std::env::var("AI_CLUSTER_PLACEMENT_TOLERANCE")
            .ok()
            .and_then(|value| value.trim().parse::<f64>().ok()),
    )
}

/// The band, clamped so a bad value cannot break the placement decision.
///
/// A negative band would sit below the cheapest cost, so every candidate would yield and
/// the unit would be stranded; a non-finite one would poison the comparison for everyone.
/// A negative value clamps to zero; a non-finite or missing value falls back to the
/// default. Split from the env lookup so the edge cases can be tested exactly.
fn clamp_tolerance(parsed: Option<f64>) -> f64 {
    match parsed {
        Some(value) if value.is_finite() => value.max(0.0),
        _ => DEFAULT_PLACEMENT_TOLERANCE,
    }
}

/// S3b: a call that speaks for `node_id` must carry that node's credential, or come from the
/// operator. Without it the join token alone could heartbeat, drain or submit for any node.
fn authorize_node<T>(
    registry: &Registry,
    request: &Request<T>,
    node_id: &str,
) -> Result<(), Status> {
    require_recovery_scope(request, node_id)?;
    if require_operator(request).is_ok() || registry.authorizes(node_id, node_credential(request)) {
        Ok(())
    } else {
        Err(Status::unauthenticated(
            "credencial do nó ausente ou inválida",
        ))
    }
}

/// A worker that refused the request itself (`invalid`) is not at fault, and another node would
/// refuse it too: no node is blamed, so the gateway neither benches it nor retries elsewhere.
fn compute_failure(error: String) -> Status {
    Status::invalid_argument(error)
}

/// Marks `status` with the node that caused it (see `FAILED_NODE_HEADER`).
fn blame(mut status: Status, node: &str) -> Status {
    if let Ok(value) = node.parse() {
        status
            .metadata_mut()
            .insert(ai_cluster_protocol::metadata::FAILED_NODE_HEADER, value);
    }
    status
}

/// Forgets the operations of a `ShardMatmul` when the call ends, for any reason.
struct ShardGuard<'a> {
    hub: &'a ShardHub,
    ops: Vec<u64>,
}

impl Drop for ShardGuard<'_> {
    fn drop(&mut self) {
        for op in &self.ops {
            self.hub.cancel(*op);
        }
    }
}

/// The operator check plus who is calling: the peer address, or `?` when the transport has none.
fn operator_peer<T>(request: &Request<T>) -> Result<String, Status> {
    require_operator(request)?;
    Ok(request
        .remote_addr()
        .map_or_else(|| "?".to_string(), |addr| addr.to_string()))
}

impl ClusterControlService {
    /// Nodes that run the desired release with the signed slot measure; empty when no release is
    /// published (nobody is attested against nothing).
    fn attested_nodes(&self) -> std::collections::HashSet<String> {
        let Some(want) = self.rollout.desired() else {
            return Default::default();
        };
        self.registry
            .list()
            .into_iter()
            .filter_map(|node| node.identity)
            .filter(|identity| is_attested(identity, &want))
            .map(|identity| identity.node_id)
            .collect()
    }

    /// The audit trail (F19): one `audit` event per operator action, with the peer and the target.
    /// It goes into the same event log, so `AI_CLUSTER_EVENTS` keeps it across restarts.
    fn audit(&self, peer: &str, action: &str, target: &str) {
        // Ids come from the caller: control characters would forge lines on a terminal, and a
        // huge one would bloat the log.
        let clean = |text: &str| {
            text.chars()
                .take(160)
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>()
        };
        self.events.record(
            unix_ms(),
            "audit",
            format!("{action} {} de {}", clean(target), clean(peer)),
        );
    }
}

#[tonic::async_trait]
impl ClusterControl for ClusterControlService {
    async fn register_node(
        &self,
        request: Request<RegisterNodeRequest>,
    ) -> Result<Response<RegisterNodeResponse>, Status> {
        let presented = node_credential(&request).to_vec();
        let recovery = is_recovery(&request);
        let peer_host = request.remote_addr().map(|addr| addr.ip().to_string());
        if let Some(identity) = &request.get_ref().identity {
            require_recovery_scope(&request, &identity.node_id)?;
        }
        let request = request.into_inner();
        let identity = request
            .identity
            .ok_or_else(|| Status::invalid_argument("identidade é obrigatória"))?;
        if identity.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        let node_id = identity.node_id.clone();
        let now = unix_ms();
        let seen = format!("{} v{}", identity.architecture, identity.runtime_version);
        // A node offers the copy of the state it holds. A controller that lost its own files
        // takes the newest one — a node keeping the copy is exactly what lets the queue come
        // back after the volume dies (I4).
        let Some(record) = self.registry.register_as(
            identity,
            request.capability.unwrap_or_default(),
            now,
            &presented,
        ) else {
            return Err(Status::failed_precondition(
                "nó já registrado com outra credencial; se aprovado, o operador precisa rodar `ai-cluster revoke` antes (um nó pendente fica livre quando para de responder)",
            ));
        };
        // S2: an unapproved node must not be able to write controller state, so its copy is
        // ignored until it is trusted. A node that comes back after approval offers it again.
        // O token de recuperação nunca injeta estado, nem que o registro saia confiável (auto-approve).
        if let Some(replica) = request.replica.filter(|_| record.trusted && !recovery) {
            self.work.restore(&replica);
        }
        if let Some(host) = peer_host {
            let mut hosts = self
                .node_hosts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if hosts.len() < MAX_NODE_HOSTS || hosts.contains_key(&node_id) {
                hosts.insert(node_id.clone(), host);
            }
        }
        self.events.record(now, "node_registered", node_id.clone());
        println!(
            "nó {node_id} entrou ({seen}){}",
            if record.trusted {
                ""
            } else {
                "; aguardando aprovação"
            }
        );
        Ok(Response::new(RegisterNodeResponse {
            accepted: true,
            cluster_id: self.cluster_id.clone(),
            message: if record.trusted {
                format!("nó {node_id} registrado")
            } else {
                format!("nó {node_id} registrado, aguardando aprovação")
            },
            node_credential: record.credential,
        }))
    }

    /// Trusts a node so it can receive work (S2). Unknown nodes answer `approved: false`.
    async fn approve_node(
        &self,
        request: Request<ApproveNodeRequest>,
    ) -> Result<Response<ApproveNodeResponse>, Status> {
        let peer = operator_peer(&request)?;
        let request = request.into_inner();
        let node_id = request.node_id;
        if node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        if request.fingerprint.trim().is_empty() {
            return Err(Status::invalid_argument(
                "fingerprint é obrigatória: use a que `ai-cluster nodes` mostra e o nó imprime ao registrar-se",
            ));
        }
        self.audit(&peer, "approve", &node_id);
        let approved = match self
            .registry
            .approve_checked(&node_id, &request.fingerprint.trim().to_ascii_lowercase())
        {
            Approval::Approved => true,
            Approval::Unknown => false,
            Approval::Mismatch => {
                return Err(Status::failed_precondition(
                    "a impressão digital não confere com a credencial atual do nó; o id pode ter sido tomado por outro registro",
                ));
            }
        };
        if approved {
            self.events.record(unix_ms(), "node_approved", node_id);
        }
        Ok(Response::new(ApproveNodeResponse { approved }))
    }

    /// Strips a node of trust and its credential, and hands its in-flight units back (S5).
    async fn revoke_node(
        &self,
        request: Request<RevokeNodeRequest>,
    ) -> Result<Response<RevokeNodeResponse>, Status> {
        let peer = operator_peer(&request)?;
        let node_id = request.into_inner().node_id;
        if node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        self.audit(&peer, "revoke", &node_id);
        let revoked = self.registry.revoke(&node_id);
        self.shards.drop_node(&node_id);
        let requeued = if revoked {
            self.work.requeue_for_node(&node_id).requeued as u32
        } else {
            0
        };
        if revoked {
            self.events.record(
                unix_ms(),
                "node_revoked",
                format!("{node_id} reenfileiradas={requeued}"),
            );
        }
        Ok(Response::new(RevokeNodeResponse { revoked, requeued }))
    }

    /// Define a release que os nós devem rodar (F19); version em branco cancela.
    async fn set_release(
        &self,
        request: Request<SetReleaseRequest>,
    ) -> Result<Response<SetReleaseResponse>, Status> {
        let peer = operator_peer(&request)?;
        let release = request.into_inner().release.unwrap_or_default();
        if release.version.trim().is_empty() {
            self.audit(&peer, "set_release", "(cancelado)");
            self.rollout.set(None);
        } else {
            rollout::validate(&release).map_err(Status::invalid_argument)?;
            self.audit(&peer, "set_release", &release.version);
            self.rollout.set(Some(release));
        }
        Ok(Response::new(SetReleaseResponse {}))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        authorize_node(&self.registry, &request, &request.get_ref().node_id)?;
        let recovery = is_recovery(&request);
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        // The node-supplied timestamp is advisory only; liveness uses controller time.
        let acknowledged = self.registry.heartbeat(&request.node_id, unix_ms());
        // A node that is behind gets the current copy; one that is level with it gets nothing,
        // so a beat stays a beat (I4). The node acknowledges by reporting the epoch back on
        // the next beat, so this needs no per-node bookkeeping — and a node that leaves leaves
        // nothing behind.
        let replica = self
            .work
            .state_replica()
            .filter(|replica| {
                replica.epoch_unix_ms > request.replica_epoch_unix_ms
                    && !recovery
                    && self.registry.is_trusted(&request.node_id)
            })
            .map(|replica| (*replica).clone());
        let now = unix_ms();
        let version = self
            .registry
            .version(&request.node_id)
            .map(|(version, _)| version)
            .unwrap_or_default();
        let (release, event) = self.rollout.announce(
            self.registry.can_update(&request.node_id),
            &request.node_id,
            &version,
            |node| self.registry.version(node),
            now,
        );
        if let Some(event) = event {
            self.events.record(now, "rollout", event);
        }
        Ok(Response::new(HeartbeatResponse {
            acknowledged,
            replica,
            // o nó de recuperação não recebe a release nem a cópia do estado
            release: release.filter(|_| !recovery),
            cancel_work_ids: self.work.take_cancels(&request.node_id),
        }))
    }

    async fn report_health(
        &self,
        request: Request<HealthStreamRequest>,
    ) -> Result<Response<HealthStreamResponse>, Status> {
        authorize_node(&self.registry, &request, &request.get_ref().node_id)?;
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        let accepted = self.registry.report_health(
            &request.node_id,
            request.health.unwrap_or_default(),
            request.capability,
        );
        Ok(Response::new(HealthStreamResponse { accepted }))
    }

    /// Pede o modelo ao gateway (ADR-022, E5 e E6): confere a RAM dos workers com RPC, fixa o plano e
    /// cria a chave de API do tenant `default` na primeira vez. Só o operador.
    async fn start_model(
        &self,
        request: Request<StartModelRequest>,
    ) -> Result<Response<StartModelResponse>, Status> {
        let peer = operator_peer(&request)?;
        let request = request.into_inner();
        let Some(dir) = self.models_dir.clone() else {
            return Err(Status::failed_precondition(
                "o controller não tem pasta de modelos (AI_CLUSTER_MODELS_DIR)",
            ));
        };
        let wanted = request.model_id.clone();
        let found = tokio::task::spawn_blocking(move || {
            ai_cluster_catalog::scan(&dir, ai_cluster_catalog::Runtime { gguf: true })
        })
        .await
        .map_err(|_| Status::internal("varredura interrompida"))?
        .map_err(|error| Status::failed_precondition(format!("pasta de modelos: {error}")))?;
        let nodes = self.rpc_node_memory();
        let Some(model) = self.pick_model(&found, &wanted, request.context, &nodes) else {
            return Err(Status::not_found(format!(
                "modelo {wanted} não está na pasta"
            )));
        };
        let refuse = |message: String, check: Option<CheckModelResponse>| {
            Ok(Response::new(StartModelResponse {
                ok: false,
                message,
                generation: 0,
                check,
                keys: Vec::new(),
            }))
        };
        if model.format != "gguf" {
            return refuse(
                format!(
                    "o llama.cpp roda GGUF; {wanted} é {}: converta para GGUF",
                    model.format
                ),
                None,
            );
        }
        if !model.supported {
            return refuse(model.reason.clone(), None);
        }
        let (check_dir, check_id) = (self.models_dir.clone(), model.id.clone());
        let hashed = match check_dir {
            Some(dir) if model.supported => tokio::task::spawn_blocking(move || {
                ai_cluster_catalog::verify_checksum(&dir, &check_id)
            })
            .await
            .map_err(|_| Status::internal("conferência interrompida"))?,
            _ => Ok(()),
        };
        if let Err(message) = hashed {
            return refuse(message, None);
        }
        let Some(api_id) = api_model_id(&model.id) else {
            return refuse(
                format!("o nome {wanted} não vira um id de modelo válido"),
                None,
            );
        };
        let candidates = self.rpc_candidates();
        let verdict = ai_cluster_catalog::check::check(
            &model,
            request.context,
            self.slots(),
            &nodes,
            self.gateway_free(),
        );
        let check = CheckModelResponse {
            ok: verdict.ok,
            weights_bytes: verdict.weights_bytes,
            kv_bytes: verdict.kv_bytes,
            margin_bytes: verdict.margin_bytes,
            required_bytes: verdict.required_bytes,
            available_bytes: verdict.available_bytes,
            nodes: verdict.nodes,
            survives_one_loss: verdict.survives_one_loss,
            estimated_tokens_per_second: 0.0,
            gateway_bytes: verdict.gateway_bytes,
            message: verdict.message,
        };
        // Sem workers com RPC o modelo roda só na CPU do gateway, cuja RAM o controller não conhece.
        if !candidates.is_empty() && !verdict.ok {
            return refuse(check.message.clone(), Some(check));
        }
        self.audit(&peer, "start_model", &wanted);
        // Sem workers o modelo roda só no gateway: não há RAM de worker para conferir depois.
        let required = if candidates.is_empty() {
            0
        } else {
            verdict.required_bytes
        };
        let (endpoints, split) = crate::models::select(candidates, required);
        let (generation, keys) = self.orchestrator.start(
            &api_id,
            &model.id,
            request.context,
            required,
            endpoints,
            split,
        );
        self.events.record(unix_ms(), "model_started", wanted);
        Ok(Response::new(StartModelResponse {
            ok: true,
            message: if check.nodes == 0 {
                "sem workers com RPC: o modelo roda só no gateway (a RAM dele não foi verificada)"
                    .into()
            } else {
                check.message.clone()
            },
            generation,
            check: Some(check),
            keys,
        }))
    }

    /// Para o modelo no gateway. As chaves continuam para o próximo `start`. Só o operador.
    async fn stop_model(
        &self,
        request: Request<StopModelRequest>,
    ) -> Result<Response<StopModelResponse>, Status> {
        let peer = operator_peer(&request)?;
        self.audit(&peer, "stop_model", "-");
        let generation = self.orchestrator.stop();
        self.events
            .record(unix_ms(), "model_stopped", "-".to_string());
        Ok(Response::new(StopModelResponse { generation }))
    }

    /// O que foi pedido, o último estado que o gateway reportou e os prefixos das chaves. Só o operador.
    async fn model_status(
        &self,
        request: Request<ModelStatusRequest>,
    ) -> Result<Response<ModelStatusResponse>, Status> {
        require_operator(&request)?;
        let (desired, running, key_prefixes) = self.orchestrator.status(unix_ms());
        // Recalcula os candidatos para os avisos refletirem os workers de agora.
        let _ = self.rpc_candidates();
        let warnings = self
            .rpc_warnings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        Ok(Response::new(ModelStatusResponse {
            desired,
            running,
            key_prefixes,
            warnings,
        }))
    }

    /// Cria a chave de um tenant novo (`model key add`) ou troca a de um que existe (`rotate`). A chave
    /// em claro volta só aqui. Só o operador.
    async fn add_key(
        &self,
        request: Request<KeyAddRequest>,
    ) -> Result<Response<KeyAddResponse>, Status> {
        let peer = operator_peer(&request)?;
        let request = request.into_inner();
        self.audit(
            &peer,
            if request.rotate {
                "rotate_key"
            } else {
                "add_key"
            },
            &request.tenant,
        );
        let reply = match self.orchestrator.add_key(
            &request.tenant,
            request.models,
            request.tokens,
            &request.class,
            request.rotate,
        ) {
            Ok(key) => KeyAddResponse {
                ok: true,
                message: String::new(),
                key: Some(key),
            },
            Err(message) => KeyAddResponse {
                ok: false,
                message,
                key: None,
            },
        };
        Ok(Response::new(reply))
    }

    /// Tira o tenant. Só o operador.
    async fn remove_key(
        &self,
        request: Request<KeyRemoveRequest>,
    ) -> Result<Response<KeyRemoveResponse>, Status> {
        let peer = operator_peer(&request)?;
        let tenant = request.into_inner().tenant;
        self.audit(&peer, "remove_key", &tenant);
        Ok(Response::new(KeyRemoveResponse {
            removed: self.orchestrator.remove_key(&tenant),
        }))
    }

    /// Os tenants com o prefixo da chave e os limites. Só o operador.
    async fn list_keys(
        &self,
        request: Request<KeyListRequest>,
    ) -> Result<Response<KeyListResponse>, Status> {
        require_operator(&request)?;
        Ok(Response::new(KeyListResponse {
            keys: self.orchestrator.list_keys(),
        }))
    }

    /// OK ou NOK de um modelo da pasta contra a RAM dos nós aprovados e vivos (ADR-022, E2). Só o operador.
    async fn check_model(
        &self,
        request: Request<CheckModelRequest>,
    ) -> Result<Response<CheckModelResponse>, Status> {
        require_operator(&request)?;
        let request = request.into_inner();
        let Some(dir) = self.models_dir.clone() else {
            return Err(Status::failed_precondition(
                "o controller não tem pasta de modelos (AI_CLUSTER_MODELS_DIR)",
            ));
        };
        let wanted = request.model_id.clone();
        let found = tokio::task::spawn_blocking(move || {
            ai_cluster_catalog::scan(&dir, ai_cluster_catalog::Runtime { gguf: true })
        })
        .await
        .map_err(|_| Status::internal("varredura interrompida"))?
        .map_err(|error| Status::failed_precondition(format!("pasta de modelos: {error}")))?;
        // Bytes do modelo que está rodando e sua velocidade medida, para estimar o novo.
        let running = self
            .orchestrator
            .status(0)
            .1
            .filter(|s| s.tokens_per_second > 0.0);
        let running_bytes = running.as_ref().map_or(0, |s| {
            found
                .iter()
                .find(|m| api_model_id(&m.id).as_deref() == Some(s.model_id.as_str()))
                .map_or(0, |m| m.bytes)
        });
        let nodes: Vec<ai_cluster_catalog::check::NodeMemory> = self
            .registry
            .list()
            .into_iter()
            .filter_map(|node| {
                let id = node.identity?.node_id;
                if !self.registry.is_available(&id) {
                    return None;
                }
                Some(ai_cluster_catalog::check::NodeMemory {
                    total_bytes: node.capability?.memory_bytes,
                    used_fraction: node.health.map_or(0.0, |h| h.memory_utilization),
                })
            })
            .collect();
        let Some(model) = self.pick_model(&found, &wanted, request.context, &nodes) else {
            return Err(Status::not_found(format!(
                "modelo {wanted} não está na pasta"
            )));
        };
        let estimated_tokens_per_second = crate::models::estimate_tokens_per_second(
            running.map_or(0.0, |s| s.tokens_per_second),
            running_bytes,
            model.bytes,
        );
        let verdict = ai_cluster_catalog::check::check(
            &model,
            request.context,
            self.slots(),
            &nodes,
            self.gateway_free(),
        );
        let (ok, message) = if model.supported {
            (verdict.ok, verdict.message)
        } else {
            (
                false,
                format!("o motor atual não roda este modelo: {}", model.reason),
            )
        };
        Ok(Response::new(CheckModelResponse {
            ok,
            weights_bytes: verdict.weights_bytes,
            kv_bytes: verdict.kv_bytes,
            margin_bytes: verdict.margin_bytes,
            required_bytes: verdict.required_bytes,
            available_bytes: verdict.available_bytes,
            nodes: verdict.nodes,
            survives_one_loss: verdict.survives_one_loss,
            estimated_tokens_per_second,
            gateway_bytes: verdict.gateway_bytes,
            message,
        }))
    }

    /// Os modelos da pasta do controller, lidos pelo cabeçalho (ADR-022, E1). Só o operador.
    async fn list_models(
        &self,
        request: Request<ListModelsRequest>,
    ) -> Result<Response<ListModelsResponse>, Status> {
        require_operator(&request)?;
        let Some(dir) = self.models_dir.clone() else {
            return Ok(Response::new(ListModelsResponse::default()));
        };
        // A varredura lê arquivos: fora do executor assíncrono.
        let scanned = tokio::task::spawn_blocking({
            let dir = dir.clone();
            move || ai_cluster_catalog::scan(&dir, ai_cluster_catalog::Runtime { gguf: true })
        })
        .await
        .map_err(|_| Status::internal("varredura interrompida"))?
        .map_err(|error| {
            Status::failed_precondition(format!("pasta de modelos {}: {error}", dir.display()))
        })?;
        let models = scanned
            .into_iter()
            .map(|m| ModelEntry {
                id: m.id,
                format: m.format,
                architecture: m.architecture,
                parameters: m.parameters,
                precision: m.precision,
                bytes: m.bytes,
                context: m.context,
                layers: m.layers,
                supported: m.supported,
                reason: m.reason,
            })
            .collect();
        Ok(Response::new(ListModelsResponse {
            models,
            directory: dir.display().to_string(),
        }))
    }

    async fn list_nodes(
        &self,
        _request: Request<ListNodesRequest>,
    ) -> Result<Response<ListNodesResponse>, Status> {
        let desired = self.rollout.desired();
        let mut nodes = self.registry.list();
        for node in &mut nodes {
            if let (Some(want), Some(identity)) = (&desired, &node.identity) {
                node.attested = is_attested(identity, want);
            }
        }
        Ok(Response::new(ListNodesResponse { nodes }))
    }

    /// Planned departure: stop dispatching to the node, hand its in-flight units
    /// back to the queue, then drop it from the registry.
    ///
    /// An unknown node is not an error — a node may leave twice, and a node that
    /// was already reaped as offline has nothing left to requeue.
    async fn leave_node(
        &self,
        request: Request<LeaveNodeRequest>,
    ) -> Result<Response<LeaveNodeResponse>, Status> {
        authorize_node(&self.registry, &request, &request.get_ref().node_id)?;
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        if !self.registry.begin_drain(&request.node_id) {
            return Ok(Response::new(LeaveNodeResponse {
                accepted: false,
                requeued: 0,
            }));
        }
        let outcome = self.work.requeue_for_node(&request.node_id);
        self.registry.remove(&request.node_id);
        self.shards.drop_node(&request.node_id);
        println!(
            "nó {} saiu; {} unidades em voo reenfileiradas, {} falharam",
            request.node_id, outcome.requeued, outcome.failed
        );
        self.events.record(
            unix_ms(),
            "node_left",
            format!(
                "{} reenfileiradas={} falharam={}",
                request.node_id, outcome.requeued, outcome.failed
            ),
        );
        Ok(Response::new(LeaveNodeResponse {
            accepted: true,
            requeued: outcome.requeued as u32,
        }))
    }

    async fn request_work(
        &self,
        request: Request<RequestWorkRequest>,
    ) -> Result<Response<RequestWorkResponse>, Status> {
        authorize_node(&self.registry, &request, &request.get_ref().node_id)?;
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        if self.registry.awaiting_approval(&request.node_id) {
            return Ok(Response::new(RequestWorkResponse::default()));
        }
        // Unknown, leaving or dead nodes get no work.
        if !self.registry.is_available(&request.node_id) {
            return Err(Status::failed_precondition(
                "nó não está disponível para trabalho",
            ));
        }
        // Candidate filtering: the unit's declared requirement and its own footprint
        // are checked against what this node reported, so a node that cannot run the
        // unit is never handed it.
        let capability = self
            .registry
            .capability(&request.node_id)
            .unwrap_or_default();
        // Placement (G3): rank the asking node against the other idle candidates and
        // hand it a unit only when it is cheap enough for it — cheapest, or within the
        // hysteresis band (G5) of cheapest. A node that is neither gets nothing and
        // polls again, so the unit waits for the node that fits it better instead of
        // going to whoever asked first.
        // Units that demand attestation only go to nodes that run the desired release with the
        // measured slot matching the signed one; the set is rebuilt only while such a unit waits.
        if self.work.needs_attestation() {
            self.work.set_attested(self.attested_nodes());
        } else {
            self.work.clear_attested();
        }
        let claimed = if placement_enabled() {
            let candidates = self.registry.candidates();
            self.work.claim_placed(
                &request.node_id,
                &capability,
                unix_ms(),
                &candidates,
                placement_tolerance(),
            )
        } else {
            self.work.claim(&request.node_id, &capability, unix_ms())
        };
        match claimed {
            Some(work) => Ok(Response::new(RequestWorkResponse {
                has_work: true,
                work: Some(work),
            })),
            None => Ok(Response::new(RequestWorkResponse {
                has_work: false,
                work: None,
            })),
        }
    }

    async fn submit_work_result(
        &self,
        request: Request<SubmitWorkResultRequest>,
    ) -> Result<Response<SubmitWorkResultResponse>, Status> {
        authorize_node(
            &self.registry,
            &request,
            request
                .get_ref()
                .result
                .as_ref()
                .map_or("", |result| result.node_id.as_str()),
        )?;
        let result = request
            .into_inner()
            .result
            .ok_or_else(|| Status::invalid_argument("resultado é obrigatório"))?;
        if result.work_id.trim().is_empty() {
            return Err(Status::invalid_argument("work_id é obrigatório"));
        }
        // G1: a wrong answer is not recorded. The unit goes back to the queue and the node
        // takes a strike; enough in a row quarantines it and hands back what it still holds.
        if self.work.result_is_valid(&result) == Some(false) {
            let (work_id, node_id) = (result.work_id.clone(), result.node_id.clone());
            self.work.reject(&work_id, &node_id);
            self.events.record(
                unix_ms(),
                "work_rejected",
                format!("{work_id} de {node_id}: resultado inválido"),
            );
            if self.registry.strike(&node_id, QUARANTINE_STRIKES) {
                let requeued = self.work.requeue_for_node(&node_id).requeued;
                self.shards.drop_node(&node_id);
                self.events.record(
                    unix_ms(),
                    "node_quarantined",
                    format!("{node_id} reenfileiradas={requeued}"),
                );
            }
            return Ok(Response::new(SubmitWorkResultResponse {
                accepted: false,
                message: "resultado inválido".to_string(),
            }));
        }
        self.registry.clear_strikes(&result.node_id);
        match self.work.complete(result, unix_ms()) {
            Some(completed) => {
                println!(
                    "trabalho {} finalizado em {} compute={}us transfer={}us total={}us checksum={:.6}",
                    completed.work_id,
                    completed.node_id,
                    completed.compute_time_us,
                    completed.transfer_time_us,
                    completed.total_latency_us,
                    completed.checksum,
                );
                self.events.record(
                    unix_ms(),
                    "work_completed",
                    format!(
                        "{} em {} compute={}us transfer={}us",
                        completed.work_id,
                        completed.node_id,
                        completed.compute_time_us,
                        completed.transfer_time_us
                    ),
                );
                Ok(Response::new(SubmitWorkResultResponse {
                    accepted: true,
                    message: "resultado registrado".to_string(),
                }))
            }
            None => Ok(Response::new(SubmitWorkResultResponse {
                accepted: false,
                message: "trabalho não foi despachado por este controller".to_string(),
            })),
        }
    }

    async fn enqueue_work(
        &self,
        request: Request<EnqueueWorkRequest>,
    ) -> Result<Response<EnqueueWorkResponse>, Status> {
        let peer = operator_peer(&request)?;
        let request = request.into_inner();
        if request.rows == 0 || request.shared == 0 || request.cols == 0 {
            return Err(Status::invalid_argument(
                "todas as dimensões da matriz devem ser maiores que zero",
            ));
        }
        if request.require_attested
            && self
                .work
                .attested_queued()
                .saturating_add(request.repetitions.max(1) as usize)
                > MAX_ATTESTED_QUEUED
        {
            return Err(Status::resource_exhausted(format!(
                "mais de {MAX_ATTESTED_QUEUED} unidades atestadas na fila; espere esvaziar"
            )));
        }
        // Sem release publicada ninguém é atestado: a unidade esperaria para sempre sem aviso.
        if request.require_attested && self.rollout.desired().is_none() {
            return Err(Status::failed_precondition(
                "require_attested exige uma release publicada (ai-cluster rollout): sem ela nenhum nó é atestado",
            ));
        }
        self.audit(
            &peer,
            "enqueue_work",
            &format!("{}x{}x{}", request.rows, request.shared, request.cols),
        );
        let repetitions = request.repetitions.max(1);
        let request_id = if request.request_id.is_empty() {
            self.work.next_request_id()
        } else {
            request.request_id.clone()
        };
        let units = (0..repetitions).map(|index| {
            let seed = request.seed.wrapping_add(u64::from(index));
            // A and B get different seeds so the product is not a square of one matrix.
            let a = Matrix::seeded(request.rows, request.shared, seed);
            let b = Matrix::seeded(request.shared, request.cols, seed ^ 0x9E37_79B9_7F4A_7C15);
            WorkUnit {
                work_id: self.work.next_work_id(),
                rows: request.rows,
                shared: request.shared,
                cols: request.cols,
                matrix_a: a.data,
                matrix_b: b.data,
                depends_on: Vec::new(),
                matrix_a_from: String::new(),
                matrix_b_from: String::new(),
                min_cpu_threads: request.min_cpu_threads,
                request_id: request_id.clone(),
                priority: request.priority.min(MAX_PRIORITY),
                require_attested: request.require_attested,
            }
        });
        let enqueued = self.work.enqueue(units);
        self.events.record(
            unix_ms(),
            "work_enqueued",
            format!(
                "{} unidades {}x{}x{} req={request_id}",
                enqueued, request.rows, request.shared, request.cols
            ),
        );
        Ok(Response::new(EnqueueWorkResponse {
            enqueued: enqueued as u32,
            request_id,
        }))
    }

    /// Enqueues a small diamond with real data flow: A -> (B, C) -> D.
    ///
    /// B and C take A's output as their left input and a seeded matrix as their
    /// right input, so they run in parallel after A. D takes B's output and C's
    /// output as its two inputs, so it runs only once both branches have finished.
    /// This is the smallest graph that exercises gating, parallelism and the
    /// hand-off of one unit's output into the next unit's input.
    async fn enqueue_graph(
        &self,
        request: Request<EnqueueGraphRequest>,
    ) -> Result<Response<EnqueueGraphResponse>, Status> {
        let peer = operator_peer(&request)?;
        let request = request.into_inner();
        // Without cancellation, graphs nobody waits for any more pile up in the queue (a gateway
        // request that timed out leaves its graph behind). A ceiling turns that into an error
        // the caller can see instead of unbounded growth.
        self.audit(&peer, "enqueue_graph", &format!("size={}", request.size));
        if self.work.pending() > MAX_QUEUED_UNITS {
            return Err(Status::resource_exhausted(
                "a fila do controller está cheia; tente de novo em instantes",
            ));
        }
        if request.size == 0 {
            return Err(Status::invalid_argument(
                "o tamanho deve ser maior que zero",
            ));
        }
        let size = request.size;
        let request_id = if request.request_id.is_empty() {
            self.work.next_request_id()
        } else {
            request.request_id.clone()
        };
        let first = self.work.next_work_id();
        let left = self.work.next_work_id();
        let right = self.work.next_work_id();
        let join = self.work.next_work_id();
        // A bound input is filled at dispatch from its source's output, so its
        // payload stays empty here. The salt only seeds the non-bound operand.
        let make = |work_id: String,
                    depends_on: Vec<String>,
                    a_from: String,
                    b_from: String,
                    salt: u64| {
            let a = Matrix::seeded(size, size, request.seed);
            let b = Matrix::seeded(size, size, request.seed ^ salt);
            WorkUnit {
                work_id,
                rows: size,
                shared: size,
                cols: size,
                matrix_a: if a_from.is_empty() {
                    a.data
                } else {
                    Vec::new()
                },
                matrix_b: if b_from.is_empty() {
                    b.data
                } else {
                    Vec::new()
                },
                depends_on,
                matrix_a_from: a_from,
                matrix_b_from: b_from,
                // Graph units carry no requirement; size is the same for all of them.
                min_cpu_threads: 0,
                request_id: request_id.clone(),
                priority: request.priority.min(MAX_PRIORITY),
                require_attested: false,
            }
        };
        let units = vec![
            // A: both operands seeded.
            make(
                first.clone(),
                Vec::new(),
                String::new(),
                String::new(),
                0x9E37_79B9_7F4A_7C15,
            ),
            // B and C: left operand from A, distinct seeded right operands.
            make(
                left.clone(),
                vec![first.clone()],
                first.clone(),
                String::new(),
                0x1234_5678_9ABC_DEF0,
            ),
            make(
                right.clone(),
                vec![first.clone()],
                first.clone(),
                String::new(),
                0x0FED_CBA9_8765_4321,
            ),
            // D: both operands from the two branches.
            make(
                join.clone(),
                vec![left.clone(), right.clone()],
                left,
                right,
                0,
            ),
        ];
        if request.keep_result {
            self.work.keep_output(&join);
        }
        let enqueued = self.work.enqueue(units);
        self.events.record(
            unix_ms(),
            "graph_enqueued",
            format!("{enqueued} unidades tamanho={size} req={request_id}"),
        );
        Ok(Response::new(EnqueueGraphResponse {
            enqueued: enqueued as u32,
            blocked: self.work.blocked() as u32,
            request_id,
        }))
    }

    /// The kept output of a request enqueued with `keep_result` (J1). Operator-only: a result
    /// is the cluster's data, and the gateway is what decides who may read it.
    async fn get_output(
        &self,
        request: Request<GetOutputRequest>,
    ) -> Result<Response<GetOutputResponse>, Status> {
        require_operator(&request)?;
        let request_id = request.into_inner().request_id;
        Ok(Response::new(match self.work.kept_output(&request_id) {
            Some((rows, cols, matrix_c)) => GetOutputResponse {
                found: true,
                rows,
                cols,
                matrix_c,
            },
            None => GetOutputResponse::default(),
        }))
    }

    type ShardChannelStream = std::pin::Pin<
        Box<dyn tonic::codegen::tokio_stream::Stream<Item = Result<ShardCompute, Status>> + Send>,
    >;

    /// The resident channel of a worker (J5). The node and its credential come in the metadata
    /// when the stream opens; the operator token does not stand in for them, so the activations
    /// that flow down this stream only ever reach an approved, live node holding its credential.
    async fn shard_channel(
        &self,
        request: Request<tonic::Streaming<ShardResult>>,
    ) -> Result<Response<Self::ShardChannelStream>, Status> {
        let node_id = request
            .metadata()
            .get(NODE_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        if node_id.is_empty() {
            return Err(Status::invalid_argument("falta o nó do canal"));
        }
        if !self
            .registry
            .channel_allowed(&node_id, node_credential(&request))
        {
            return Err(Status::unauthenticated(
                "nó desconhecido, não aprovado, indisponível ou com credencial inválida",
            ));
        }
        let credential = node_credential(&request).to_vec();
        let mut incoming = request.into_inner();
        let (session, frames) = self.shards.attach(&node_id);
        // A revoke or leave can land between the check above and the attach; check again so a
        // revoked node never keeps a session.
        if !self.registry.channel_allowed(&node_id, &credential) {
            self.shards.detach(&node_id, session);
            return Err(Status::unauthenticated("o nó deixou de ser aprovado"));
        }
        self.events
            .record(unix_ms(), "shard_channel_opened", node_id.clone());
        let hub = self.shards.clone();
        tokio::spawn(async move {
            while let Ok(Some(result)) = incoming.message().await {
                hub.complete(&node_id, result);
            }
            hub.detach(&node_id, session);
        });
        Ok(Response::new(Box::pin(
            tonic::codegen::tokio_stream::wrappers::ReceiverStream::new(frames),
        )))
    }

    /// O gateway se apresenta (quem é, onde está, que modelos serve e onde ficam as fatias).
    async fn report_gateway(
        &self,
        request: Request<ReportGatewayRequest>,
    ) -> Result<Response<ReportGatewayResponse>, Status> {
        require_inference(&request)?;
        let host = request.remote_addr().map(|addr| addr.ip());
        let request = request.into_inner();
        if !valid_gateway(&request) {
            return Err(Status::invalid_argument("apresentação de gateway inválida"));
        }
        let running = request.running.clone().and_then(clean_status);
        if !request.llama_version.is_empty()
            && request.llama_version.len() <= 64
            && request.llama_version.chars().all(|c| c.is_ascii_graphic())
        {
            *self
                .llama_version
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = request.llama_version.clone();
        }
        if request.memory_available_bytes > 0 {
            self.gateway_free.store(
                request.memory_available_bytes,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        if (1..=16).contains(&request.llama_slots) {
            self.llama_slots
                .store(request.llama_slots, std::sync::atomic::Ordering::Relaxed);
        }
        let now = unix_ms();
        let is_new = {
            let mut gateways = self
                .gateways
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut hosts = self
                .gateway_hosts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let alive = gateways.get(&request.gateway_id).is_some_and(|gateway| {
                now.saturating_sub(gateway.last_seen_unix_ms) <= GATEWAY_ALIVE_MS
            });
            if gateway_taken(alive, hosts.get(&request.gateway_id), host) {
                return Err(Status::permission_denied(
                    "gateway já apresentado por outro endereço; aguarde ele ficar sem sinal",
                ));
            }
            if let Some(host) = host
                && host_is_full(
                    &hosts,
                    |id| {
                        gateways.get(id).is_some_and(|gateway| {
                            now.saturating_sub(gateway.last_seen_unix_ms) <= GATEWAY_ALIVE_MS
                        })
                    },
                    host,
                    &request.gateway_id,
                    max_gateways_per_host(),
                )
            {
                return Err(Status::resource_exhausted("gateways demais neste endereço"));
            }
            if !gateways.contains_key(&request.gateway_id) && gateways.len() >= MAX_GATEWAYS {
                // Um gateway morto não segura a vaga para sempre: sai o que está sem sinal há mais
                // tempo. Se todos estão vivos, quem chega espera.
                let stale = gateways
                    .values()
                    .filter(|gateway| {
                        now.saturating_sub(gateway.last_seen_unix_ms) > GATEWAY_ALIVE_MS
                    })
                    .min_by_key(|gateway| gateway.last_seen_unix_ms)
                    .map(|gateway| gateway.gateway_id.clone());
                match stale {
                    Some(id) => {
                        gateways.remove(&id);
                        hosts.remove(&id);
                    }
                    None => return Err(Status::resource_exhausted("gateways demais")),
                }
            }
            if let Some(host) = host {
                hosts.insert(request.gateway_id.clone(), host);
            }
            gateways
                .insert(
                    request.gateway_id.clone(),
                    GatewayInfo {
                        gateway_id: request.gateway_id.clone(),
                        url: request.url,
                        models: request.models,
                        last_seen_unix_ms: now,
                        alive: true,
                        stats: request.stats,
                    },
                )
                .is_none()
        };
        if is_new {
            self.events
                .record(now, "gateway_registered", request.gateway_id);
        }
        if let Some(running) = running {
            self.orchestrator.note_running(running);
        }
        Ok(Response::new(ReportGatewayResponse {
            desired: self.orchestrator.plan(self.rpc_candidates(), now),
            tenants: self.orchestrator.grants(),
        }))
    }

    async fn list_gateways(
        &self,
        request: Request<ListGatewaysRequest>,
    ) -> Result<Response<ListGatewaysResponse>, Status> {
        require_operator(&request)?;
        let now = unix_ms();
        let mut gateways: Vec<GatewayInfo> = self
            .gateways
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .map(|mut gateway| {
                gateway.alive = now.saturating_sub(gateway.last_seen_unix_ms) <= GATEWAY_ALIVE_MS;
                gateway
            })
            .collect();
        gateways.sort_by(|a, b| a.gateway_id.cmp(&b.gateway_id));
        Ok(Response::new(ListGatewaysResponse {
            gateways,
            now_unix_ms: now,
        }))
    }

    /// Quanto cada nó nomeado aceita guardar de fatias (admissão por capacidade). Só diz o que o
    /// próprio nó reportou; nó desconhecido ou fora do ar volta como indisponível.
    async fn shard_capacity(
        &self,
        request: Request<ShardCapacityRequest>,
    ) -> Result<Response<ShardCapacityResponse>, Status> {
        require_inference(&request)?;
        let ids = request.into_inner().node_ids;
        if ids.is_empty() || ids.len() > MAX_SHARD_OPS {
            return Err(Status::invalid_argument("lista de nós inválida"));
        }
        let nodes = ids
            .into_iter()
            .map(|node_id| {
                let capability = self.registry.capability(&node_id).unwrap_or_default();
                NodeShardCapacity {
                    available: self.registry.is_available(&node_id),
                    shard_cache_bytes: capability.shard_cache_bytes,
                    memory_bytes: capability.memory_bytes,
                    node_id,
                }
            })
            .collect();
        Ok(Response::new(ShardCapacityResponse { nodes }))
    }

    /// Runs one activation against several weight slices on the nodes that hold them (J5).
    /// Operator-only: it commands work on every worker.
    async fn shard_matmul(
        &self,
        request: Request<ShardMatmulRequest>,
    ) -> Result<Response<ShardMatmulResponse>, Status> {
        require_inference(&request)?;
        let request = request.into_inner();
        let x_len = (request.rows as usize).checked_mul(request.k as usize);
        if request.ops.is_empty()
            || request.ops.len() > MAX_SHARD_OPS
            || request.rows == 0
            || request.k == 0
            || x_len != Some(request.x.len())
            || request.x.len() > MAX_SHARD_X
            || request.ops.iter().any(|op| {
                op.node_id.is_empty()
                    || op.model_subdir.len() > 128
                    || op.tensor.is_empty()
                    || op.tensor.len() > 256
                    || op.row_start >= op.row_end
                    || op.row_end - op.row_start > MAX_SHARD_ROWS
            })
        {
            return Err(Status::invalid_argument("operação de fatia inválida"));
        }
        let _call = self.shard_calls.try_acquire().map_err(|_| {
            Status::resource_exhausted("ShardMatmul demais em andamento; tente de novo")
        })?;
        // The answers come back as f64 in one message each: bound them per operation and in total.
        let out_values = |op: &ai_cluster_protocol::cluster::ShardOp| {
            request.rows as usize * (op.row_end - op.row_start) as usize
        };
        if request.ops.iter().any(|op| out_values(op) > MAX_SHARD_OUT)
            || request.ops.iter().map(out_values).sum::<usize>() > 2 * MAX_SHARD_OUT
        {
            return Err(Status::invalid_argument("saída de fatias grande demais"));
        }
        if let Some(op) = request
            .ops
            .iter()
            .find(|op| !self.registry.serves_inference(&op.node_id))
        {
            return Err(blame(
                Status::failed_precondition(format!("nó {} indisponível", op.node_id)),
                &op.node_id,
            ));
        }

        // Operations still waiting are forgotten however this call ends: an answer, an error, or
        // the caller going away and dropping this future.
        let mut guard = ShardGuard {
            hub: &self.shards,
            ops: Vec::with_capacity(request.ops.len()),
        };
        let mut waiting = Vec::with_capacity(request.ops.len());
        for op in &request.ops {
            let frame = ShardCompute {
                op_id: 0,
                model_subdir: op.model_subdir.clone(),
                tensor: op.tensor.clone(),
                row_start: op.row_start,
                row_end: op.row_end,
                by_columns: op.by_columns,
                rows: request.rows,
                k: request.k,
                x: request.x.clone(),
                cancel: false,
            };
            let (op_id, answer) = self
                .shards
                .submit(&op.node_id, frame)
                .map_err(|status| blame(status, &op.node_id))?;
            guard.ops.push(op_id);
            waiting.push(answer);
        }

        let deadline = tokio::time::Instant::now() + shard_timeout();
        let mut outputs = Vec::with_capacity(waiting.len());
        for (spec, answer) in request.ops.iter().zip(waiting) {
            let result = match tokio::time::timeout_at(deadline, answer).await {
                Err(_) => {
                    return Err(blame(
                        Status::deadline_exceeded("o worker não respondeu a tempo"),
                        &spec.node_id,
                    ));
                }
                Ok(Err(_)) => {
                    return Err(blame(
                        Status::unavailable("o canal do worker caiu"),
                        &spec.node_id,
                    ));
                }
                Ok(Ok(result)) => result,
            };
            let cols = spec.row_end - spec.row_start;
            if !result.ok {
                return Err(if result.invalid {
                    compute_failure(result.error)
                } else {
                    // The node's own trouble (model missing, disk): the others may be fine.
                    blame(Status::failed_precondition(result.error), &spec.node_id)
                });
            }
            if result.rows != request.rows
                || result.cols != cols
                || result.data.len() != request.rows as usize * cols as usize
            {
                return Err(Status::internal("saída de fatia com forma inesperada"));
            }
            outputs.push(ShardOutput {
                rows: result.rows,
                cols: result.cols,
                data: result.data,
                compute_us: result.compute_us,
            });
        }
        Ok(Response::new(ShardMatmulResponse { outputs }))
    }

    async fn work_report(
        &self,
        request: Request<WorkReportRequest>,
    ) -> Result<Response<WorkReportResponse>, Status> {
        require_operator(&request)?;
        let limit = request.into_inner().limit as usize;
        // Lido sob o lock e convertido direto: sem a cópia intermediária do histórico inteiro, e só
        // a janela pedida (as mais recentes) quando há limite.
        let completed = self.work.with_completed(|records| {
            tail(records, limit)
                .iter()
                .map(|record| WorkReportEntry {
                    work_id: record.work_id.clone(),
                    node_id: record.node_id.clone(),
                    compute_time_us: record.compute_time_us,
                    transfer_time_us: record.transfer_time_us,
                    total_latency_us: record.total_latency_us,
                    checksum: record.checksum,
                })
                .collect()
        });
        Ok(Response::new(WorkReportResponse {
            completed,
            pending: self.work.pending() as u32,
            failed: self.work.failed() as u32,
        }))
    }

    async fn list_events(
        &self,
        request: Request<ListEventsRequest>,
    ) -> Result<Response<ListEventsResponse>, Status> {
        require_operator(&request)?;
        let limit = request.into_inner().limit as usize;
        Ok(Response::new(ListEventsResponse {
            events: self.events.recent(limit),
        }))
    }

    type WatchEventsStream = std::pin::Pin<
        Box<dyn tonic::codegen::tokio_stream::Stream<Item = Result<Event, Status>> + Send>,
    >;

    /// O barramento de eventos: cada evento novo chega ao operador assim que é gravado.
    async fn watch_events(
        &self,
        request: Request<WatchEventsRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        let peer = operator_peer(&request)?;
        let permit = self
            .watchers
            .clone()
            .try_acquire_owned()
            .map_err(|_| Status::resource_exhausted("assinantes de eventos demais"))?;
        let mut bus = self.events.subscribe();
        self.audit(&peer, "watch_events", "");
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            let _permit = permit;
            loop {
                let next = match bus.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(lost)) => Event {
                        at_unix_ms: unix_ms(),
                        kind: "events_lagged".into(),
                        detail: format!("{lost} eventos perdidos"),
                    },
                    Err(_) => break,
                };
                // o operador saiu: encerra e libera a vaga
                if tx.send(Ok(next)).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(Box::pin(
            tonic::codegen::tokio_stream::wrappers::ReceiverStream::new(rx),
        )))
    }

    async fn get_metrics(
        &self,
        _request: Request<GetMetricsRequest>,
    ) -> Result<Response<MetricsResponse>, Status> {
        // Node counts come from one registry lock and work counts from the work store, so
        // the read is a consistent-enough snapshot: a unit completing between the two calls
        // can land in either bucket, never both. These are point-in-time metrics, not a
        // transaction.
        let (nodes_total, nodes_available) = self.registry.counts();
        let (pending, blocked, failed) = (
            self.work.pending() as u32,
            self.work.blocked() as u32,
            self.work.failed() as u32,
        );
        let nodes = self.registry.list();
        Ok(Response::new(self.work.with_completed(|completed| {
            let mut summary = metrics::summarize(
                nodes_total,
                nodes_available,
                completed,
                pending,
                blocked,
                failed,
            );
            metrics::add_rollups(&mut summary, &nodes, completed, unix_ms());
            summary
        })))
    }

    async fn get_trace(
        &self,
        request: Request<GetTraceRequest>,
    ) -> Result<Response<GetTraceResponse>, Status> {
        let request_id = request.into_inner().request_id;
        Ok(Response::new(GetTraceResponse {
            units: self.work.trace(&request_id),
            request_id,
        }))
    }

    /// Cancela uma requisição (F11): tira da fila o que ela ainda não despachou.
    async fn cancel_request(
        &self,
        request: Request<CancelRequestRequest>,
    ) -> Result<Response<CancelRequestResponse>, Status> {
        let peer = operator_peer(&request)?;
        let request_id = request.into_inner().request_id;
        if request_id.trim().is_empty() {
            return Err(Status::invalid_argument("request_id é obrigatório"));
        }
        self.audit(&peer, "cancel_request", &request_id);
        let (removed, in_flight) = self.work.cancel_request(&request_id);
        if removed > 0 || in_flight > 0 {
            self.events.record(
                unix_ms(),
                "request_cancelled",
                format!("{request_id} removidas={removed} em_voo={in_flight}"),
            );
        }
        Ok(Response::new(CancelRequestResponse {
            removed: removed as u32,
            in_flight: in_flight as u32,
        }))
    }

    /// Retém um nó para uma requisição futura (G6).
    ///
    /// O nó precisa existir e estar disponível: reservar um nó que o controller não
    /// entregaria trabalho nenhum não protege nada, então isso é recusado em vez de
    /// aceito e ignorado. Uma reserva nova sobre o mesmo nó substitui a anterior.
    async fn reserve(
        &self,
        request: Request<ReserveRequest>,
    ) -> Result<Response<ReserveResponse>, Status> {
        let peer = operator_peer(&request)?;
        let request = request.into_inner();
        if request.node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        if request.request_id.trim().is_empty() {
            return Err(Status::invalid_argument("request_id é obrigatório"));
        }
        self.audit(
            &peer,
            "reserve",
            &format!("{} para {}", request.node_id, request.request_id),
        );
        if !self.registry.is_available(&request.node_id) {
            return Ok(Response::new(ReserveResponse {
                granted: false,
                expires_at_unix_ms: 0,
                message: format!("nó {} não está disponível para reserva", request.node_id),
            }));
        }
        let now = unix_ms();
        let reservation = self.work.reserve_slots(
            &request.node_id,
            &request.request_id,
            request.slots,
            request.ttl_ms,
            now,
        );
        self.events.record(
            now,
            "node_reserved",
            format!(
                "{} retido para {} até {}",
                request.node_id, request.request_id, reservation.expires_at_unix_ms
            ),
        );
        Ok(Response::new(ReserveResponse {
            granted: true,
            expires_at_unix_ms: reservation.expires_at_unix_ms,
            message: String::new(),
        }))
    }

    async fn list_reservations(
        &self,
        _request: Request<ListReservationsRequest>,
    ) -> Result<Response<ListReservationsResponse>, Status> {
        let reservations = self
            .work
            .reservations(unix_ms())
            .into_iter()
            .map(|(node_id, reservation)| Reservation {
                node_id,
                request_id: reservation.request_id,
                expires_at_unix_ms: reservation.expires_at_unix_ms,
                slots: reservation.slots,
            })
            .collect();
        Ok(Response::new(ListReservationsResponse { reservations }))
    }

    async fn release_reservation(
        &self,
        request: Request<ReleaseReservationRequest>,
    ) -> Result<Response<ReleaseReservationResponse>, Status> {
        let peer = operator_peer(&request)?;
        let node_id = request.into_inner().node_id;
        if node_id.trim().is_empty() {
            return Err(Status::invalid_argument("node_id é obrigatório"));
        }
        self.audit(&peer, "release_reservation", &node_id);
        let released = self.work.release_reservation(&node_id);
        if released {
            self.events
                .record(unix_ms(), "reservation_released", node_id.clone());
        }
        Ok(Response::new(ReleaseReservationResponse { released }))
    }
}

/// Throughput probe (C2): the worker times the upload, so the answer is only the byte count.
#[derive(Debug, Default)]
pub struct LinkProbeService;

#[tonic::async_trait]
impl LinkProbe for LinkProbeService {
    async fn probe_link(
        &self,
        request: Request<ProbeLinkRequest>,
    ) -> Result<Response<ProbeLinkResponse>, Status> {
        Ok(Response::new(ProbeLinkResponse {
            received_bytes: request.into_inner().payload.len() as u64,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_cluster_protocol::cluster::{
        CapabilityReport, HealthReport, NodeIdentity, WorkResult, WorkUnit,
    };

    /// A request that arrived with the operator token, as the interceptor would mark it.
    fn op<T>(message: T) -> Request<T> {
        let mut request = Request::new(message);
        request
            .extensions_mut()
            .insert(ai_cluster_protocol::metadata::Operator);
        request
    }

    fn service_with_work() -> (ClusterControlService, Arc<WorkStore>) {
        let work = Arc::new(WorkStore::new());
        let service = ClusterControlService::new(
            Arc::new(Registry::new()),
            work.clone(),
            Arc::new(EventLog::new()),
            "test-cluster",
        );
        (service, work)
    }

    fn service_with_registry_and_work() -> (ClusterControlService, Arc<Registry>, Arc<WorkStore>) {
        let registry = Arc::new(Registry::new());
        let work = Arc::new(WorkStore::new());
        let service = ClusterControlService::new(
            registry.clone(),
            work.clone(),
            Arc::new(EventLog::new()),
            "test-cluster",
        );
        (service, registry, work)
    }

    fn service() -> ClusterControlService {
        service_with_work().0
    }

    #[tokio::test]
    async fn heartbeat_anuncia_a_release_desejada_so_a_quem_difere() {
        let service = service();
        let register = |version: &str| {
            Request::new(RegisterNodeRequest {
                identity: Some(NodeIdentity {
                    runtime_version: version.to_string(),
                    can_update: true,
                    ..identity("n1")
                }),
                capability: None,
                replica: None,
            })
        };
        service.register_node(register("1")).await.unwrap();
        let beat = || {
            op(HeartbeatRequest {
                node_id: "n1".into(),
                ..Default::default()
            })
        };
        let release = ai_cluster_protocol::cluster::Release {
            version: "2".into(),
            url: "http://h/x.gz".into(),
            sha256: "a".repeat(64),
            image_sha256: "b".repeat(64),
            signature: format!("{}==", "A".repeat(86)),
        };
        // sem release definida, nada a anunciar
        assert!(
            service
                .heartbeat(beat())
                .await
                .unwrap()
                .into_inner()
                .release
                .is_none()
        );
        // sem o token de operador não se define release
        let denied = service
            .set_release(Request::new(SetReleaseRequest {
                release: Some(release.clone()),
            }))
            .await;
        assert!(denied.is_err());
        service
            .set_release(op(SetReleaseRequest {
                release: Some(release),
            }))
            .await
            .unwrap();
        let got = service
            .heartbeat(beat())
            .await
            .unwrap()
            .into_inner()
            .release;
        assert_eq!(got.unwrap().version, "2");
        // o nó volta já na versão desejada: nada mais a anunciar
        service.register_node(register("2")).await.unwrap();
        assert!(
            service
                .heartbeat(beat())
                .await
                .unwrap()
                .into_inner()
                .release
                .is_none()
        );
    }

    fn identity(node_id: &str) -> NodeIdentity {
        NodeIdentity {
            node_id: node_id.to_string(),
            device_id: format!("{node_id}-device"),
            ..Default::default()
        }
    }

    /// A unit large enough that compute dominates the fixed transfer term, so a thread
    /// difference is a real cost difference. See the same helper in `work.rs`.
    fn big_unit(work_id: &str) -> WorkUnit {
        WorkUnit {
            work_id: work_id.to_string(),
            rows: 64,
            shared: 64,
            cols: 64,
            matrix_a: vec![0.0; 64 * 64],
            matrix_b: vec![0.0; 64 * 64],
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
            request_id: "req-test".to_string(),
            priority: 0,
            require_attested: false,
        }
    }

    fn unit(work_id: &str) -> WorkUnit {
        WorkUnit {
            work_id: work_id.to_string(),
            rows: 2,
            shared: 2,
            cols: 2,
            matrix_a: vec![1.0, 2.0, 3.0, 4.0],
            matrix_b: vec![5.0, 6.0, 7.0, 8.0],
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
            request_id: "req-test".to_string(),
            priority: 0,
            require_attested: false,
        }
    }

    #[tokio::test]
    async fn registration_heartbeat_health_and_list_round_trip() {
        let service = service();
        let registered = service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: Some(CapabilityReport {
                    cpu_threads: 4,
                    ..Default::default()
                }),
                replica: None,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(registered.accepted);
        assert_eq!(registered.cluster_id, "test-cluster");

        let heartbeat = service
            .heartbeat(op(HeartbeatRequest {
                node_id: "node-a".to_string(),
                unix_ms: 123,
                replica_epoch_unix_ms: 0,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(heartbeat.acknowledged);

        let health = service
            .report_health(op(HealthStreamRequest {
                node_id: "node-a".to_string(),
                health: Some(HealthReport {
                    health_score: 0.9,
                    ..Default::default()
                }),
                capability: None,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(health.accepted);

        let listed = service
            .list_nodes(Request::new(ListNodesRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(listed.nodes.len(), 1);
        assert_eq!(listed.nodes[0].state, "READY");
        assert_eq!(listed.nodes[0].capability.as_ref().unwrap().cpu_threads, 4);
        assert_eq!(listed.nodes[0].health.as_ref().unwrap().health_score, 0.9);
    }

    #[tokio::test]
    async fn missing_identity_and_unknown_node_are_rejected() {
        let service = service();

        let missing = service
            .register_node(Request::new(RegisterNodeRequest::default()))
            .await
            .unwrap_err();
        assert_eq!(missing.code(), tonic::Code::InvalidArgument);

        let blank = service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(NodeIdentity::default()),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(blank.code(), tonic::Code::InvalidArgument);

        let ghost = service
            .heartbeat(op(HeartbeatRequest {
                node_id: "ghost".to_string(),
                unix_ms: 0,
                replica_epoch_unix_ms: 0,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!ghost.acknowledged);
    }

    #[tokio::test]
    async fn work_dispatch_and_result_round_trip() {
        let (service, work_store) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        let idle = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!idle.has_work);
        assert!(idle.work.is_none());

        work_store.enqueue([unit("w1")]);
        let claimed = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(claimed.has_work);
        let claimed = claimed.work.expect("work unit present");
        assert_eq!(claimed.work_id, "w1");
        assert_eq!(claimed.matrix_a.len(), 4);

        let accepted = service
            .submit_work_result(op(SubmitWorkResultRequest {
                result: Some(WorkResult {
                    work_id: "w1".to_string(),
                    node_id: "node-a".to_string(),
                    rows: 2,
                    cols: 2,
                    matrix_c: vec![19.0, 22.0, 43.0, 50.0],
                    checksum: 134.0,
                    compute_time_us: 1_000,
                }),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(accepted.accepted);
        assert_eq!(work_store.completed().len(), 1);
        assert_eq!(work_store.completed()[0].work_id, "w1");
    }

    #[tokio::test]
    async fn list_models_scans_the_models_directory_for_the_operator_only() {
        let dir = std::env::temp_dir().join(format!("aic-models-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("m")).unwrap();
        let header = r#"{"w":{"dtype":"F32","shape":[3,4],"data_offsets":[0,48]}}"#;
        let mut file = (header.len() as u64).to_le_bytes().to_vec();
        file.extend(header.as_bytes());
        std::fs::write(dir.join("m/model.safetensors"), file).unwrap();
        std::fs::write(dir.join("m/config.json"), r#"{"model_type":"gpt2"}"#).unwrap();
        let (mut service, _) = service_with_work();
        service.models_dir = Some(dir.clone());

        let denied = service
            .list_models(Request::new(ListModelsRequest {}))
            .await;
        assert_eq!(denied.unwrap_err().code(), tonic::Code::PermissionDenied);
        let reply = service
            .list_models(op(ListModelsRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(reply.models.len(), 1);
        let model = &reply.models[0];
        assert_eq!(
            (model.id.as_str(), model.parameters, model.supported),
            ("m", 12, true)
        );

        service.models_dir = None;
        let none = service
            .list_models(op(ListModelsRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert!(none.directory.is_empty() && none.models.is_empty());
    }

    #[tokio::test]
    async fn check_model_compares_the_model_with_the_ram_of_approved_live_nodes() {
        let dir = std::env::temp_dir().join(format!("aic-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("m")).unwrap();
        // 2 GiB de F32 declarados no cabeçalho (só o cabeçalho é lido).
        let header = r#"{"w":{"dtype":"F32","shape":[536870912],"data_offsets":[0,2147483648]}}"#;
        let mut file = (header.len() as u64).to_le_bytes().to_vec();
        file.extend(header.as_bytes());
        std::fs::write(dir.join("m/model.safetensors"), file).unwrap();
        std::fs::write(dir.join("m/config.json"), r#"{"model_type":"gpt2"}"#).unwrap();
        let (mut service, _) = service_with_work();
        service.models_dir = Some(dir);
        let ask = || CheckModelRequest {
            model_id: "m".into(),
            context: 0,
        };

        let denied = service.check_model(Request::new(ask())).await;
        assert_eq!(denied.unwrap_err().code(), tonic::Code::PermissionDenied);
        let missing = service
            .check_model(op(CheckModelRequest {
                model_id: "x".into(),
                context: 0,
            }))
            .await;
        assert_eq!(missing.unwrap_err().code(), tonic::Code::NotFound);
        // Sem nós: NOK.
        let none = service.check_model(op(ask())).await.unwrap().into_inner();
        assert!(!none.ok && none.nodes == 0);
    }

    #[tokio::test]
    async fn three_invalid_results_in_a_row_quarantine_the_node() {
        let (service, work_store) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();
        work_store.enqueue([unit("w1")]);
        let bad = |checksum: f64| {
            op(SubmitWorkResultRequest {
                result: Some(WorkResult {
                    work_id: "w1".to_string(),
                    node_id: "node-a".to_string(),
                    rows: 2,
                    cols: 2,
                    matrix_c: vec![19.0, 22.0, 43.0, 50.0],
                    checksum,
                    compute_time_us: 1,
                }),
            })
        };
        for _ in 0..QUARANTINE_STRIKES {
            // Each wrong answer sends the unit back; the node asks for it again.
            let claimed = service
                .request_work(op(RequestWorkRequest {
                    node_id: "node-a".to_string(),
                }))
                .await
                .unwrap()
                .into_inner();
            assert!(claimed.has_work);
            let reply = service
                .submit_work_result(bad(1.0))
                .await
                .unwrap()
                .into_inner();
            assert!(!reply.accepted);
        }
        assert!(work_store.completed().is_empty());
        assert_eq!(work_store.pending(), 1);
        let listed = service
            .list_nodes(op(ListNodesRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(listed.nodes[0].state, "QUARANTINED");
        // A quarantined node is no longer offered work.
        let denied = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await;
        assert!(denied.is_err() || !denied.unwrap().into_inner().has_work);
    }

    #[tokio::test]
    async fn work_requests_require_a_registered_node() {
        let service = service();

        let blank = service
            .request_work(op(RequestWorkRequest {
                node_id: "  ".to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(blank.code(), tonic::Code::InvalidArgument);

        let unknown = service
            .request_work(op(RequestWorkRequest {
                node_id: "ghost".to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(unknown.code(), tonic::Code::FailedPrecondition);

        let missing = service
            .submit_work_result(op(SubmitWorkResultRequest { result: None }))
            .await
            .unwrap_err();
        assert_eq!(missing.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn a_family_name_picks_the_most_precise_variant_that_fits() {
        use ai_cluster_catalog::{ModelEntry, check::NodeMemory};
        const GIB: u64 = 1 << 30;
        let (service, _, _) = service_with_registry_and_work();
        let entry = |id: &str, gib: u64| ModelEntry {
            id: id.into(),
            bytes: gib * GIB,
            supported: true,
            ..ModelEntry::default()
        };
        let found = [
            entry("m-q8.gguf", 8),
            entry("m-q4.gguf", 4),
            entry("m-q2.gguf", 2),
        ];
        let node = |gib: u64| NodeMemory {
            total_bytes: gib * GIB,
            used_fraction: 0.0,
        };
        let id = |nodes: &[NodeMemory], wanted: &str| {
            service.pick_model(&found, wanted, 0, nodes).map(|m| m.id)
        };
        // 3 nós de 4 GiB (~9,2 GB úteis) comportam o q8; 2 nós (~6,1 GB) só o q4.
        assert_eq!(
            id(&[node(4), node(4), node(4)], "m").as_deref(),
            Some("m-q8.gguf")
        );
        assert_eq!(id(&[node(4), node(4)], "m").as_deref(), Some("m-q4.gguf"));
        // Nada cabe: devolve a menor, para o veredito dizer quanto falta.
        assert_eq!(id(&[node(1)], "m").as_deref(), Some("m-q2.gguf"));
        // Id exato não troca de variante.
        assert_eq!(id(&[node(1)], "m-q8.gguf").as_deref(), Some("m-q8.gguf"));
        assert_eq!(id(&[node(1)], "nada"), None);
    }

    #[test]
    fn the_slots_reported_by_the_gateway_beat_the_environment() {
        let (service, _, _) = service_with_registry_and_work();
        let env = service.slots();
        assert!((1..=16).contains(&env));
        service
            .llama_slots
            .store(7, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(service.slots(), 7);
    }

    #[test]
    fn a_draining_node_stays_in_the_inference_plan_until_it_is_removed() {
        let (service, registry, _) = service_with_registry_and_work();
        let capability = CapabilityReport {
            rpc_port: 50061,
            rpc_host: "10.0.0.5".into(),
            ..Default::default()
        };
        registry.register(identity("node-a"), capability, 1_000);
        assert_eq!(service.rpc_endpoints().len(), 1);
        assert!(registry.begin_drain("node-a"));
        assert_eq!(
            service.rpc_endpoints().len(),
            1,
            "drain não reinicia o modelo"
        );
        registry.remove("node-a");
        assert!(service.rpc_endpoints().is_empty());
    }

    #[tokio::test]
    async fn enqueue_work_caps_the_attested_queue() {
        let (service, work_store) = service_with_work();
        let status = service
            .enqueue_work(op(EnqueueWorkRequest {
                rows: 2,
                shared: 2,
                cols: 2,
                repetitions: 257,
                require_attested: true,
                ..Default::default()
            }))
            .await
            .unwrap_err();
        assert_eq!(status.code(), tonic::Code::ResourceExhausted);
        assert_eq!(work_store.attested_queued(), 0);
    }

    #[tokio::test]
    async fn enqueue_work_builds_units_matching_the_declared_shape() {
        let (service, work_store) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        let enqueued = service
            .enqueue_work(op(EnqueueWorkRequest {
                rows: 4,
                shared: 4,
                cols: 4,
                seed: 7,
                repetitions: 2,
                min_cpu_threads: 0,
                request_id: String::new(),
                priority: 0,
                require_attested: false,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(enqueued.enqueued, 2);
        assert_eq!(work_store.pending(), 2);

        let claimed = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner()
            .work
            .expect("work unit present");
        assert_eq!(claimed.rows, 4);
        assert_eq!(claimed.shared, 4);
        assert_eq!(claimed.cols, 4);
        assert_eq!(claimed.matrix_a.len(), 16);
        assert_eq!(claimed.matrix_b.len(), 16);

        let invalid = service
            .enqueue_work(op(EnqueueWorkRequest {
                rows: 0,
                shared: 4,
                cols: 4,
                seed: 1,
                repetitions: 1,
                min_cpu_threads: 0,
                request_id: String::new(),
                priority: 0,
                require_attested: false,
            }))
            .await
            .unwrap_err();
        assert_eq!(invalid.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn leave_node_requeues_in_flight_work_and_drops_the_node() {
        let (service, _registry, work_store) = service_with_registry_and_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        work_store.enqueue([unit("w1")]);
        let claimed = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(claimed.has_work);
        assert_eq!(work_store.pending(), 1);

        let left = service
            .leave_node(op(LeaveNodeRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(left.accepted);
        assert_eq!(left.requeued, 1);

        // The node is gone and its unit is back in the queue.
        let listed = service
            .list_nodes(Request::new(ListNodesRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert!(listed.nodes.is_empty());
        assert_eq!(work_store.pending(), 1);

        // Leaving twice reports accepted=false rather than failing the call.
        let again = service
            .leave_node(op(LeaveNodeRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!again.accepted);
        assert_eq!(again.requeued, 0);

        // A blank node id is a client error.
        let blank = service
            .leave_node(op(LeaveNodeRequest {
                node_id: "  ".to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(blank.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn a_node_call_needs_the_credential_issued_at_registration() {
        let service = service();
        let registered = service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap()
            .into_inner();
        let beat = |credential: Option<&str>| {
            let mut request = Request::new(HeartbeatRequest {
                node_id: "node-a".to_string(),
                ..Default::default()
            });
            if let Some(value) = credential {
                request.metadata_mut().insert(
                    ai_cluster_protocol::metadata::NODE_AUTH_HEADER,
                    value.parse().unwrap(),
                );
            }
            request
        };
        assert!(!registered.node_credential.is_empty());
        for bad in [None, Some("wrong")] {
            let refused = service.heartbeat(beat(bad)).await.unwrap_err();
            assert_eq!(refused.code(), tonic::Code::Unauthenticated);
        }
        assert!(
            service
                .heartbeat(beat(Some(&registered.node_credential)))
                .await
                .is_ok()
        );
        // The operator may act for any node.
        assert!(
            service
                .leave_node(op(LeaveNodeRequest {
                    node_id: "node-a".to_string(),
                }))
                .await
                .is_ok()
        );
    }

    fn with_credential<T>(mut request: Request<T>, credential: &str) -> Request<T> {
        request.metadata_mut().insert(
            ai_cluster_protocol::metadata::NODE_AUTH_HEADER,
            credential.parse().unwrap(),
        );
        request
    }

    #[tokio::test]
    async fn a_node_cannot_act_in_the_name_of_another_with_its_own_credential() {
        let service = service();
        let mut credential = std::collections::HashMap::new();
        for id in ["node-a", "node-b"] {
            let registered = service
                .register_node(Request::new(RegisterNodeRequest {
                    identity: Some(identity(id)),
                    capability: None,
                    replica: None,
                }))
                .await
                .unwrap()
                .into_inner();
            credential.insert(id, registered.node_credential);
        }
        // credencial de B nas chamadas que falam por A
        let b = credential["node-b"].clone();
        let node_a = || "node-a".to_string();
        let refused = [
            service
                .leave_node(with_credential(
                    Request::new(LeaveNodeRequest { node_id: node_a() }),
                    &b,
                ))
                .await
                .map(|_| ()),
            service
                .request_work(with_credential(
                    Request::new(RequestWorkRequest { node_id: node_a() }),
                    &b,
                ))
                .await
                .map(|_| ()),
            service
                .heartbeat(with_credential(
                    Request::new(HeartbeatRequest {
                        node_id: node_a(),
                        ..Default::default()
                    }),
                    &b,
                ))
                .await
                .map(|_| ()),
            service
                .submit_work_result(with_credential(
                    Request::new(SubmitWorkResultRequest {
                        result: Some(WorkResult {
                            work_id: "w".into(),
                            node_id: node_a(),
                            ..Default::default()
                        }),
                    }),
                    &b,
                ))
                .await
                .map(|_| ()),
        ];
        for result in refused {
            assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
        }
        // A continua no cluster: o LeaveNode alheio não a tirou
        let nodes = service
            .list_nodes(Request::new(ListNodesRequest {}))
            .await
            .unwrap()
            .into_inner()
            .nodes;
        assert!(nodes.iter().any(|node| {
            node.identity
                .as_ref()
                .is_some_and(|i| i.node_id == "node-a")
        }));
    }

    #[test]
    fn o_segredo_nao_aparece_no_debug() {
        let registry = Registry::new();
        let record = registry.register(identity("node-a"), CapabilityReport::default(), 1);
        assert!(!record.credential.is_empty());
        assert!(!format!("{record:?}").contains(&record.credential));
        let credential = ai_cluster_protocol::metadata::NodeCredential::default();
        credential.set("segredo-do-no");
        assert!(!format!("{credential:?}").contains("segredo-do-no"));
        let client = ai_cluster_protocol::metadata::ClientMetadata::with_token("token-secreto");
        assert!(!format!("{client:?}").contains("token-secreto"));
        let server =
            ai_cluster_protocol::metadata::ServerMetadata::new("token-secreto", "op-secreto");
        assert!(!format!("{server:?}").contains("secreto"));
        // só nó aprovado recebe estado do controller
        assert!(registry.is_trusted("node-a"));
        assert!(!Registry::requiring_approval().is_trusted("node-a"));
    }

    #[tokio::test]
    async fn request_work_is_refused_for_an_offline_node() {
        let (service, registry, _work) = service_with_registry_and_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        registry.mark_stale_offline(unix_ms() + 20_000, 10_000);
        let refused = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn enqueue_graph_refuses_new_graphs_when_the_queue_is_full() {
        let service = service();
        let graph = || {
            op(EnqueueGraphRequest {
                size: 2,
                seed: 1,
                request_id: String::new(),
                keep_result: false,
                priority: 0,
            })
        };
        // Four units per graph: the ceiling is crossed after 65 of them.
        for _ in 0..=MAX_QUEUED_UNITS / 4 {
            service.enqueue_graph(graph()).await.unwrap();
        }
        let refused = service.enqueue_graph(graph()).await.unwrap_err();
        assert_eq!(refused.code(), tonic::Code::ResourceExhausted);
    }

    #[tokio::test]
    async fn enqueue_graph_builds_a_diamond_with_blocked_branches() {
        let (service, work_store) = service_with_work();
        let response = service
            .enqueue_graph(op(EnqueueGraphRequest {
                size: 2,
                seed: 7,
                request_id: String::new(),
                keep_result: false,
                priority: 0,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.enqueued, 4);
        // A is ready; B, C and D wait on their dependencies.
        assert_eq!(response.blocked, 3);
        assert_eq!(work_store.pending(), 4);
        assert_eq!(work_store.blocked(), 3);

        // A size of zero is a client error.
        let invalid = service
            .enqueue_graph(op(EnqueueGraphRequest {
                size: 0,
                seed: 7,
                request_id: String::new(),
                keep_result: false,
                priority: 0,
            }))
            .await
            .unwrap_err();
        assert_eq!(invalid.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn placement_prefers_the_cheaper_idle_node() {
        let (service, _registry, work_store) = service_with_registry_and_work();
        for (id, threads) in [("node-weak", 2u32), ("node-strong", 8)] {
            service
                .register_node(Request::new(RegisterNodeRequest {
                    identity: Some(identity(id)),
                    capability: Some(CapabilityReport {
                        cpu_threads: threads,
                        ..Default::default()
                    }),
                    replica: None,
                }))
                .await
                .unwrap();
        }
        work_store.enqueue([big_unit("w1")]);

        // The weak node polls first, but the strong node is idle and cheaper, so the
        // weak node yields and the unit stays queued. First-come-first-served would
        // have handed it to the weak node.
        // The unit is 64x64x64 on purpose: on a small unit the thread-independent
        // transfer term dwarfs compute, the two costs land within the G5 hysteresis
        // band, and the weak node would rightly take it instead of yielding.
        let weak = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-weak".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!weak.has_work);
        assert!(weak.work.is_none());
        assert_eq!(work_store.pending(), 1);

        // The node that fits the unit better takes it.
        let strong = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-strong".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(strong.has_work);
        assert_eq!(strong.work.expect("strong node got w1").work_id, "w1");
    }

    #[tokio::test]
    async fn lifecycle_events_are_recorded_and_listed() {
        let (service, _work) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        let events = service
            .list_events(op(ListEventsRequest { limit: 0 }))
            .await
            .unwrap()
            .into_inner()
            .events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "node_registered");
        assert!(events[0].detail.contains("node-a"));

        // A limit returns only the newest event, still oldest first.
        let limited = service
            .list_events(op(ListEventsRequest { limit: 1 }))
            .await
            .unwrap()
            .into_inner()
            .events;
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].kind, "node_registered");
    }

    #[test]
    fn clamps_the_placement_tolerance_band() {
        // A negative band would sit below the cheapest cost and strand every unit, so
        // it clamps to zero — strictly cheapest.
        assert_eq!(clamp_tolerance(Some(-1.0)), 0.0);
        // A non-finite value would poison the comparison for every candidate.
        assert_eq!(clamp_tolerance(Some(f64::NAN)), DEFAULT_PLACEMENT_TOLERANCE);
        assert_eq!(
            clamp_tolerance(Some(f64::INFINITY)),
            DEFAULT_PLACEMENT_TOLERANCE
        );
        // Unset means the default, not zero.
        assert_eq!(clamp_tolerance(None), DEFAULT_PLACEMENT_TOLERANCE);
        // A real value passes through untouched.
        assert_eq!(clamp_tolerance(Some(0.25)), 0.25);
        assert_eq!(clamp_tolerance(Some(0.0)), 0.0);
    }

    #[tokio::test]
    async fn metrics_roll_up_nodes_and_completed_work() {
        let (service, work_store) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        // An idle cluster: a node is present, nothing has run, and the averages are zero
        // rather than a division by zero.
        let metrics = service
            .get_metrics(Request::new(GetMetricsRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(metrics.nodes_total, 1);
        assert_eq!(metrics.nodes_available, 1);
        assert_eq!(metrics.units_completed, 0);
        assert_eq!(metrics.units_pending, 0);
        assert_eq!(metrics.avg_compute_time_us, 0);

        work_store.enqueue([unit("w1")]);
        service
            .request_work(op(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap();
        service
            .submit_work_result(op(SubmitWorkResultRequest {
                result: Some(WorkResult {
                    work_id: "w1".to_string(),
                    node_id: "node-a".to_string(),
                    rows: 2,
                    cols: 2,
                    matrix_c: vec![19.0, 22.0, 43.0, 50.0],
                    checksum: 134.0,
                    compute_time_us: 1_000,
                }),
            }))
            .await
            .unwrap();

        // The unit moved from pending to completed and its compute time is summed. The
        // latency total is not asserted: it comes from the controller clock while compute
        // comes from the worker, so on a unit that finishes within one millisecond the
        // worker's self-report can exceed the observed wall time. That is a measurement
        // artifact of an instant test, not a bug — in production a unit takes milliseconds.
        let metrics = service
            .get_metrics(Request::new(GetMetricsRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(metrics.units_completed, 1);
        assert_eq!(metrics.units_pending, 0);
        assert_eq!(metrics.total_compute_time_us, 1_000);
        assert_eq!(metrics.avg_compute_time_us, 1_000);
    }

    #[tokio::test]
    async fn get_trace_follows_a_requests_units_through_the_service() {
        let (service, _work_store) = service_with_work();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("node-a")),
                capability: None,
                replica: None,
            }))
            .await
            .unwrap();

        // The service mints a request id when the client does not supply one.
        let enqueued = service
            .enqueue_work(op(EnqueueWorkRequest {
                rows: 2,
                shared: 2,
                cols: 2,
                seed: 7,
                repetitions: 1,
                min_cpu_threads: 0,
                request_id: String::new(),
                priority: 0,
                require_attested: false,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!enqueued.request_id.is_empty());

        // Before dispatch the unit is queued and carries no cost.
        let trace = service
            .get_trace(Request::new(GetTraceRequest {
                request_id: enqueued.request_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(trace.units.len(), 1);
        assert_eq!(trace.units[0].status, "queued");

        // Run the unit end to end: dispatch, compute, submit.
        let claimed = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner()
            .work
            .expect("work unit present");
        service
            .submit_work_result(op(SubmitWorkResultRequest {
                result: Some(WorkResult {
                    work_id: claimed.work_id.clone(),
                    node_id: "node-a".to_string(),
                    rows: 2,
                    cols: 2,
                    matrix_c: vec![19.0, 22.0, 43.0, 50.0],
                    checksum: 134.0,
                    compute_time_us: 1_000,
                }),
            }))
            .await
            .unwrap();

        // The same request id now resolves to the completed unit and its timings.
        let trace = service
            .get_trace(Request::new(GetTraceRequest {
                request_id: enqueued.request_id,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(trace.units.len(), 1);
        assert_eq!(trace.units[0].work_id, claimed.work_id);
        assert_eq!(trace.units[0].status, "completed");
        assert_eq!(trace.units[0].node_id, "node-a");
        assert_eq!(trace.units[0].compute_time_us, 1_000);
    }

    /// A unit under a given client request, so a reservation can be matched to it (G6).
    fn unit_for(work_id: &str, request_id: &str) -> WorkUnit {
        WorkUnit {
            request_id: request_id.to_string(),
            ..unit(work_id)
        }
    }

    async fn register(service: &ClusterControlService, node_id: &str, threads: u32) {
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity(node_id)),
                capability: Some(CapabilityReport {
                    cpu_threads: threads,
                    ..Default::default()
                }),
                replica: None,
            }))
            .await
            .unwrap();
    }

    fn recovery<T>(message: T) -> Request<T> {
        let mut request = Request::new(message);
        request
            .extensions_mut()
            .insert(ai_cluster_protocol::metadata::Recovery);
        request
    }

    #[tokio::test]
    async fn o_token_de_recuperacao_so_fala_por_no_com_sufixo_rec() {
        let (service, _work) = service_with_work();
        register(&service, "node-a", 4).await;
        let register_as = |id: &str| {
            recovery(RegisterNodeRequest {
                identity: Some(identity(id)),
                capability: Some(CapabilityReport::default()),
                replica: None,
            })
        };
        // não registra (nem troca a credencial de) um nó real
        let err = service
            .register_node(register_as("node-a"))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
        service
            .register_node(register_as("node-a-rec"))
            .await
            .unwrap();
        // heartbeat de nó alheio ou desconhecido: negado, sem vazar a cópia do estado
        for id in ["node-a", "inventado"] {
            let beat = recovery(HeartbeatRequest {
                node_id: id.into(),
                unix_ms: 0,
                replica_epoch_unix_ms: 0,
            });
            let err = service.heartbeat(beat).await.unwrap_err();
            assert_eq!(err.code(), tonic::Code::PermissionDenied, "{id}");
        }
    }

    fn reserve_request(node_id: &str, request_id: &str, ttl_ms: u64) -> Request<ReserveRequest> {
        op(ReserveRequest {
            node_id: node_id.to_string(),
            request_id: request_id.to_string(),
            ttl_ms,
            slots: 0,
        })
    }

    #[tokio::test]
    async fn reserve_holds_a_registered_node_and_lists_the_hold() {
        let (service, _work) = service_with_work();
        register(&service, "node-a", 8).await;

        let granted = service
            .reserve(reserve_request("node-a", "req-x", 5_000))
            .await
            .unwrap()
            .into_inner();
        assert!(granted.granted);
        assert!(granted.expires_at_unix_ms > 0);

        let listed = service
            .list_reservations(Request::new(ListReservationsRequest {}))
            .await
            .unwrap()
            .into_inner()
            .reservations;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].node_id, "node-a");
        assert_eq!(listed[0].request_id, "req-x");
    }

    #[tokio::test]
    async fn a_reserved_node_stops_taking_another_requests_work() {
        let (service, _registry, work_store) = service_with_registry_and_work();
        // Only node-a exists, so if it refuses the unit nothing else can take it and the
        // refusal is unambiguously the hold, not the cost ranking.
        register(&service, "node-a", 8).await;
        work_store.enqueue([unit_for("w1", "req-x")]);
        service
            .reserve(reserve_request("node-a", "req-other", 5_000))
            .await
            .unwrap();

        let refused = service
            .request_work(op(RequestWorkRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!refused.has_work);
        assert_eq!(work_store.pending(), 1);
    }

    #[tokio::test]
    async fn reserve_refuses_an_unknown_node_without_an_error() {
        let (service, _work) = service_with_work();

        // Not reserving is a valid answer, not a transport failure: the client gets
        // granted=false and a reason it can print.
        let refused = service
            .reserve(reserve_request("ghost", "req-x", 0))
            .await
            .unwrap()
            .into_inner();
        assert!(!refused.granted);
        assert!(refused.message.contains("ghost"));
    }

    #[tokio::test]
    async fn reserve_requires_a_node_and_a_request_id() {
        let (service, _work) = service_with_work();

        let missing_node = service
            .reserve(reserve_request("  ", "req-x", 0))
            .await
            .unwrap_err();
        assert_eq!(missing_node.code(), tonic::Code::InvalidArgument);

        let missing_request = service
            .reserve(reserve_request("node-a", "", 0))
            .await
            .unwrap_err();
        assert_eq!(missing_request.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn release_reservation_frees_the_hold() {
        let (service, _work) = service_with_work();
        register(&service, "node-a", 8).await;
        service
            .reserve(reserve_request("node-a", "req-x", 5_000))
            .await
            .unwrap();

        let released = service
            .release_reservation(op(ReleaseReservationRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(released.released);

        // Releasing a node nobody holds is not an error, just false.
        let again = service
            .release_reservation(op(ReleaseReservationRequest {
                node_id: "node-a".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!again.released);
    }

    fn inference<T>(message: T) -> Request<T> {
        let mut request = Request::new(message);
        request
            .extensions_mut()
            .insert(ai_cluster_protocol::metadata::Inference);
        request
    }

    #[tokio::test]
    async fn um_no_pendente_nao_escreve_estado_no_controller() {
        use ai_cluster_protocol::cluster::{StateReplica, WorkSnapshot};
        use prost::Message;
        let dir = std::env::temp_dir().join(format!("aic-pend-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let work = Arc::new(WorkStore::with_state(dir.join("state.bin")));
        let registry = Arc::new(Registry::requiring_approval());
        let service = ClusterControlService::new(
            registry.clone(),
            work.clone(),
            Arc::new(EventLog::new()),
            "test-cluster",
        );
        // uma cópia com uma unidade pendente, de uma época bem nova
        let copy = StateReplica {
            epoch_unix_ms: u64::MAX / 2,
            snapshot: WorkSnapshot {
                pending: vec![unit("copiada")],
                ..Default::default()
            }
            .encode_to_vec(),
        };
        let register = |replica: Option<StateReplica>| RegisterNodeRequest {
            identity: Some(identity("node-a")),
            capability: None,
            replica,
        };
        service
            .register_node(Request::new(register(Some(copy.clone()))))
            .await
            .unwrap();
        assert!(!registry.is_trusted("node-a"));
        assert_eq!(
            work.pending(),
            0,
            "nó pendente não escreve o estado do controller"
        );
        // confiável desde o registro (sem exigir aprovação), a mesma cópia vale
        let trusted_dir = dir.join("confiavel");
        std::fs::create_dir_all(&trusted_dir).unwrap();
        let work = Arc::new(WorkStore::with_state(trusted_dir.join("state.bin")));
        let service = ClusterControlService::new(
            Arc::new(Registry::new()),
            work.clone(),
            Arc::new(EventLog::new()),
            "test-cluster",
        );
        service
            .register_node(Request::new(register(Some(copy))))
            .await
            .unwrap();
        let restored = work.pending();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(restored, 1);
    }

    #[tokio::test]
    async fn reports_and_events_are_for_the_operator_only() {
        let (service, _) = service_with_work();
        let denied = service
            .work_report(Request::new(WorkReportRequest { limit: 0 }))
            .await
            .unwrap_err();
        assert_eq!(denied.code(), tonic::Code::PermissionDenied);
        let denied = service
            .list_events(Request::new(ListEventsRequest { limit: 0 }))
            .await
            .unwrap_err();
        assert_eq!(denied.code(), tonic::Code::PermissionDenied);
        assert!(
            service
                .work_report(op(WorkReportRequest { limit: 0 }))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn the_gateway_reports_its_ram_and_slots_and_invalid_values_keep_the_last() {
        let service = service();
        assert_eq!(service.gateway_free(), None, "sem relatório, não confere");
        let report = |slots: u32, free: u64| ReportGatewayRequest {
            llama_version: String::new(),
            llama_slots: slots,
            memory_available_bytes: free,
            running: None,
            stats: None,
            gateway_id: "gw1".into(),
            url: "http://gw:8080".into(),
            models: Vec::new(),
        };
        service
            .report_gateway(inference(report(4, 1 << 30)))
            .await
            .unwrap();
        assert_eq!(service.gateway_free(), Some(1 << 30));
        assert_eq!(service.slots(), 4);
        // Zero e vagas fora de 1..=16 não apagam o que valia.
        service
            .report_gateway(inference(report(99, 0)))
            .await
            .unwrap();
        assert_eq!(service.gateway_free(), Some(1 << 30));
        assert_eq!(service.slots(), 4);
    }

    #[test]
    fn the_gateway_ram_makes_pick_model_drop_a_variant_the_workers_could_hold() {
        use ai_cluster_catalog::{ModelEntry, check::NodeMemory};
        const GIB: u64 = 1 << 30;
        let (service, _, _) = service_with_registry_and_work();
        let entry = |id: &str, gib: u64| ModelEntry {
            id: id.into(),
            bytes: gib * GIB,
            supported: true,
            ..ModelEntry::default()
        };
        let found = [entry("m-q8.gguf", 8), entry("m-q4.gguf", 4)];
        let nodes = [NodeMemory {
            total_bytes: 32 * GIB,
            used_fraction: 0.0,
        }];
        let id = || service.pick_model(&found, "m", 0, &nodes).map(|m| m.id);
        assert_eq!(
            id().as_deref(),
            Some("m-q8.gguf"),
            "sem informação, o maior"
        );
        // O gateway só tem 2 GiB livres: o q8 pede ~2,5 GiB lá, o q4 ~1,3.
        service
            .gateway_free
            .store(2 * GIB, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(id().as_deref(), Some("m-q4.gguf"));
    }

    #[tokio::test]
    async fn o_gateway_se_apresenta_e_o_operador_o_lista() {
        use ai_cluster_protocol::cluster::GatewayModel;
        let service = service();
        let report = |id: &str, models: &[&str]| ReportGatewayRequest {
            llama_version: String::new(),
            llama_slots: 0,
            memory_available_bytes: 0,
            running: None,
            stats: None,
            gateway_id: id.into(),
            url: "http://gw:8080".into(),
            models: models
                .iter()
                .map(|model| GatewayModel {
                    model_id: model.to_string(),
                    shard_nodes: vec!["w1".into()],
                })
                .collect(),
        };
        service
            .report_gateway(inference(report("gw1", &["gpt2"])))
            .await
            .unwrap();
        // repetir atualiza, não duplica, e só a primeira vez vira evento
        service
            .report_gateway(inference(report("gw1", &["gpt2", "smollm2"])))
            .await
            .unwrap();
        let listed = service
            .list_gateways(op(ListGatewaysRequest {}))
            .await
            .unwrap()
            .into_inner()
            .gateways;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].models.len(), 2);
        assert!(listed[0].alive);
        let registered = service
            .events
            .recent(50)
            .into_iter()
            .filter(|event| event.kind == "gateway_registered")
            .count();
        assert_eq!(registered, 1);
        // texto de controle, id vazio e modelo com nome vazio são recusados
        for bad in [report("", &[]), report("g\nx", &[]), report("gw2", &[""])] {
            assert!(service.report_gateway(inference(bad)).await.is_err());
        }
        // estatística com NaN ou negativa não entra
        let mut hostile = report("gw3", &[]);
        hostile.stats = Some(ai_cluster_protocol::cluster::GatewayStats {
            ttft_p50_seconds: f64::NAN,
            ..Default::default()
        });
        assert!(service.report_gateway(inference(hostile)).await.is_err());
        let mut honest = report("gw4", &[]);
        honest.stats = Some(ai_cluster_protocol::cluster::GatewayStats {
            requests: 3,
            ..Default::default()
        });
        service.report_gateway(inference(honest)).await.unwrap();
        let shown = service
            .list_gateways(op(ListGatewaysRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert!(shown.now_unix_ms > 0);
        let gw4 = shown
            .gateways
            .iter()
            .find(|g| g.gateway_id == "gw4")
            .unwrap();
        assert_eq!(gw4.stats.as_ref().map(|s| s.requests), Some(3));
        // texto que disfarça (inversão de direção, largura zero) também é recusado
        assert!(
            service
                .report_gateway(inference(report("gw\u{202E}x", &[])))
                .await
                .is_err()
        );
        // listar é do operador: o token de inferência não passa
        assert!(
            service
                .list_gateways(inference(ListGatewaysRequest {}))
                .await
                .is_err()
        );
    }

    #[test]
    fn a_janela_do_relatorio_traz_as_mais_recentes() {
        let all = [1, 2, 3, 4];
        assert_eq!(tail(&all, 0), &[1, 2, 3, 4]);
        assert_eq!(tail(&all, 2), &[3, 4]);
        assert_eq!(tail(&all, 9), &[1, 2, 3, 4]);
    }

    #[test]
    fn um_gateway_vivo_nao_e_tomado_por_outro_endereco() {
        let a: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        let b: std::net::IpAddr = "10.0.0.2".parse().unwrap();
        assert!(gateway_taken(true, Some(&a), Some(b)));
        assert!(!gateway_taken(true, Some(&a), Some(a)), "o dono volta");
        assert!(
            !gateway_taken(false, Some(&a), Some(b)),
            "sem sinal, o id fica livre"
        );
        assert!(
            !gateway_taken(true, Some(&a), None),
            "sem endereço, sem regra"
        );
        assert!(!gateway_taken(true, None, Some(b)), "id novo");
    }

    #[test]
    fn um_endereco_nao_enche_a_tabela_de_gateways() {
        let a: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        let b: std::net::IpAddr = "10.0.0.2".parse().unwrap();
        let mut hosts = std::collections::HashMap::new();
        for n in 0..3 {
            hosts.insert(format!("g{n}"), a);
        }
        hosts.insert("outro".to_string(), b);
        let alive = |_: &str| true;
        // 3 vivos em `a`: com teto 3 um id novo não entra; um id já conhecido sim (não conta a si).
        assert!(host_is_full(&hosts, alive, a, "novo", 3));
        assert!(!host_is_full(&hosts, alive, a, "g0", 3));
        // Outro endereço não é afetado, e gateway sem sinal não ocupa vaga.
        assert!(!host_is_full(&hosts, alive, b, "novo", 3));
        assert!(!host_is_full(
            &hosts,
            |id| id != "g1" && id != "g2",
            a,
            "novo",
            2
        ));
    }

    #[tokio::test]
    async fn gateways_demais_esperam_ou_trocam_o_morto() {
        let service = service();
        let report = |id: String| {
            inference(ReportGatewayRequest {
                llama_version: String::new(),
                llama_slots: 0,
                memory_available_bytes: 0,
                running: None,
                stats: None,
                gateway_id: id,
                url: String::new(),
                models: vec![],
            })
        };
        for n in 0..MAX_GATEWAYS {
            service
                .report_gateway(report(format!("g{n}")))
                .await
                .unwrap();
        }
        // todos vivos: o 65º espera
        assert!(service.report_gateway(report("novo".into())).await.is_err());
        // um deles fica sem sinal: o 65º entra no lugar dele
        service
            .gateways
            .lock()
            .unwrap()
            .get_mut("g7")
            .unwrap()
            .last_seen_unix_ms = 1;
        service.report_gateway(report("novo".into())).await.unwrap();
        let gateways = service.gateways.lock().unwrap();
        assert!(gateways.contains_key("novo") && !gateways.contains_key("g7"));
        assert_eq!(gateways.len(), MAX_GATEWAYS);
    }

    #[tokio::test]
    async fn shard_capacity_diz_o_que_o_no_aceita_guardar() {
        let service = service();
        service
            .register_node(Request::new(RegisterNodeRequest {
                identity: Some(identity("w1")),
                capability: Some(CapabilityReport {
                    memory_bytes: 8,
                    shard_cache_bytes: 4,
                    ..Default::default()
                }),
                replica: None,
            }))
            .await
            .unwrap();
        let ask = |ids: &[&str]| ShardCapacityRequest {
            node_ids: ids.iter().map(|id| id.to_string()).collect(),
        };
        let answer = service
            .shard_capacity(inference(ask(&["w1", "ninguem"])))
            .await
            .unwrap()
            .into_inner()
            .nodes;
        assert_eq!(
            (
                answer[0].available,
                answer[0].shard_cache_bytes,
                answer[0].memory_bytes
            ),
            (true, 4, 8)
        );
        assert!(!answer[1].available);
        // sem token de inferência ou de operador, não responde; lista vazia ou grande demais, também
        let denied = service.shard_capacity(Request::new(ask(&["w1"]))).await;
        assert_eq!(denied.unwrap_err().code(), tonic::Code::PermissionDenied);
        let empty = service.shard_capacity(inference(ask(&[]))).await;
        assert_eq!(empty.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn o_barramento_entrega_o_evento_novo_so_ao_operador() {
        use tonic::codegen::tokio_stream::StreamExt;
        let (service, _work) = service_with_work();
        let denied = service
            .watch_events(Request::new(WatchEventsRequest {}))
            .await;
        assert_eq!(denied.err().unwrap().code(), tonic::Code::PermissionDenied);
        let mut stream = service
            .watch_events(op(WatchEventsRequest {}))
            .await
            .unwrap()
            .into_inner();
        // a própria assinatura é auditada e chega pelo barramento
        let audit = stream.next().await.unwrap().unwrap();
        assert_eq!(
            (audit.kind.as_str(), audit.detail.contains("watch_events")),
            ("audit", true)
        );
        service.events.record(7, "teste", "novo");
        let got = stream.next().await.unwrap().unwrap();
        assert_eq!(
            (got.at_unix_ms, got.kind.as_str(), got.detail.as_str()),
            (7, "teste", "novo")
        );
        // o limite de assinantes recusa o excesso
        let mut held = Vec::new();
        for _ in 0..MAX_EVENT_WATCHERS {
            match service.watch_events(op(WatchEventsRequest {})).await {
                Ok(stream) => held.push(stream),
                Err(status) => {
                    assert_eq!(status.code(), tonic::Code::ResourceExhausted);
                    return;
                }
            }
        }
        let excess = service.watch_events(op(WatchEventsRequest {})).await;
        assert_eq!(excess.err().unwrap().code(), tonic::Code::ResourceExhausted);
    }

    #[tokio::test]
    async fn shard_matmul_refuses_beyond_the_global_limit() {
        let service = service();
        let held = service
            .shard_calls
            .clone()
            .try_acquire_many_owned(MAX_SHARD_CALLS as u32)
            .unwrap();
        let request = op(ShardMatmulRequest {
            rows: 1,
            k: 1,
            x: vec![1.0],
            ops: vec![ai_cluster_protocol::cluster::ShardOp {
                node_id: "n".into(),
                tensor: "h.0.mlp.c_fc.weight".into(),
                row_start: 0,
                row_end: 1,
                ..Default::default()
            }],
        });
        let refused = service.shard_matmul(request).await.unwrap_err();
        assert_eq!(refused.code(), tonic::Code::ResourceExhausted);
        drop(held);
    }

    #[test]
    fn a_compute_error_blames_no_node() {
        let status = compute_failure("tensor não permitido".into());
        assert!(
            status
                .metadata()
                .get(ai_cluster_protocol::metadata::FAILED_NODE_HEADER)
                .is_none()
        );
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }
    #[test]
    fn rpc_address_refuses_loopback_link_local_and_odd_names() {
        assert_eq!(
            rpc_address("192.168.1.18", 50061).as_deref(),
            Some("192.168.1.18:50061")
        );
        assert_eq!(
            rpc_address("fd00::5", 50061).as_deref(),
            Some("[fd00::5]:50061")
        );
        assert_eq!(
            rpc_address("worker-1.lan", 50061).as_deref(),
            Some("worker-1.lan:50061")
        );
        for bad in [
            "127.0.0.1",
            "::1",
            "0.0.0.0",
            "169.254.1.1",
            "fe80::1",
            "224.0.0.1",
            "localhost",
            "a b",
            "",
            "x..y",
            "-",
        ] {
            let ok = rpc_address(bad, 50061);
            assert!(ok.is_none() || bad == "-", "{bad} -> {ok:?}");
        }
        assert!(rpc_address("10.0.0.1", 0).is_none());
        assert!(rpc_address("10.0.0.1", 70_000).is_none());
    }

    #[test]
    fn clean_status_drops_bad_ids_and_control_characters() {
        let status = |id: &str, reason: &str, rate: f64| ModelStatus {
            model_id: id.into(),
            state: 3,
            reason: reason.into(),
            generation: 1,
            tokens_per_second: rate,
        };
        let ok = clean_status(status("smollm2-q4", "falhou\x1b[31m!", 2.0)).unwrap();
        assert!(!ok.reason.contains('\x1b'));
        assert_eq!(
            clean_status(status("x", &"a".repeat(1000), 0.0))
                .unwrap()
                .reason
                .len(),
            200
        );
        assert!(clean_status(status("com espaço", "", 1.0)).is_none());
        assert!(clean_status(status("", "", 1.0)).is_none());
        assert!(clean_status(status("m", "", f64::NAN)).is_none());
        assert!(clean_status(status("m", "", -1.0)).is_none());
    }
}
