use std::sync::Arc;
use std::time::Duration;

use ai_cluster_controller::enrollment::EnrollmentService;
use ai_cluster_controller::events::EventLog;
use ai_cluster_controller::registry::{Registry, unix_ms};
use ai_cluster_controller::service::{ClusterControlService, LinkProbeService, MAX_PROBE_BYTES};
use ai_cluster_controller::work::WorkStore;
use ai_cluster_protocol::cluster::cluster_control_server::ClusterControlServer;
use ai_cluster_protocol::cluster::enrollment_server::EnrollmentServer;
use ai_cluster_protocol::cluster::link_probe_server::LinkProbeServer;
use ai_cluster_protocol::metadata::{
    INFERENCE_TOKEN_ENV, INSECURE_ENV, JOIN_TOKEN_ENV, MAX_MESSAGE_BYTES, OPERATOR_TOKEN_ENV,
    RECOVERY_TOKEN_ENV, ServerMetadata, TLS_CLIENT_CA_ENV, inference_gate, inference_token,
    insecure_allowed, join_token, operator_token, recovery_gate, recovery_token,
};
use tokio::time;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};

/// Where a piece of controller state lives: the variable that names its file, or `file` inside
/// `AI_CLUSTER_DATA_DIR` (one variable that makes the registry, the queue and the events survive a
/// restart). `None` is memory only. A file named by its own variable wins over the directory.
fn state_path(var: &str, file: &str) -> Option<std::path::PathBuf> {
    resolve_state_path(
        std::env::var(var).ok().as_deref(),
        std::env::var("AI_CLUSTER_DATA_DIR").ok().as_deref(),
        file,
    )
}

fn resolve_state_path(
    explicit: Option<&str>,
    dir: Option<&str>,
    file: &str,
) -> Option<std::path::PathBuf> {
    fn set(value: Option<&str>) -> Option<&str> {
        value.map(str::trim).filter(|value| !value.is_empty())
    }
    set(explicit)
        .map(std::path::PathBuf::from)
        .or_else(|| set(dir).map(|dir| std::path::Path::new(dir).join(file)))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ai_cluster_protocol::metadata::check_secret_files()?;
    let addr = std::env::var("AI_CLUSTER_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50051".to_string())
        .parse::<std::net::SocketAddr>()?;
    let grace_ms = std::env::var("AI_CLUSTER_HEARTBEAT_GRACE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(15_000);

    // Fail closed: a controller with no token would accept any client, including one that
    // injects state through RegisterNode, so it does not start.
    let Some(token) = join_token() else {
        return Err(
            format!("{JOIN_TOKEN_ENV} é obrigatório; o controller não sobe sem token").into(),
        );
    };
    let Some(operator) = operator_token() else {
        return Err(format!(
            "{OPERATOR_TOKEN_ENV} é obrigatório; sem ele ninguém aprova nós nem enfileira trabalho"
        )
        .into());
    };
    if operator == token {
        return Err(format!("{OPERATOR_TOKEN_ENV} deve ser diferente de {JOIN_TOKEN_ENV}").into());
    }
    // Optional third secret (J5): lets a gateway run models on the workers without the operator
    // token. It must differ from the other two, or it would stand in for them.
    let inference = inference_token();
    if inference
        .as_ref()
        .is_some_and(|t| *t == token || *t == operator)
    {
        return Err(format!(
            "{INFERENCE_TOKEN_ENV} deve ser diferente de {JOIN_TOKEN_ENV} e de {OPERATOR_TOKEN_ENV}"
        )
        .into());
    }
    // Optional fourth secret (F19, ADR-017): the token a node in network recovery carries in clear
    // on its card. It reaches only the calls that show a node up and must differ from the others.
    let recovery = recovery_token();
    if recovery
        .as_ref()
        .is_some_and(|t| *t == token || *t == operator || inference.as_ref() == Some(t))
    {
        return Err(format!(
            "{RECOVERY_TOKEN_ENV} deve ser diferente de {JOIN_TOKEN_ENV}, {OPERATOR_TOKEN_ENV} e {INFERENCE_TOKEN_ENV}"
        )
        .into());
    }
    let auth = ServerMetadata::new(&token, &operator)
        .with_inference(inference.as_deref().unwrap_or(""))
        .with_recovery(recovery.as_deref().unwrap_or(""));
    let gate = inference.clone().unwrap_or_default();
    let recovery_scope = recovery.clone().unwrap_or_default();

    // S2: a new node waits for `aic cluster node approve` unless the operator opts out, which is
    // what a closed lab network or the E2E scripts do.
    let auto_approve = std::env::var("AI_CLUSTER_AUTO_APPROVE")
        .is_ok_and(|value| matches!(value.trim(), "1" | "true"));
    let registry = if auto_approve {
        Registry::new()
    } else {
        Registry::requiring_approval()
    };
    if let Ok(dir) = std::env::var("AI_CLUSTER_DATA_DIR")
        && !dir.trim().is_empty()
    {
        std::fs::create_dir_all(dir.trim())
            .map_err(|err| format!("AI_CLUSTER_DATA_DIR {dir}: {err}"))?;
    }
    // AI_CLUSTER_NODES names a file where approvals and credentials survive a restart (S5).
    let registry = Arc::new(match state_path("AI_CLUSTER_NODES", "nodes") {
        Some(path) => registry.persisted(path),
        None => registry,
    });
    // AI_CLUSTER_STATE names a snapshot file. Unset means memory only, which is what
    // the benchmark runs use; point it at a mounted volume so a restart keeps the
    // queue instead of losing it with the container.
    let work = match state_path("AI_CLUSTER_STATE", "state.bin") {
        Some(path) => WorkStore::with_state(path),
        None => WorkStore::new(),
    };
    // O histórico de concluídas (traces e roll-up) também sobrevive a um reinício: AI_CLUSTER_TRACES
    // ou, com AI_CLUSTER_DATA_DIR, `traces.log`.
    let work = Arc::new(match state_path("AI_CLUSTER_TRACES", "traces.log") {
        Some(path) => work.with_traces(path),
        None => work,
    });
    // Rajadas de mutações gravam o snapshot no máximo a cada AI_CLUSTER_STATE_WRITE_MS (padrão 250;
    // 0 grava a cada mutação), poupando SD e disco fraco; um flush por segundo grava o resto.
    work.set_write_gap_ms(
        std::env::var("AI_CLUSTER_STATE_WRITE_MS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(250),
    );
    let flusher = work.clone();
    tokio::spawn(async move {
        let mut ticker = time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            flusher.flush();
        }
    });
    // As taxas por segundo só valem depois de o controller observar por uma janela inteira.
    ai_cluster_controller::metrics::mark_start();
    // AI_CLUSTER_EVENTS names a file where the event timeline survives a restart (F06).
    let events = Arc::new(match state_path("AI_CLUSTER_EVENTS", "events.log") {
        Some(path) => EventLog::new().persisted(path),
        None => EventLog::new(),
    });
    // AI_CLUSTER_EVENTS_SYSLOG=host:porta envia cada evento novo a um coletor por syslog UDP (F05).
    if let Some(target) = std::env::var("AI_CLUSTER_EVENTS_SYSLOG")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        ai_cluster_controller::syslog::spawn(events.clone(), target);
    }
    let service =
        ClusterControlService::new(registry.clone(), work.clone(), events.clone(), "local");
    let enrollments = service.enrollments().clone();

    // ponytail: single sweep interval for all nodes; per-node timers only if grace must vary.
    let sweep = registry.clone();
    let sweep_work = work.clone();
    let sweep_events = events.clone();
    // Quando o controller subiu. A posse restaurada do disco é medida contra isto, não
    // contra o horário do despacho original: o que importa é se o dono voltou depois do
    // restart.
    let started_unix_ms = unix_ms();
    tokio::spawn(async move {
        let mut ticker = time::interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            for node_id in sweep.mark_stale_offline(unix_ms(), grace_ms) {
                // A node that went silent still holds its in-flight units; hand
                // them back so the surviving nodes can finish the work.
                let outcome = sweep_work.requeue_for_node(&node_id);
                println!(
                    "nó {node_id} offline após {grace_ms}ms sem heartbeat; {} unidades em voo reenfileiradas, {} falharam",
                    outcome.requeued, outcome.failed
                );
                sweep_events.record(
                    unix_ms(),
                    "node_offline",
                    format!(
                        "{node_id} após {grace_ms}ms reenfileiradas={} falharam={}",
                        outcome.requeued, outcome.failed
                    ),
                );
            }
            // Unidades em voo cujo dono nunca reapareceu depois de um restart: a posse veio
            // do snapshot, mas o nó não voltou. Antes do grace elas esperam — o dono pode
            // estar voltando, e reoferecer agora só faria a unidade rodar duas vezes. Passado
            // o grace valem as mesmas regras de um nó perdido: volta para a fila, ou falha se
            // o orçamento de tentativas acabou.
            if unix_ms().saturating_sub(started_unix_ms) >= grace_ms {
                let known: Vec<String> = sweep
                    .list()
                    .iter()
                    .filter_map(|node| node.identity.as_ref().map(|id| id.node_id.clone()))
                    .collect();
                for node_id in sweep_work.unknown_owners(&known) {
                    let outcome = sweep_work.requeue_for_node(&node_id);
                    println!(
                        "nó {node_id} não voltou após o restart; {} unidades em voo reenfileiradas, {} falharam",
                        outcome.requeued, outcome.failed
                    );
                    sweep_events.record(
                        unix_ms(),
                        "node_offline",
                        format!(
                            "{node_id} ausente após restart reenfileiradas={} falharam={}",
                            outcome.requeued, outcome.failed
                        ),
                    );
                }
            }
        }
    });

    // Prometheus exporter (H1): the same roll-up pushed to whoever scrapes, so watching the
    // cluster does not require a client that speaks gRPC. A failure here is reported and
    // never fatal — losing the exporter must not take the control plane down with it.
    let exporter_registry = registry.clone();
    let exporter_work = work.clone();
    let exporter_token: Arc<str> = operator.as_str().into();
    tokio::spawn(async move {
        let metrics_addr = std::env::var("AI_CLUSTER_METRICS_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:9101".to_string());
        println!("exportador de métricas em http://{metrics_addr}/metrics");
        if let Err(err) = ai_cluster_controller::exporter::serve(
            metrics_addr,
            exporter_registry,
            exporter_work,
            exporter_token,
        )
        .await
        {
            println!("exportador de métricas parou: {err}");
        }
    });

    // The generated `with_interceptor` builds its own server without the size limits, so
    // the limits are set on the server first and the interceptor is attached afterwards.
    let server = ClusterControlServer::new(service)
        .max_decoding_message_size(MAX_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES);
    // Messages a little over the probe payload (protobuf framing), still far under the data limit.
    let probe =
        LinkProbeServer::new(LinkProbeService).max_decoding_message_size(MAX_PROBE_BYTES + 1024);
    // S4: certificate and key come as a pair. A certificate wins over the insecure opt-out, so a
    // forgotten flag cannot downgrade a controller that was given one.
    let identity = match (
        std::env::var("AI_CLUSTER_TLS_CERT"),
        std::env::var("AI_CLUSTER_TLS_KEY"),
    ) {
        (Ok(cert), Ok(key)) => Some(Identity::from_pem(
            std::fs::read(cert)?,
            std::fs::read(key)?,
        )),
        (Err(_), Err(_)) => None,
        _ => {
            return Err("AI_CLUSTER_TLS_CERT e AI_CLUSTER_TLS_KEY vêm juntos".into());
        }
    };
    // ADR-023: a única porta sem token, só para a etapa de espera de instalação. Desligada por
    // padrão; sem client CA de propósito, porque o aparelho ainda não tem certificado nenhum.
    if let Some(enroll_addr) = std::env::var("AI_CLUSTER_ENROLL_ADDR")
        .ok()
        .filter(|addr| !addr.trim().is_empty())
    {
        let enroll_addr: std::net::SocketAddr = enroll_addr.parse()?;
        let listener = tokio::net::TcpListener::bind(enroll_addr).await?;
        let mut enroll_server = Server::builder()
            .timeout(Duration::from_secs(10))
            .concurrency_limit_per_connection(4)
            .http2_keepalive_interval(Some(Duration::from_secs(10)))
            .http2_keepalive_timeout(Some(Duration::from_secs(10)));
        if let Some(identity) = identity.clone() {
            enroll_server = enroll_server.tls_config(ServerTlsConfig::new().identity(identity))?;
        }
        let service = EnrollmentServer::new(EnrollmentService::new(enrollments))
            .max_decoding_message_size(16 * 1024);
        println!("etapa de espera de instalação em {enroll_addr} (sem token)");
        tokio::spawn(async move {
            let incoming = ai_cluster_controller::enrollment::limited_incoming(listener);
            if let Err(err) = enroll_server
                .add_service(service)
                .serve_with_incoming(incoming)
                .await
            {
                println!("servidor de instalação parou: {err}");
            }
        });
    }
    // Detects a worker that vanished without closing its resident stream.
    let mut builder = Server::builder()
        .http2_keepalive_interval(Some(std::time::Duration::from_secs(10)))
        .http2_keepalive_timeout(Some(std::time::Duration::from_secs(10)));
    match identity {
        Some(identity) => {
            let mut tls = ServerTlsConfig::new().identity(identity);
            // mTLS (F19): with a client CA, a peer without a certificate it signed never gets
            // past the handshake, before any token is looked at.
            if let Ok(ca) = std::env::var(TLS_CLIENT_CA_ENV) {
                tls = tls.client_ca_root(Certificate::from_pem(std::fs::read(ca)?));
                println!("mTLS: só clientes com certificado assinado por {TLS_CLIENT_CA_ENV}");
            }
            builder = builder.tls_config(tls)?;
        }
        None if std::env::var(TLS_CLIENT_CA_ENV).is_ok() => {
            return Err(format!("{TLS_CLIENT_CA_ENV} exige AI_CLUSTER_TLS_CERT/KEY").into());
        }
        None if insecure_allowed() => {
            println!("AVISO: sem TLS ({INSECURE_ENV}=1); os tokens trafegam em claro");
        }
        None => {
            return Err(format!(
                "sem AI_CLUSTER_TLS_CERT/KEY o controller não sobe; {INSECURE_ENV}=1 libera numa rede fechada"
            )
            .into());
        }
    }
    println!(
        "{}",
        ai_cluster_common::brand::banner(
            "controller",
            env!("CARGO_PKG_VERSION"),
            &format!("gRPC em {addr}")
        )
    );
    println!("controller escutando em {addr} (graça de heartbeat {grace_ms}ms)");
    builder
        .add_service(InterceptedService::new(
            recovery_gate(inference_gate(server, &gate), &recovery_scope),
            auth.clone(),
        ))
        .add_service(InterceptedService::new(
            recovery_gate(inference_gate(probe, &gate), &recovery_scope),
            auth,
        ))
        .serve_with_shutdown(addr, async {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("install SIGTERM handler");
                tokio::select! {
                    _ = term.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            #[cfg(not(unix))]
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    // Desligamento limpo: grava o que o intervalo mínimo entre gravações ainda segurava.
    work.flush_now();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_state_path;
    use std::path::PathBuf;

    #[test]
    fn o_diretorio_de_dados_da_o_padrao_e_o_arquivo_explicito_vence() {
        assert_eq!(resolve_state_path(None, None, "nodes"), None);
        assert_eq!(resolve_state_path(Some(" "), Some(""), "nodes"), None);
        assert_eq!(
            resolve_state_path(None, Some("/var/lib/aic"), "nodes"),
            Some(PathBuf::from("/var/lib/aic/nodes"))
        );
        assert_eq!(
            resolve_state_path(Some("/x/n"), Some("/var/lib/aic"), "nodes"),
            Some(PathBuf::from("/x/n"))
        );
    }
}
