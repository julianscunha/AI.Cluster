mod shards;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ai_cluster_common::matrix::{Matrix, REFERENCE_CORE_GFLOPS, flops};
use ai_cluster_protocol::cluster::{
    CapabilityReport, HealthReport, HealthStreamRequest, HeartbeatRequest, LeaveNodeRequest,
    NodeIdentity, ProbeLinkRequest, RegisterNodeRequest, Release, RequestWorkRequest, StateReplica,
    SubmitWorkResultRequest, WorkResult, WorkUnit,
};
use ai_cluster_protocol::metadata::{ProbeClient, connect_node, connect_probe};
use prost::Message;
use tokio::sync::Semaphore;
use tokio::time;

const DEFAULT_CONTROLLER: &str = "http://127.0.0.1:50051";
const HEARTBEAT_INTERVAL_SECS: u64 = 5;
/// How long a worker waits before asking for work again when it found none. Kept
/// short: a unit enqueued just after a poll should be picked up almost at once.
/// A worker that just finished a unit asks again immediately, without waiting.
const WORK_POLL_IDLE_MS: u64 = 200;

/// Reference per-core matmul throughput for the capability benchmark. A core that
/// measures `REFERENCE_CORE_GFLOPS` scores 1.0, which is exactly what the placement
/// model assumed before a score existed — so enabling the benchmark leaves a cluster
/// of ordinary nodes ranked as it was, while slower and faster cores sort correctly
/// against each other. Defined in `ai_cluster_common::matrix` so the controller can
/// invert the same normalisation to predict time (H5).
/// Matmul edge length for the compute benchmark. Large enough for the O(n^3) kernel to
/// dominate per-call overhead, small enough to keep startup in the tens of milliseconds.
const BENCH_SIZE: u32 = 256;
/// Timed repetitions. The score is the best of these runs, so a few extra iterations buy a
/// less-disturbed sample at a few milliseconds of startup each.
const BENCH_ITERS: u32 = 5;

/// Link rate the network score normalises against: 1 GbE, in Mbps (Fase C1).
///
/// Same shape as the compute score, which `REFERENCE_CORE_GFLOPS` normalises: a node on the
/// reference link scores 1.0, so the placement model's transfer term stays on the scale its
/// 1.0 fallback assumed. The fallback and the reference are the same number on purpose — a
/// node that did not report a rate is treated as being on the reference link, not as having
/// no network.
const REFERENCE_LINK_MBPS: f64 = 1000.0;
/// Upload probe (C2): `PROBE_RUNS` payloads of `PROBE_BYTES`, best run kept. Small enough to
/// leave a slow link usable, large enough that the RPC fixed cost does not dominate.
const PROBE_BYTES: usize = 2 * 1024 * 1024;
const PROBE_RUNS: usize = 3;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Disk probe (K2): 16 MiB written and fsynced, best of `PROBE_RUNS`. The reference is a SATA
/// SSD, the same "1.0 is a reference device" scale as the compute and network scores.
const STORAGE_PROBE_BYTES: usize = 16 * 1024 * 1024;
/// Latency probe: small synced writes, the fastest of `LATENCY_PROBE_RUNS` kept.
const LATENCY_PROBE_BYTES: usize = 4096;
const LATENCY_PROBE_RUNS: usize = 16;
const REFERENCE_STORAGE_MBPS: f64 = 500.0;
/// Memory probe (C2): a 32 MiB copy, well past any cache, best of `PROBE_RUNS`. The reference
/// is ~10 GB/s of single-thread copy, a common DDR4 desktop core.
const MEMORY_PROBE_BYTES: usize = 32 * 1024 * 1024;
const REFERENCE_MEMORY_MBPS: f64 = 10_000.0;

/// The copy of the controller's state this node keeps, if it was asked to keep one (I4).
///
/// The controller hands its snapshot down on a heartbeat when the copy here is behind, and
/// this node offers it back on (re-)registration. That is how a controller whose own volume
/// died recovers the queue: the copy outlives the failure because it lives here, on a node.
///
/// A static because one worker process is one node — the same reason `CURRENT_COMPUTE` is.
static STATE_REPLICA: Mutex<Option<StateReplica>> = Mutex::new(None);

/// Where this node keeps its copy, from `AI_CLUSTER_STATE_REPLICA`. Unset means it keeps
/// none: a plain compute node, with the controller's own files as the only copy of the state.
fn state_replica_path() -> Option<PathBuf> {
    std::env::var("AI_CLUSTER_STATE_REPLICA")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
}

/// Reads the copy a previous run of this node left behind.
///
/// A restarted node has to be able to offer a copy even though it lost its memory, so the
/// file is read at startup. A missing one is the normal case for a node never handed a copy.
/// An unreadable or undecodable one is reported and ignored: a corrupt copy must never be
/// what a controller restores its queue from.
fn load_state_replica(path: &Path) {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            println!("réplica de estado {} ilegível ({err})", path.display());
            return;
        }
    };
    let Ok(replica) = StateReplica::decode(bytes.as_slice()) else {
        println!("réplica de estado {} não decodifica", path.display());
        return;
    };
    println!(
        "réplica de estado carregada de {}: época {} ({} bytes)",
        path.display(),
        replica.epoch_unix_ms,
        replica.snapshot.len()
    );
    *STATE_REPLICA
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(replica);
}

/// Writes the copy the controller just handed down, then remembers it.
///
/// Written to a sibling first and renamed into place, the same way the controller writes its
/// own snapshot: a crash mid-write leaves the previous copy intact instead of a truncated one
/// the next controller would refuse to restore from.
///
/// The epoch is remembered only once the write succeeded. A disk that keeps failing therefore
/// keeps reporting the old epoch and the controller keeps offering the copy — visible noise,
/// instead of a copy that silently stopped being refreshed while looking current.
fn store_state_replica(replica: &StateReplica) {
    let Some(path) = state_replica_path() else {
        return;
    };
    let temp = path.with_extension("tmp");
    if let Err(err) =
        std::fs::write(&temp, replica.encode_to_vec()).and_then(|()| std::fs::rename(&temp, &path))
    {
        println!(
            "guardar réplica de estado em {} falhou: {err}",
            path.display()
        );
        return;
    }
    *STATE_REPLICA
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(replica.clone());
    println!(
        "réplica de estado guardada em {}: época {} ({} bytes)",
        path.display(),
        replica.epoch_unix_ms,
        replica.snapshot.len()
    );
}

/// The copy this node holds, to offer on registration. `None` when it holds none.
fn state_replica() -> Option<StateReplica> {
    STATE_REPLICA
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// File where this node keeps its credential between runs (S5); unset means memory only, and a
/// restart then asks for approval again.
fn credential_file() -> Option<String> {
    std::env::var("AI_CLUSTER_NODE_CREDENTIAL_FILE")
        .ok()
        .filter(|path| !path.trim().is_empty())
}

fn load_credential() -> Option<String> {
    let text = std::fs::read_to_string(credential_file()?).ok()?;
    Some(text.trim().to_string()).filter(|value| !value.is_empty())
}

fn store_credential(value: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let Some(path) = credential_file() else {
        return;
    };
    // Recreated so the 0600 mode applies even to a file left behind with another one.
    let _ = std::fs::remove_file(&path);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut file| file.write_all(value.as_bytes()));
    if let Err(err) = written {
        println!("aviso: não consegui gravar a credencial em {path}: {err}");
    }
}

/// Epoch of the copy this node holds, for the heartbeat request. Zero when it holds none.
///
/// The controller compares this against its own epoch and sends a copy only when its own is
/// newer, so the whole snapshot travels only when the node is genuinely behind — not every
/// beat. Reporting the epoch is also how the node acknowledges a copy it stored.
fn state_replica_epoch() -> u64 {
    STATE_REPLICA
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map_or(0, |replica| replica.epoch_unix_ms)
}

/// Release aceita para instalar: o worker só age se `AI_CLUSTER_UPDATE_CMD` estiver definido, a versão
/// difere da que roda, ainda não foi tentada (evita laço quando a nova volta por rollback) e tudo
/// o que vai ao comando está em caracteres seguros. Downgrade não é detectado aqui (sem ordem
/// total de versões): quem define a release é o operador; assinatura virá depois.
fn accept_update(release: &Release, current: &str, tried: &str) -> bool {
    let safe = |text: &str, extra: &str| {
        !text.is_empty()
            && text.len() <= 200
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
    };
    release.version != current
        && release.version != tried.trim()
        && safe(&release.version, "._-+")
        && safe(&release.url, ":/._-%+")
        && [&release.sha256, &release.image_sha256]
            .iter()
            .all(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
        && release.signature.len() == 88
        && release
            .signature
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c))
}

/// Pergunta ao comando de atualização (`--verify`) se a assinatura da release confere, antes de o
/// worker drenar: sem isso um controller malicioso tira o nó do cluster com uma versão inventada.
async fn update_verified(command: &str, release: &Release) -> bool {
    // Roda no laço do heartbeat: sem prazo, um comando travado faria o controller dar o nó por morto.
    let run = tokio::process::Command::new(command)
        .arg("--verify")
        .args([
            &release.version,
            &release.url,
            &release.sha256,
            &release.image_sha256,
            &release.signature,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .status();
    match time::timeout(Duration::from_secs(10), run).await {
        Ok(Ok(status)) => {
            if !status.success() {
                println!(
                    "conferência da release {} falhou: {status}",
                    release.version
                );
            }
            status.success()
        }
        Ok(Err(err)) => {
            println!("não consegui rodar {command} --verify: {err}");
            false
        }
        Err(_) => {
            println!(
                "conferência da release {} passou de 10 s; recusada",
                release.version
            );
            false
        }
    }
}

fn update_mark_path() -> PathBuf {
    std::env::var_os("AI_CLUSTER_UPDATE_MARK")
        .map_or_else(|| "/data/state/update-tried".into(), Into::into)
}

/// Roda o comando de atualização (sem shell) com versão, URL e SHA-256. Numa imagem A/B ele grava o
/// slot inativo e reinicia; se voltar, a atualização falhou.
fn run_update(command: &str, release: &Release) -> std::io::Result<std::process::ExitStatus> {
    std::process::Command::new(command)
        .args([
            &release.version,
            &release.url,
            &release.sha256,
            &release.image_sha256,
            &release.signature,
        ])
        .status()
}

/// Medida do slot em execução, gravada pelo serviço `aic-measure` (root) antes do worker subir.
/// Vazio fora da imagem A/B ou sem a medição.
fn image_measurement() -> String {
    let path =
        std::env::var("AI_CLUSTER_MEASURE_FILE").unwrap_or_else(|_| "/run/aic-measure".into());
    std::fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .ok()
        .filter(|text| text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or_default()
}

/// A versão que o nó roda: a da imagem (`/etc/aic-release`, gravada no build) ou, sem ela, a do
/// binário. O rollout compara este valor com a release desejada.
fn runtime_version() -> String {
    let path =
        std::env::var("AI_CLUSTER_RELEASE_FILE").unwrap_or_else(|_| "/etc/aic-release".into());
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty() && text.len() <= 64)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("AI_CLUSTER_CONTROLLER").unwrap_or_else(|_| DEFAULT_CONTROLLER.to_string());
    let node_id = node_id();
    let identity = NodeIdentity {
        node_id: node_id.clone(),
        device_id: device_id(),
        architecture: std::env::consts::ARCH.to_string(),
        os_version: os_version(),
        runtime_version: runtime_version(),
        image_sha256: image_measurement(),
        can_update: std::env::var("AI_CLUSTER_UPDATE_CMD").is_ok_and(|c| !c.is_empty()),
    };
    let mut capability = capability();
    // Fase G4: the local scheduler decides how many cores each unit gets. The node runs up to
    // `slots` units at once and splits the budget it reports between them, so one slot keeps
    // the whole budget (the behaviour before slots existed).
    let slots = work_slots();
    capability.work_slots = slots as u32;
    let cores = (core_budget(capability.cpu_threads) / slots).max(1);
    let permits = Arc::new(Semaphore::new(slots));

    let (mut client, credential) = connect_node(endpoint.clone()).await?;
    // A restarted worker presents the credential it saved, so it keeps the approval it earned.
    if let Some(saved) = load_credential() {
        credential.set(&saved);
    }
    // A pinned node measures nothing, the same contract as the pinned compute score.
    // The probe has its own connection; if it cannot open, the node simply reports no rate.
    let mut probe_client = match pinned_network_score() {
        Some(_) => None,
        None => connect_probe(endpoint.clone()).await.ok(),
    };
    if let Some(probe) = probe_client.as_mut() {
        capability.network_achieved_score = probe_link(probe).await;
    }
    // A copy left by a previous run of this node is still worth offering: the controller may
    // have lost its own files while this node was down (I4).
    if let Some(path) = state_replica_path() {
        load_state_replica(&path);
    }
    let registered = client
        .register_node(RegisterNodeRequest {
            identity: Some(identity.clone()),
            capability: Some(capability.clone()),
            replica: state_replica(),
        })
        .await?
        .into_inner();
    credential.set(&registered.node_credential);
    store_credential(&registered.node_credential);
    println!(
        "registrado node_id={node_id} aceito={} cluster_id={} controller={endpoint}",
        registered.accepted, registered.cluster_id
    );

    // Planned departure: stop taking work, then tell the controller so it can hand
    // this node's in-flight units to someone else before the process exits.
    let mut shutdown = tokio::spawn(async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    });

    // F12: weights that no longer match their manifest are refused before the channel opens, so
    // a node never serves slices of a file that changed on disk.
    if let Some(root) = std::env::var_os("AI_CLUSTER_MODEL_DIR") {
        let root = PathBuf::from(root);
        tokio::task::spawn_blocking(move || verify_model_dirs(&root)).await??;
    }

    // Resident weight slices (J5): only a node that was told where the models live opens the
    // channel. The cap bounds the memory the slices may hold.
    let shard_channel = std::env::var_os("AI_CLUSTER_MODEL_DIR").map(|root| {
        let cap_mb = std::env::var("AI_CLUSTER_SHARD_CACHE_MB")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(2048);
        let store = std::sync::Arc::new(ai_cluster_engine::ShardStore::new(
            PathBuf::from(root),
            cap_mb.saturating_mul(1024 * 1024),
            cores,
        ));
        tokio::spawn(shards::run(client.clone(), node_id.clone(), store))
    });

    // Liveness runs on its own interval, not inside the work loop. Sharing the loop
    // means a unit that takes longer than the heartbeat grace starves the heartbeat,
    // and the controller reaps a healthy worker and hands its unit to another node
    // while this one is still computing it.
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    // Set when a unit's result could not be handed in. The unit stays in flight at the
    // controller, so the worker leaves and `leave_node` gives it to another node.
    let broken = Arc::new(AtomicBool::new(false));
    // Release que o controller mandou instalar (F19): o worker drena, sai e só então roda o comando.
    let update: Arc<Mutex<Option<Release>>> = Arc::new(Mutex::new(None));
    let update_cmd = std::env::var("AI_CLUSTER_UPDATE_CMD")
        .ok()
        .filter(|c| !c.is_empty());
    let heartbeat_task = tokio::spawn({
        let mut client = client.clone();
        let node_id = node_id.clone();
        let identity = identity.clone();
        let capability = capability.clone();
        let credential = credential.clone();
        let (broken, update, update_cmd) = (broken.clone(), update.clone(), update_cmd.clone());
        async move {
            let mut ticker = time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
            let mut rejected = String::new(); // versão/assinatura recusada: não reconfere a cada heartbeat
            loop {
                tokio::select! {
                    _ = ticker.tick() => {}
                    _ = &mut stop_rx => break,
                }
                match client
                    .heartbeat(HeartbeatRequest {
                        node_id: node_id.clone(),
                        unix_ms: now_ms(),
                        replica_epoch_unix_ms: state_replica_epoch(),
                    })
                    .await
                {
                    Ok(beat) => {
                        let beat = beat.into_inner();
                        // The controller hands a copy of its state down when the one here is
                        // behind (I4). Stored before the re-registration check: a node that
                        // just came back is exactly the one that should keep the copy. A node
                        // with no replica path configured ignores it.
                        if let Some(replica) = beat.replica {
                            store_state_replica(&replica);
                        }
                        if let Some(release) = beat.release
                            && let Some(command) = update_cmd.as_deref()
                        {
                            let tried =
                                std::fs::read_to_string(update_mark_path()).unwrap_or_default();
                            let key = format!("{}/{}", release.version, release.signature);
                            if accept_update(&release, &runtime_version(), &tried)
                                && rejected != key
                            {
                                if update_verified(command, &release).await {
                                    println!("atualização pedida versão={}", release.version);
                                    *update.lock().unwrap_or_else(|p| p.into_inner()) =
                                        Some(release);
                                    broken.store(true, Ordering::Relaxed);
                                } else {
                                    println!(
                                        "release {} recusada; o nó continua no cluster",
                                        release.version
                                    );
                                    rejected = key;
                                }
                            }
                        }
                        if !beat.acknowledged {
                            println!("controller não conhece {node_id}; re-registrando");
                            match client
                                .register_node(RegisterNodeRequest {
                                    identity: Some(identity.clone()),
                                    capability: Some(capability.clone()),
                                    replica: state_replica(),
                                })
                                .await
                            {
                                Ok(again) => {
                                    let issued = again.into_inner().node_credential;
                                    credential.set(&issued);
                                    store_credential(&issued);
                                }
                                Err(err) => println!("heartbeat: re-registro falhou: {err}"),
                            }
                        }
                    }
                    Err(err) => println!("heartbeat falhou: {err}"),
                }
                if let Err(err) = client
                    .report_health(HealthStreamRequest {
                        node_id: node_id.clone(),
                        health: Some(health()),
                        capability: Some(capability_with_current(&capability)),
                    })
                    .await
                {
                    println!("relatório de saúde falhou: {err}");
                }
            }
        }
    });

    // The work poll runs on its own cadence, decoupled from the heartbeat interval.
    // Sharing the heartbeat's ticker capped throughput at one unit per worker per
    // interval, however fast the compute was: a worker that finished a unit sat idle
    // until the next heartbeat tick. A worker now asks again the moment it finishes,
    // and only waits a short backoff when it found nothing.
    // Consecutive refusals that are not final. An offline mark heals in a heartbeat or two; a
    // node that is draining, quarantined or removed never does, so it gives up and leaves.
    let mut refused = 0u32;
    loop {
        if shutdown.is_finished() || broken.load(Ordering::Relaxed) {
            break;
        }
        // Wait for a free slot before asking: a unit taken with no slot to run it would sit
        // here while another node could have started it.
        let permit = tokio::select! {
            permit = permits.clone().acquire_owned() => permit?,
            _ = &mut shutdown => break,
        };
        let work = match client
            .request_work(RequestWorkRequest {
                node_id: node_id.clone(),
            })
            .await
        {
            Ok(work) => {
                refused = 0;
                work.into_inner()
            }
            // Refused for good (revoked, bad credential): leave cleanly instead of dropping
            // units that are still running.
            Err(err)
                if matches!(
                    err.code(),
                    tonic::Code::PermissionDenied | tonic::Code::Unauthenticated
                ) =>
            {
                println!("pedido de trabalho falhou: {err}; saindo");
                break;
            }
            // Anything else (the controller marked this node offline during a pause or a
            // partition, or is briefly away) heals through the heartbeat, which brings the node
            // back; keep asking instead of quitting for good (F19 soak).
            Err(err) => {
                drop(permit);
                refused += 1;
                if refused >= MAX_REFUSED_REQUESTS {
                    println!("pedido de trabalho falhou {refused} vezes: {err}; saindo");
                    break;
                }
                println!("pedido de trabalho falhou: {err}; tentando de novo");
                tokio::select! {
                    _ = time::sleep(Duration::from_secs(1)) => {}
                    _ = &mut shutdown => break,
                }
                continue;
            }
        };
        match work.work {
            Some(unit) => {
                // The slot is held until the result is submitted, so `slots` bounds the units
                // this node has in flight, and the controller counts it busy at that point.
                let mut client = client.clone();
                let node_id = node_id.clone();
                let broken = broken.clone();
                tokio::spawn(async move {
                    let _slot = permit;
                    let work_id = unit.work_id.clone();
                    // Test hook: hold the unit before computing it so a fault-injection run
                    // can kill this worker while the unit is in flight. Unset means no delay.
                    let delay = work_delay();
                    if !delay.is_zero() {
                        time::sleep(delay).await;
                    }
                    let shape = (unit.rows, unit.shared, unit.cols);
                    let computed =
                        tokio::task::spawn_blocking(move || execute_work(&unit, &node_id, cores))
                            .await;
                    match computed {
                        Err(err) => {
                            println!("trabalho {work_id}: cálculo falhou: {err}");
                            broken.store(true, Ordering::Relaxed);
                        }
                        Ok(Some(mut result)) => {
                            // Test hook (G1): report a wrong checksum so a run can watch the
                            // controller reject the answer and quarantine this node.
                            if std::env::var_os("AI_CLUSTER_CORRUPT_RESULTS").is_some() {
                                result.checksum += 1.0;
                            }
                            let mut sent = None;
                            for attempt in 0..SUBMIT_ATTEMPTS {
                                match client
                                    .submit_work_result(SubmitWorkResultRequest {
                                        result: Some(result.clone()),
                                    })
                                    .await
                                {
                                    Ok(reply) => {
                                        sent = Some(reply.into_inner().accepted);
                                        break;
                                    }
                                    Err(err) => {
                                        println!("trabalho {work_id}: envio falhou: {err}");
                                        if attempt + 1 < SUBMIT_ATTEMPTS {
                                            time::sleep(Duration::from_millis(500)).await;
                                        }
                                    }
                                }
                            }
                            match sent {
                                Some(accepted) => println!(
                                    "trabalho {work_id} shape={}x{}x{} compute_done threads={cores} aceito={accepted}",
                                    shape.0, shape.1, shape.2
                                ),
                                // Still in flight at the controller: leave so it is handed back.
                                None => broken.store(true, Ordering::Relaxed),
                            }
                        }
                        Ok(None) => {
                            println!(
                                "trabalho {work_id} payload não corresponde ao shape declarado; descartado"
                            )
                        }
                    }
                });
            }
            None => {
                drop(permit);
                // Nothing ready: use the idle time to refresh the current-compute estimate,
                // then wait before asking again (Fase C3).
                // The remeasurements steal cores, so they only run with no unit in flight.
                if permits.available_permits() == slots {
                    refresh_current_compute();
                    refresh_current_storage().await;
                    if let Some(probe) = probe_client.as_mut() {
                        refresh_current_network(probe).await;
                    }
                }
                // Wait before asking again, but wake at once on shutdown.
                tokio::select! {
                    _ = time::sleep(Duration::from_millis(WORK_POLL_IDLE_MS)) => {}
                    _ = &mut shutdown => break,
                }
            }
        }
    }

    if let Some(task) = shard_channel {
        task.abort();
    }
    // Let the units already running hand in their results before the node says goodbye.
    // Bounded: a hung unit must not keep a SIGTERM from ever finishing; `leave_node` hands
    // back whatever is still in flight.
    let _ = time::timeout(Duration::from_secs(30), permits.acquire_many(slots as u32)).await;
    // Stop liveness before leaving: a beat that lands after LeaveNode would make the
    // controller re-register this node, undoing the departure.
    stop_tx.send(()).ok();
    let _ = heartbeat_task.await;

    let left = client
        .leave_node(LeaveNodeRequest {
            node_id: node_id.clone(),
        })
        .await?
        .into_inner();
    println!(
        "saiu do cluster node_id={node_id} aceito={} requeued={}",
        left.accepted, left.requeued
    );
    let pending = update.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let (Some(release), Some(command)) = (pending, update_cmd) {
        // Marca antes: se o nó voltar por rollback, não tenta a mesma versão de novo.
        let _ = std::fs::write(update_mark_path(), &release.version);
        // resultado de uma tentativa anterior não vale para esta
        let _ = std::fs::remove_file(update_mark_path().with_file_name("update-result"));
        println!("atualizando para {}", release.version);
        let status = run_update(&command, &release);
        if status.as_ref().is_ok_and(|s| s.success()) {
            // O pedido foi entregue e a máquina vai reiniciar: ficar fora do cluster até lá, em vez de
            // voltar a pegar trabalho. Se o updater recusar (assinatura, versão, espaço), ele deixa
            // o motivo em `update-result` e o worker volta; senão espera até 20 min pelo reinício.
            let result = update_mark_path().with_file_name("update-result");
            for _ in 0..240 {
                if let Ok(reason) = std::fs::read_to_string(&result) {
                    println!("atualização recusada: {}", reason.trim());
                    break;
                }
                time::sleep(Duration::from_secs(5)).await;
            }
        }
        println!(
            "atualização não concluiu versão={} resultado={status:?}",
            release.version
        );
        return Err("atualização falhou".into());
    }
    Ok(())
}

/// Verifies the manifest of every model directory under `root` that has one. A directory without
/// manifest is served as before; one whose files do not match stops the worker.
fn verify_model_dirs(root: &Path) -> Result<(), String> {
    // `AI_CLUSTER_REQUIRE_MANIFEST=1` makes a model directory without a manifest an error too.
    let required = std::env::var("AI_CLUSTER_REQUIRE_MANIFEST")
        .is_ok_and(|value| matches!(value.trim(), "1" | "true"));
    let entries = std::fs::read_dir(root).map_err(|err| format!("{}: {err}", root.display()))?;
    for entry in entries {
        let dir = entry.map_err(|err| err.to_string())?.path();
        if !dir.is_dir() {
            continue;
        }
        if let Some(version) = ai_cluster_engine::manifest::verified_version(&dir, required)? {
            println!(
                "modelo {} versão {version} confere com o manifest",
                dir.display()
            );
        }
    }
    Ok(())
}

/// Rebuilds the matrices from the flat payload, multiplies on `cores` threads, and reports
/// the checksum.
///
/// Returns `None` when the payload lengths disagree with the declared shape.
fn execute_work(unit: &WorkUnit, node_id: &str, cores: usize) -> Option<WorkResult> {
    let expected_a = (unit.rows as usize).checked_mul(unit.shared as usize)?;
    let expected_b = (unit.shared as usize).checked_mul(unit.cols as usize)?;
    if unit.matrix_a.len() != expected_a || unit.matrix_b.len() != expected_b {
        return None;
    }
    let a = Matrix {
        rows: unit.rows,
        cols: unit.shared,
        data: unit.matrix_a.clone(),
    };
    let b = Matrix {
        rows: unit.shared,
        cols: unit.cols,
        data: unit.matrix_b.clone(),
    };
    let started = Instant::now();
    let product = a.multiply_with_threads(&b, cores)?;
    let compute_time_us = started.elapsed().as_micros() as u64;
    // Compute the checksum before `data` is moved out of the product.
    let checksum = product.checksum();
    Some(WorkResult {
        work_id: unit.work_id.clone(),
        node_id: node_id.to_string(),
        rows: product.rows,
        cols: product.cols,
        matrix_c: product.data,
        checksum,
        compute_time_us,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// How long to hold a claimed unit before computing it.
///
/// Fault-injection hook for the failure tests: without a hold, a unit is in flight
/// for microseconds and a worker cannot be reliably killed while holding one. Unset
/// or unparsable means compute immediately, which is the production behaviour.
fn work_delay() -> Duration {
    std::env::var("AI_CLUSTER_WORK_DELAY_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::ZERO)
}

fn node_id() -> String {
    std::env::var("AI_CLUSTER_NODE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| read_trimmed("/etc/hostname"))
        .unwrap_or_else(|| "worker".to_string())
}

/// Device identity is separate from the logical node id and survives re-imaging.
fn device_id() -> String {
    read_trimmed("/etc/machine-id").unwrap_or_else(|| "unknown-device".to_string())
}

fn os_version() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with("PRETTY_NAME="))
                .map(|line| {
                    line.trim_start_matches("PRETTY_NAME=")
                        .trim_matches('"')
                        .to_string()
                })
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "linux".to_string())
}

fn read_trimmed(path: &str) -> Option<String> {
    let value = std::fs::read_to_string(path).ok()?.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn parse_meminfo(content: &str, key: &str) -> Option<u64> {
    content
        .lines()
        .find(|line| line.starts_with(key))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u64>().ok())
}

fn meminfo_kb(key: &str) -> Option<u64> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?, key)
}

/// Cores the local scheduler grants one unit on this node (Fase G4).
///
/// The worker runs one unit at a time, so that unit gets the whole budget. The count the
/// node reports is the ceiling — a node that reports 2 threads must not quietly run 8 —
/// and the host's real parallelism caps it too, so a node that reports more threads than
/// the machine has does not oversubscribe and run slower than serial. Never zero.
/// Tries to hand a finished unit's result to the controller before giving the node up.
const SUBMIT_ATTEMPTS: u32 = 3;

/// Work requests refused in a row (one second apart) before the worker leaves.
const MAX_REFUSED_REQUESTS: u32 = 30;

/// How many units this node runs at once. `AI_CLUSTER_WORK_SLOTS`; unset or unparsable means
/// one, the behaviour before the local queue.
fn work_slots() -> usize {
    std::env::var("AI_CLUSTER_WORK_SLOTS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map_or(1, |slots| slots.clamp(1, 64))
}

fn core_budget(reported_threads: u32) -> usize {
    let available = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    grant_cores(reported_threads, available)
}

/// The grant itself, split from the host lookup so it can be tested exactly.
fn grant_cores(reported_threads: u32, available: usize) -> usize {
    (reported_threads.max(1) as usize).min(available.max(1))
}

/// The registration benchmark as a whole distribution, not just its best case: the score
/// that becomes the baseline capability, plus the median and relative deviation a time
/// prediction needs to calibrate against (`predictions.md`).
///
/// Single-threaded on purpose: the placement model multiplies the score by the reported
/// thread count, so a multithreaded measurement would count the cores twice. Uses the
/// same matmul kernel the worker runs on real units, so the rate is directly comparable
/// to the workload it predicts.
///
/// A pinned score short-circuits the benchmark — the operator chose the number, so nothing
/// runs and the median and deviation stay zero, the same "not measured" as an unmeasured
/// score. Readers fall back to the pinned `compute_score`.
///
/// ponytail: assumes compute scales linearly with threads; turbo and SMT mean a real
/// 4-thread run is usually a bit under 4x the 1-thread rate, so the model is optimistic
/// on big nodes. Revisit with a multithreaded sweep if placement skews to them.
fn baseline_measurement() -> ComputeMeasurement {
    if let Some(forced) = pinned_compute_score() {
        return ComputeMeasurement {
            score: forced,
            median_score: 0.0,
            deviation: 0.0,
            confidence: 0.0,
        };
    }
    measure_compute(BENCH_SIZE, BENCH_ITERS, REFERENCE_CORE_GFLOPS)
}

/// The score `AI_CLUSTER_COMPUTE_SCORE` pins, when set and sensible. Unset, unparsable or
/// non-positive means "not pinned", so the caller measures instead.
///
/// One place reads the override so the baseline and the idle remeasurement agree on what a
/// pinned node means: the operator chose the number, so nothing remeasures it.
fn pinned_compute_score() -> Option<f64> {
    std::env::var("AI_CLUSTER_COMPUTE_SCORE")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
}

/// Result of one compute benchmark: the normalised score and how much the timed runs
/// agreed with each other.
///
/// `score` é o melhor caso — a capability, o que o nó faz quando menos perturbado.
/// `median_score` é a execução típica — o valor contra o qual uma previsão de tempo deve ser
/// calibrada, porque a máquina raramente repete a execução mais sortuda. `confidence` (melhor
/// sobre mediana) e `deviation` (desvio relativo) dizem os dois quanto as execuções
/// discordaram; um benchmark estável tem confiança perto de 1 e desvio perto de 0.
#[derive(Debug, Clone, Copy)]
struct ComputeMeasurement {
    score: f64,
    median_score: f64,
    deviation: f64,
    confidence: f64,
}

/// Times `iters` serial multiplies of a `size`-cubed matmul and normalises the rate.
///
/// Reports the best of the timed runs, not the mean: contention from another container or
/// a noisy neighbour only ever makes a run slower, so the fastest run is the least
/// disturbed and the most reproducible. Averaging drags the rate down whenever the host is
/// shared, which would make the same node look slower from one poll to the next. The cold
/// first run is discarded by the same mechanism, so no separate warm-up pass is needed.
///
/// The best case is the capability, but it is the wrong number to *calibrate a time
/// prediction* against: the machine rarely repeats its luckiest run. So the same runs also
/// yield the median score and the relative deviation beside the best-case score. A prediction
/// that used the best case would understate the time of every unit that ran on a typical,
/// slightly disturbed pass — which is most of them. See `predictions.md`.
///
/// Returns the whole distribution so the caller can report all of it (C3).
fn measure_compute(size: u32, iters: u32, reference: f64) -> ComputeMeasurement {
    let a = Matrix::seeded(size, size, 1);
    let b = Matrix::seeded(size, size, 2);
    if a.multiply(&b).is_none() {
        return ComputeMeasurement {
            score: 0.0,
            median_score: 0.0,
            deviation: 0.0,
            confidence: 0.0,
        };
    }
    let mut times = Vec::with_capacity(iters.max(1) as usize);
    for _ in 0..iters.max(1) {
        let started = Instant::now();
        if a.multiply(&b).is_none() {
            return ComputeMeasurement {
                score: 0.0,
                median_score: 0.0,
                deviation: 0.0,
                confidence: 0.0,
            };
        }
        times.push(started.elapsed().as_secs_f64());
    }
    let best = times.iter().copied().fold(f64::INFINITY, f64::min);
    let median = median_time(&times);
    ComputeMeasurement {
        score: normalize_compute_score(flops(size, size, size) / best / 1e9, reference),
        median_score: normalize_compute_score(flops(size, size, size) / median / 1e9, reference),
        deviation: relative_deviation(&times),
        confidence: measurement_confidence(&times),
    }
}

/// Median of the timed runs, or zero when there is none to take one from.
///
/// The central run rather than an average: one disturbed run drags a mean up but only moves
/// the median by one place. An even count averages the two middle runs, so a pass always has
/// a single representative value. Zero reads as "not measured", the same convention as an
/// unmeasured score — never as "instantaneous".
fn median_time(times: &[f64]) -> f64 {
    if times.is_empty() {
        return 0.0;
    }
    let mut sorted = times.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    }
}

/// Relative deviation of the timed runs: standard deviation over mean — the coefficient of
/// variation. How much the runs disagreed, as a fraction of their own size, so a slow node
/// and a fast node are judged by the same yardstick. Zero means every run took the same time;
/// the larger it is, the more the measurement was disturbed, and the less the median beside
/// it should be trusted. Fewer than two runs cannot disagree, so they report zero.
fn relative_deviation(times: &[f64]) -> f64 {
    let count = times.len();
    if count < 2 {
        return 0.0;
    }
    let n = count as f64;
    let mean = times.iter().sum::<f64>() / n;
    if !mean.is_finite() || mean <= 0.0 {
        return 0.0;
    }
    let variance = times.iter().map(|time| (time - mean).powi(2)).sum::<f64>() / n;
    variance.sqrt() / mean
}

/// How much the timed runs agreed, 0..1, for the confidence a measurement carries (C3).
///
/// Best over median. Every disturbance — another tenant, a background task, a frequency
/// dip — only ever makes a run slower, so the fastest run is the trustworthy one and the
/// gap up to the median is exactly how much disturbance there was. Expressed as a fraction
/// of the best so a slow node and a fast node are judged by the same yardstick. One run
/// cannot disagree with itself, so a single iteration counts as certain; no runs at all
/// counts as no confidence. A best that is zero or not finite reads as no confidence at
/// all, never as a perfect one.
fn measurement_confidence(times: &[f64]) -> f64 {
    if times.is_empty() {
        return 0.0;
    }
    if times.len() == 1 {
        return 1.0;
    }
    let mut sorted = times.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let best = sorted[0];
    if !best.is_finite() || best <= 0.0 {
        return 0.0;
    }
    (best / sorted[sorted.len() / 2]).clamp(0.0, 1.0)
}

/// Normalises a measured GFLOP/s rate to the placement model's per-core score.
///
/// A core at `reference` GFLOP/s scores 1.0 — the value the thread fallback used — so a
/// homogeneous cluster is not re-ranked by turning the benchmark on. A rate that is zero
/// or nonsensical scores 0.0, which the placement model reads as "not measured" and
/// replaces with the fallback; a broken benchmark must not look like a fast node.
fn normalize_compute_score(gflops: f64, reference: f64) -> f64 {
    if !gflops.is_finite() || gflops <= 0.0 || !reference.is_finite() || reference <= 0.0 {
        return 0.0;
    }
    gflops / reference
}

/// Classes de armazenamento em bloco do nó, da mais quente para a mais fria.
///
/// `/sys/block/*/queue/rotational` diz 0 para estado sólido e 1 para disco giratório, mas
/// NVMe e SSD respondem 0 igualmente. O nome do dispositivo é a única pista que o kernel
/// dá sem abrir o dispositivo, então `nvme` vem do prefixo. Loop, ram e device-mapper ficam
/// de fora: existem em qualquer nó e não são armazenamento que o cluster possa usar.
///
/// A classificação é por dispositivo, não por tier: HDD é COLD, SSD/NVMe são WARM, e o HOT
/// é RAM — que já vem em `memory_bytes`. Ver `storage-intelligence`.
fn storage_class(name: &str, rotational: &str) -> Option<&'static str> {
    // zram is compressed RAM used as swap: memory, not storage, and it reports rotational=0.
    if name.starts_with("loop")
        || name.starts_with("ram")
        || name.starts_with("zram")
        || name.starts_with("dm-")
    {
        return None;
    }
    match rotational.trim() {
        // SD cards and eMMC (`mmcblk`) also report rotational=0 but sit well below a SATA SSD
        // on random IO, so they get their own class between SSD and HDD.
        "0" => Some(if name.starts_with("nvme") {
            "nvme"
        } else if name.starts_with("mmcblk") {
            "mmc"
        } else {
            "ssd"
        }),
        "1" => Some("hdd"),
        _ => None,
    }
}

/// Distinct storage classes from `(device, rotational)` pairs, hottest first.
///
/// Split from the directory read so the classification is testable without `/sys/block`.
/// An unreadable rotational file drops the device: "not measured" is not "solid state".
fn storage_classes_from(pairs: impl IntoIterator<Item = (String, String)>) -> Vec<String> {
    let mut classes: Vec<&'static str> = Vec::new();
    for (name, rotational) in pairs {
        if let Some(class) = storage_class(&name, &rotational)
            && !classes.contains(&class)
        {
            classes.push(class);
        }
    }
    classes.sort_by_key(|class| match *class {
        "nvme" => 0,
        "ssd" => 1,
        "mmc" => 2,
        _ => 3,
    });
    classes.into_iter().map(str::to_string).collect()
}

/// Detects the node's storage classes by reading `/sys/block`.
///
/// `AI_CLUSTER_STORAGE_CLASSES` overrides the detection, the way `AI_CLUSTER_CPU_THREADS`
/// and `AI_CLUSTER_MEMORY_BYTES` override their capabilities: without it the answer depends
/// on the host's disks, and a container sees the host's, not its own.
fn storage_classes() -> Vec<String> {
    if let Ok(value) = std::env::var("AI_CLUSTER_STORAGE_CLASSES") {
        return value
            .split(',')
            .map(str::trim)
            .filter(|class| !class.is_empty())
            .map(str::to_string)
            .collect();
    }
    let Ok(entries) = std::fs::read_dir("/sys/block") else {
        return Vec::new();
    };
    let pairs = entries.flatten().filter_map(|entry| {
        let rotational = std::fs::read_to_string(entry.path().join("queue/rotational")).ok()?;
        Some((entry.file_name().to_string_lossy().into_owned(), rotational))
    });
    storage_classes_from(pairs)
}

/// Link speed of the node's fastest non-loopback interface, in Mbps (Fase C1).
///
/// `/sys/class/net/<iface>/speed` is the *negotiated* link rate: what the NIC and the switch
/// agreed on, not what the link achieves under load. A virtual interface — veth in a
/// container, a bridge, a bond member — has no negotiated rate and reports an error or a
/// negative value, so those are skipped rather than read as "no link". An unreadable directory
/// is "not measured", the same convention as an unreported health score. Zero means "not
/// measured", never "no network".
fn link_speed_mbps() -> f64 {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return 0.0;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy() != "lo")
        .filter_map(|entry| {
            std::fs::read_to_string(entry.path().join("speed"))
                .ok()?
                .trim()
                .parse::<f64>()
                .ok()
        })
        .filter(|mbps| mbps.is_finite() && *mbps > 0.0)
        .fold(0.0_f64, f64::max)
}

/// The node's network score for the placement model (Fase C1): the link rate relative to the
/// reference link, the same normalisation the compute score uses against the reference core.
///
/// Nominal link speed, not achieved throughput — what the link *is*, not what it *does*. That
/// makes the number honest about being a ceiling: a link shared with other traffic will do
/// worse. Measuring the achieved rate and the latency is a benchmark's job (`capability-engine`), which
/// is the next rung, not this one.
///
/// Set `AI_CLUSTER_NETWORK_SCORE` to pin it instead of reading the interface, so a placement
/// test can stand a slow-link node next to a fast-link one without two networks. Unset or
/// unparsable means discover. Zero means "not measured".
fn network_score() -> f64 {
    match pinned_network_score() {
        Some(forced) => forced,
        None => normalize_network_score(link_speed_mbps()),
    }
}

/// The score `AI_CLUSTER_NETWORK_SCORE` pins, when set and sensible — the same contract as
/// `AI_CLUSTER_COMPUTE_SCORE`: unset, unparsable or non-positive means "not pinned", so the
/// caller discovers instead. One place reads the override, so a pinned node stays pinned.
fn pinned_network_score() -> Option<f64> {
    std::env::var("AI_CLUSTER_NETWORK_SCORE")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
}

/// Link rate over the reference, split from the lookup so the arithmetic is tested exactly.
/// A missing or nonsense rate is "not measured" (zero), never "no network" — and never an
/// infinite score, which would make the node free to reach.
fn normalize_network_score(mbps: f64) -> f64 {
    if !mbps.is_finite() || mbps <= 0.0 {
        return 0.0;
    }
    mbps / REFERENCE_LINK_MBPS
}

/// Achieved upload rate to the controller over the 1 GbE reference, or zero when the probe
/// failed. Best of a few runs: contention only ever slows a run, so the fastest is the link's
/// real ability, the same reasoning as the compute benchmark's best case.
/// shortcut: upload only and the controller path only, add a download leg when a unit's
/// result transfer needs it.
async fn probe_link(client: &mut ProbeClient) -> f64 {
    let payload = vec![0u8; PROBE_BYTES];
    let mut best = 0.0f64;
    for _ in 0..PROBE_RUNS {
        let started = Instant::now();
        // A stuck controller must not freeze the work loop that calls this while idle.
        let sent = time::timeout(
            PROBE_TIMEOUT,
            client.probe_link(ProbeLinkRequest {
                payload: payload.clone(),
            }),
        )
        .await;
        if !matches!(sent, Ok(Ok(_))) {
            break;
        }
        best = best.max(achieved_mbps(PROBE_BYTES, started.elapsed()));
    }
    normalize_network_score(best)
}

/// Where the storage probes write. `AI_CLUSTER_STORAGE_PROBE_DIR` wins; otherwise the temp dir,
/// unless that is RAM (many embedded images mount `/tmp` as tmpfs, and the probe then measured
/// memory: an SD card doing 6 MB/s scored as 780 MB/s), in which case `/var/tmp`.
fn probe_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("AI_CLUSTER_STORAGE_PROBE_DIR") {
        return dir.into();
    }
    let temp = std::env::temp_dir();
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    if ram_backed(&mounts, &temp) {
        let fallback = std::path::PathBuf::from("/var/tmp");
        if fallback.is_dir() && !ram_backed(&mounts, &fallback) {
            return fallback;
        }
    }
    temp
}

/// Whether `path` lives on a RAM filesystem, from the text of `/proc/self/mounts`: the longest
/// mount point that contains it decides.
fn ram_backed(mounts: &str, path: &std::path::Path) -> bool {
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (_device, point, kind) = (fields.next()?, fields.next()?, fields.next()?);
            path.starts_with(point).then_some((point.len(), kind))
        })
        .max_by_key(|(len, _)| *len)
        .is_some_and(|(_, kind)| matches!(kind, "tmpfs" | "ramfs" | "rootfs"))
}

/// Sequential write rate of the disk under the temp dir over the reference, or zero when the
/// probe could not write. The fsync is what makes it a disk number: without it the page cache
/// answers and every node looks like RAM. Write only; read-back would hit the cache again.
/// shortcut: measures the temp dir's device, not every device in `storage_classes`, upgrade
/// when placement needs a rate per tier.
fn storage_score() -> f64 {
    use std::io::{Seek, Write};
    static PROBE_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = PROBE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = probe_dir().join(format!("ai-cluster-probe-{}-{seq}", std::process::id()));
    let block = vec![0u8; STORAGE_PROBE_BYTES];
    // create_new refuses a pre-planted path or symlink in a shared temp dir; the one handle
    // is rewritten from the start on each run.
    let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    else {
        return 0.0;
    };
    let mut best = 0.0f64;
    for _ in 0..PROBE_RUNS {
        let started = Instant::now();
        let written = file
            .seek(std::io::SeekFrom::Start(0))
            .and_then(|_| file.write_all(&block))
            .and_then(|_| file.sync_all());
        if written.is_err() {
            break;
        }
        // Bytes over seconds is MB/s once the megabit factor is taken back out.
        best = best.max(achieved_mbps(STORAGE_PROBE_BYTES, started.elapsed()) / 8.0);
    }
    let _ = std::fs::remove_file(&path);
    best / REFERENCE_STORAGE_MBPS
}

/// Single-thread copy rate over the reference, or zero when the clock gave nothing usable.
/// Best of N for the same reason as compute: contention only slows a run. Copy, not read,
/// because a read the compiler can see is unused gets optimised away.
/// shortcut: one thread, so it under-reports multi-channel nodes; upgrade when placement needs
/// the aggregate bandwidth of a memory-bound unit.
fn memory_bandwidth_score() -> f64 {
    let source = vec![1u8; MEMORY_PROBE_BYTES];
    let mut target = vec![1u8; MEMORY_PROBE_BYTES]; // non-zero: touches the pages now, no calloc faults in run 1
    let mut best = 0.0f64;
    for _ in 0..PROBE_RUNS {
        let started = Instant::now();
        target.copy_from_slice(std::hint::black_box(&source));
        std::hint::black_box(&target);
        best = best.max(achieved_mbps(MEMORY_PROBE_BYTES, started.elapsed()) / 8.0);
    }
    best / REFERENCE_MEMORY_MBPS
}

/// Latency of a 4 KiB write followed by `fsync`, in milliseconds, or zero when the probe could
/// not write. Small synced writes expose what sequential throughput hides: a seek on a HDD, the
/// flush on a SATA SSD, almost nothing on NVMe.
/// shortcut: write latency only, read latency would hit the page cache; upgrade with O_DIRECT.
fn storage_latency_ms() -> f64 {
    use std::io::{Seek, Write};
    static PROBE_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = PROBE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = probe_dir().join(format!("ai-cluster-latency-{}-{seq}", std::process::id()));
    let block = vec![0u8; LATENCY_PROBE_BYTES];
    let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    else {
        return 0.0;
    };
    let mut best = f64::MAX;
    for _ in 0..LATENCY_PROBE_RUNS {
        let started = Instant::now();
        let written = file
            .seek(std::io::SeekFrom::Start(0))
            .and_then(|_| file.write_all(&block))
            .and_then(|_| file.sync_data());
        if written.is_err() {
            break;
        }
        best = best.min(started.elapsed().as_secs_f64() * 1000.0);
    }
    let _ = std::fs::remove_file(&path);
    if best == f64::MAX { 0.0 } else { best }
}

fn achieved_mbps(bytes: usize, elapsed: Duration) -> f64 {
    bytes as f64 * 8.0 / 1e6 / elapsed.as_secs_f64().max(1e-9)
}

fn capability() -> CapabilityReport {
    // Report fewer threads than the host has so a placement test can stand a weak
    // node next to a strong one without a second machine. Unset means report reality.
    let cpu_threads = std::env::var("AI_CLUSTER_CPU_THREADS")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|threads| threads.get() as u32)
                .unwrap_or(0)
        });
    // Same idea for memory: a placement test can stand a small-memory node next to a
    // large one on a single host. Unset means report reality.
    let memory_bytes = std::env::var("AI_CLUSTER_MEMORY_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or_else(|| meminfo_kb("MemTotal:").map(|kb| kb * 1024).unwrap_or(0));
    let measurement = baseline_measurement();
    let storage_score = storage_score();
    // Only the first measurement is the baseline: a later capability() on a degraded disk
    // must not hide the degradation by re-baselining.
    let mut baseline = BASELINE_STORAGE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if *baseline == 0.0 {
        *baseline = storage_score;
    }
    drop(baseline);
    CapabilityReport {
        cpu_threads,
        memory_bytes,
        compute_score: measurement.score,
        compute_median_score: measurement.median_score,
        compute_deviation: measurement.deviation,
        storage_classes: storage_classes(),
        storage_score,
        storage_latency_ms: storage_latency_ms(),
        memory_bandwidth_score: memory_bandwidth_score(),
        network_score: network_score(),
        precisions: vec!["fp32".to_string()],
        ..Default::default()
    }
}

/// Last compute remeasurement taken while the node was idle (Fase C3).
///
/// The registration measurement is the baseline — what this node could do when it was new.
/// This is the same kernel re-run while idle, so a node that got slower after joining (a
/// noisy neighbour on a shared host, a power cap) shows up even when no temperature sensor
/// denounces it. The heartbeat reports it beside the baseline so a time prediction can use
/// what the node will really do now, not what it could do at registration.
///
/// A static because one worker process is one node. Only the work loop writes it, and only
/// while idle, so the benchmark never steals cores from a unit in flight or corrupts that
/// unit's measured compute time.
static CURRENT_COMPUTE: Mutex<Option<(ComputeMeasurement, Instant)>> = Mutex::new(None);

/// How long between idle remeasurements. The benchmark is heavy enough to disturb a unit in
/// flight, so it only runs when there is nothing to compute; this cadence keeps the
/// steady-state cost near zero without letting the estimate go stale. A test overrides it so
/// it does not have to wait a full interval to see the current score appear.
const RECOMPUTE_INTERVAL_SECS: u64 = 60;

/// Remeasures compute while the node is idle, at most once per `RECOMPUTE_INTERVAL_SECS`.
///
/// Called from the idle branch of the work loop, where the cores are free. A no-op while the
/// last measurement is still fresh. `AI_CLUSTER_RECOMPUTE_INTERVAL_SECS` overrides the
/// cadence; 0 remeasures on every idle pass, which only a test wants.
fn refresh_current_compute() {
    // A pinned score means the operator chose the number — a remeasurement would contradict
    // it and make a determinism knob non-deterministic. A pinned node reports no current
    // estimate, and prediction keeps using the baseline, which is what the pin asked for.
    if pinned_compute_score().is_some() {
        return;
    }
    let interval = recompute_interval();
    let stale = {
        let guard = CURRENT_COMPUTE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match *guard {
            Some((_, at)) => interval == 0 || at.elapsed().as_secs() >= interval,
            None => true,
        }
    };
    if !stale {
        return;
    }
    let measurement = measure_compute(BENCH_SIZE, BENCH_ITERS, REFERENCE_CORE_GFLOPS);
    let mut guard = CURRENT_COMPUTE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some((measurement, Instant::now()));
}

/// Round trip to the controller in milliseconds, from the latest idle probe (F09 E5). Zero is
/// "not measured". Reported in `HealthReport.network_latency_ms`.
static LATENCY_MS: Mutex<f64> = Mutex::new(0.0);

/// Empty probes per latency measurement; the fastest is kept, like the other probes.
const LATENCY_RUNS: usize = 5;

/// Round trip of an empty `ProbeLink`, in milliseconds, best of `LATENCY_RUNS`. Zero when the
/// controller never answered. An empty payload keeps bandwidth out of the number.
async fn probe_latency(client: &mut ProbeClient) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..LATENCY_RUNS {
        let started = Instant::now();
        let sent = time::timeout(
            PROBE_TIMEOUT,
            client.probe_link(ProbeLinkRequest {
                payload: Vec::new(),
            }),
        )
        .await;
        if !matches!(sent, Ok(Ok(_))) {
            break;
        }
        best = best.min(started.elapsed().as_secs_f64() * 1000.0);
    }
    if best == f64::MAX { 0.0 } else { best }
}

/// Latest idle remeasurement of the achieved link rate (Fase C4), with when it was taken.
static CURRENT_NETWORK: Mutex<Option<(f64, Instant)>> = Mutex::new(None);

/// Re-runs the link probe while idle, on the same cadence as the compute remeasurement.
/// A pinned node does not probe, for the same reason as at registration. A failed probe
/// (zero) keeps the previous estimate rather than reporting a dead link.
async fn refresh_current_network(client: &mut ProbeClient) {
    if pinned_network_score().is_some() {
        return;
    }
    let interval = recompute_interval();
    let stale = match *CURRENT_NETWORK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
    {
        Some((_, at)) => interval == 0 || at.elapsed().as_secs() >= interval,
        None => true,
    };
    if !stale {
        return;
    }
    let score = probe_link(client).await;
    let latency = probe_latency(client).await;
    if latency > 0.0 {
        *LATENCY_MS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = latency;
    }
    // The attempt is stamped even when it failed, so a flaky controller is retried on the
    // interval and not on every idle poll; the last good estimate stays.
    let mut guard = CURRENT_NETWORK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let kept = guard.map_or(0.0, |(previous, _)| previous);
    *guard = Some((if score > 0.0 { score } else { kept }, Instant::now()));
}

/// Baseline disk rate taken at registration, and the latest idle remeasurement with when it
/// was taken (storage health). Zero baseline means the probe failed: nothing to compare with.
const STORAGE_NOISE_FLOOR: f64 = 0.8;
static BASELINE_STORAGE: Mutex<f64> = Mutex::new(0.0);
static CURRENT_STORAGE: Mutex<Option<(f64, Instant)>> = Mutex::new(None);

/// Re-runs the disk probe while idle, on the compute cadence. A failed probe (zero) keeps
/// the previous estimate, like the link probe. `AI_CLUSTER_STORAGE_HEALTH` pins the health, so
/// the probe is skipped then. The fsync runs on a blocking thread and with no lock held, so a
/// slow disk delays neither the runtime nor the heartbeat that reads `storage_health`.
async fn refresh_current_storage() {
    if pinned_storage_health().is_some() {
        return;
    }
    let interval = recompute_interval();
    let previous = *CURRENT_STORAGE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if previous.is_some_and(|(_, at)| interval != 0 && at.elapsed().as_secs() < interval) {
        return;
    }
    let score = tokio::task::spawn_blocking(storage_score)
        .await
        .unwrap_or(0.0);
    let kept = previous.map_or(0.0, |(score, _)| score);
    *CURRENT_STORAGE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some((if score > 0.0 { score } else { kept }, Instant::now()));
}

fn pinned_storage_health() -> Option<f64> {
    std::env::var("AI_CLUSTER_STORAGE_HEALTH")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|health| (0.0..=1.0).contains(health))
}

/// Disk rate now over the registration baseline, 0..1 (1 = as fast as when it joined), or
/// zero when either side was not measured. Zero is "not measured", never "dead disk".
/// shortcut: compares the temp dir's device with itself, so it sees slowdown but not SMART
/// wear or errors; upgrade when a node exposes them.
fn storage_health() -> f64 {
    if let Some(pinned) = pinned_storage_health() {
        return pinned;
    }
    let baseline = *BASELINE_STORAGE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = CURRENT_STORAGE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .map_or(0.0, |(score, _)| score);
    if baseline > 0.0 && current > 0.0 {
        // fsync rates are noisy (best of 3 on both sides), so a drop under 20% is noise, not
        // a sick disk: it reads as healthy.
        let ratio = (current / baseline).clamp(0.0, 1.0);
        if ratio >= STORAGE_NOISE_FLOOR {
            1.0
        } else {
            ratio
        }
    } else {
        0.0
    }
}

/// The idle remeasurement cadence in seconds. Unset or unparsable means the default.
fn recompute_interval() -> u64 {
    std::env::var("AI_CLUSTER_RECOMPUTE_INTERVAL_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(RECOMPUTE_INTERVAL_SECS)
}

/// The registered capability with the latest idle remeasurement folded in (Fase C3).
///
/// `compute_score` stays the baseline — what the node measured when it registered — so
/// placement keeps ranking on what a node can do rather than on a noisy instant. The current
/// estimate and its confidence travel beside it for time prediction. Before the first idle
/// remeasurement both current fields stay zero, which every reader treats as "not remeasured"
/// and falls back to the baseline.
fn capability_with_current(baseline: &CapabilityReport) -> CapabilityReport {
    let current = *CURRENT_COMPUTE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut capability = apply_current(baseline, current);
    if let Some((score, _)) = *CURRENT_NETWORK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
    {
        capability.current_network_score = score;
    }
    capability
}

/// The fold itself, split from the static so the two-field update is testable without one.
///
/// `None` leaves both fields at zero — the "not remeasured" state every reader falls back
/// from — rather than copying the baseline into them, which would make a stale node look
/// freshly measured.
///
/// `compute_median_score` and `compute_deviation` are **not** touched here. They describe the
/// same registration benchmark that produced `compute_score`, so they belong to the baseline
/// family: `apply_current` folds the current family in beside it. That is also why they stay
/// put — the registration benchmark is the *stable* number a time prediction can be calibrated
/// against, and a value that moved every heartbeat would be exactly what calibration must not
/// chase (`predictions.md`).
fn apply_current(
    baseline: &CapabilityReport,
    current: Option<(ComputeMeasurement, Instant)>,
) -> CapabilityReport {
    let mut capability = baseline.clone();
    if let Some((measurement, _)) = current {
        capability.current_compute_score = measurement.score;
        capability.compute_confidence = measurement.confidence;
    }
    capability
}

/// Previous /proc sample, for the node metrics that are rates rather than gauges.
///
/// A static because one worker process is one node: there is no second node to confuse the
/// sample with, and the heartbeat task is the only reader.
#[derive(Clone, Copy)]
struct ProcSample {
    cpu_total: u64,
    cpu_idle: u64,
    io_ops: u64,
    at: Instant,
}

static PREVIOUS_SAMPLE: Mutex<Option<ProcSample>> = Mutex::new(None);

/// Total and idle CPU jiffies from the aggregate `cpu` line of /proc/stat.
///
/// Fields are user, nice, system, idle, iowait, irq, softirq, steal. Idle counts iowait
/// too: a core blocked on disk is not doing CPU work. Split from the file read so the
/// parsing is testable without /proc.
fn parse_cpu_times(content: &str) -> Option<(u64, u64)> {
    // The trailing space keeps this off the per-core `cpu0`, `cpu1` lines.
    let line = content.lines().find(|line| line.starts_with("cpu "))?;
    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u64>().ok())
        .collect();
    if values.len() < 4 {
        return None;
    }
    let total: u64 = values.iter().sum();
    let idle = values[3] + values.get(4).copied().unwrap_or(0);
    Some((total, idle))
}

/// Read and write operations completed, summed over /proc/diskstats.
///
/// Reads and writes together, which is what "IOPS" means to someone watching a node.
/// Sectors are not counted: those measure bytes, not operations.
fn parse_disk_ops(content: &str) -> Option<u64> {
    let mut total = 0_u64;
    let mut seen = false;
    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // major, minor, name, reads_completed, reads_merged, sectors_read, ms_reading,
        // writes_completed — so writes_completed is index 7.
        if fields.len() < 8 {
            continue;
        }
        if let (Ok(reads), Ok(writes)) = (fields[3].parse::<u64>(), fields[7].parse::<u64>()) {
            total = total.saturating_add(reads).saturating_add(writes);
            seen = true;
        }
    }
    seen.then_some(total)
}

/// CPU utilization (0..1) and disk IOPS between two samples.
///
/// Pure, so the delta arithmetic is testable without a live /proc. Both counters are
/// monotonic since boot, so the ratio of two readings is the load over that interval.
fn load_between(before: ProcSample, now: ProcSample) -> (f64, f64) {
    let cpu_delta = now.cpu_total.saturating_sub(before.cpu_total);
    let idle_delta = now.cpu_idle.saturating_sub(before.cpu_idle);
    let cpu_utilization = if cpu_delta == 0 {
        0.0
    } else {
        (1.0 - idle_delta as f64 / cpu_delta as f64).clamp(0.0, 1.0)
    };
    let seconds = now.at.saturating_duration_since(before.at).as_secs_f64();
    let io_ops = if seconds <= 0.0 {
        0.0
    } else {
        now.io_ops.saturating_sub(before.io_ops) as f64 / seconds
    };
    (cpu_utilization, io_ops)
}

/// CPU utilization and disk IOPS since the previous report.
///
/// The first call has nothing to difference against and reports zero for both, which reads
/// as "not measured" rather than as an idle node — the same convention as an unreported
/// compute score.
fn node_load(now: Instant) -> (f64, f64) {
    let cpu = std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|content| parse_cpu_times(&content));
    let io = std::fs::read_to_string("/proc/diskstats")
        .ok()
        .and_then(|content| parse_disk_ops(&content));
    let sample = ProcSample {
        cpu_total: cpu.map_or(0, |(total, _)| total),
        cpu_idle: cpu.map_or(0, |(_, idle)| idle),
        io_ops: io.unwrap_or(0),
        at: now,
    };
    let mut previous = PREVIOUS_SAMPLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let load = match (*previous, cpu.is_some()) {
        (Some(before), true) => load_between(before, sample),
        _ => (0.0, 0.0),
    };
    *previous = Some(sample);
    load
}

/// Temperature where thermal pressure starts to count against the health score.
///
/// A generic stand-in. `saude-e-confianca` asks for a baseline specific to the hardware — a laptop core
/// and a server core tolerate different limits — and the worker has no way to know one yet.
const THERMAL_WARN_C: f64 = 80.0;
/// Temperature at which thermal pressure saturates and the node scores the floor.
const THERMAL_CRITICAL_C: f64 = 100.0;
/// Lowest score a measured node can report.
///
/// Zero is not free here: the placement model reads a health score of exactly 0 as "not
/// reported" and skips the penalty, so a node with the worst possible readings would look
/// like a node with no readings at all and pay nothing. Dead nodes are excluded by state.
const MIN_HEALTH_SCORE: f64 = 0.05;

/// Package temperature in Celsius, when the kernel exposes a thermal zone.
///
/// Absent in most containers, so 0 means "no sensor visible", not "0 °C" — the same
/// convention as an unmeasured compute score. Scans a few zones because zone0 is not
/// always the CPU one. `AI_CLUSTER_TEMPERATURE_C` overrides the sensor, the way the other
/// overrides stand in for hardware: without it the thermal term cannot be exercised on a
/// machine whose kernel exposes no thermal zone.
fn temperature_c() -> f64 {
    if let Ok(value) = std::env::var("AI_CLUSTER_TEMPERATURE_C") {
        return value.trim().parse::<f64>().unwrap_or(0.0);
    }
    for zone in 0..4 {
        let path = format!("/sys/class/thermal/thermal_zone{zone}/temp");
        if let Some(raw) = read_trimmed(&path).and_then(|value| value.parse::<f64>().ok()) {
            // Millidegrees on every kernel that exposes this file.
            if raw.is_finite() && raw > 0.0 {
                return raw / 1000.0;
            }
        }
    }
    0.0
}

/// Battery charge (0..1) and whether the node is draining it, from the kernel's power supply
/// class, or `None` for a machine with no battery. `AI_CLUSTER_BATTERY=<percent>:<charging|
/// discharging>` stands in for the hardware, like the temperature override.
fn battery() -> Option<(f64, bool)> {
    if let Ok(value) = std::env::var("AI_CLUSTER_BATTERY") {
        let (percent, state) = value.split_once(':')?;
        let level = percent.trim().parse::<f64>().ok()? / 100.0;
        return Some((level.clamp(0.0, 1.0), state.trim() == "discharging"));
    }
    for entry in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let dir = entry.path();
        if read_trimmed(&dir.join("type").to_string_lossy()).as_deref() != Some("Battery") {
            continue;
        }
        // A battery that reports no usable capacity is skipped, not the end of the search.
        let Some(level) = read_trimmed(&dir.join("capacity").to_string_lossy())
            .and_then(|text| text.parse::<f64>().ok())
        else {
            continue;
        };
        let discharging =
            read_trimmed(&dir.join("status").to_string_lossy()).as_deref() == Some("Discharging");
        return Some(((level / 100.0).clamp(0.0, 1.0), discharging));
    }
    None
}

/// How much a draining battery counts against the node, 0..1: nothing above half charge, rising
/// to 1 at 5%. A node on the wall, or with no battery, pays nothing. The limit is the one the
/// EP asks of a mobile node: it must not be drained for the cluster's sake.
fn battery_pressure(battery: Option<(f64, bool)>) -> f64 {
    match battery {
        Some((level, true)) if level.is_finite() => ((0.5 - level) / (0.5 - 0.05)).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// How much the temperature counts against the node, 0..1.
///
/// Zero below the warning threshold and when no sensor is visible, ramping to 1 at the
/// critical one. A node with no sensor is not a cool node, but it is also not a hot one, and
/// the score cannot punish what it cannot see — the raw temperature stays in the report for
/// whoever can tell the difference.
fn thermal_pressure(temperature_c: f64) -> f64 {
    if !temperature_c.is_finite() || temperature_c <= THERMAL_WARN_C {
        return 0.0;
    }
    ((temperature_c - THERMAL_WARN_C) / (THERMAL_CRITICAL_C - THERMAL_WARN_C)).clamp(0.0, 1.0)
}

/// The node's health score from its measured signals, 0..1, where 1 is pristine.
///
/// The worst domain decides, instead of averaging: a node out of swap or running at 95 °C is
/// in trouble even with half its memory free, and an average would hide the one domain that
/// matters. Floored above zero for the reason in `MIN_HEALTH_SCORE`.
///
/// `saude-e-confianca` lists CPU as a health domain and this score leaves it out. Utilization measures
/// load, not condition: a node that just finished a unit reads as busy at the exact moment it
/// becomes the best place to send the next one. Placement already asks that question — it
/// ranks idle candidates — so load belongs to the placement model, not here. Storage health
/// is the disk rate now over its registration baseline; network degradation is still unmeasured.
fn health_score(
    memory_utilization: f64,
    swap_utilization: f64,
    temperature_c: f64,
    storage_health: f64,
) -> f64 {
    // Zero storage health is "not measured": it does not count against the node.
    let storage_pressure = if storage_health > 0.0 {
        1.0 - storage_health
    } else {
        0.0
    };
    let worst = memory_utilization
        .max(swap_utilization)
        .max(storage_pressure)
        .max(thermal_pressure(temperature_c))
        .clamp(0.0, 1.0);
    (1.0 - worst).max(MIN_HEALTH_SCORE)
}

/// Fraction of swap in use, 0..1. Zero when the node has no swap configured.
fn swap_utilization() -> f64 {
    match (meminfo_kb("SwapTotal:"), meminfo_kb("SwapFree:")) {
        (Some(total), Some(free)) if total > 0 => {
            (1.0 - free as f64 / total as f64).clamp(0.0, 1.0)
        }
        _ => 0.0,
    }
}

/// What the node reports about its own condition (Fase H1, score from C4).
///
/// Every signal is reported raw so a consumer can judge for itself, and the score folds in
/// the domains that are actually measured — memory pressure, swap pressure and thermal
/// pressure. `health_score` carries the reasoning.
fn health() -> HealthReport {
    let memory_utilization = match (meminfo_kb("MemTotal:"), meminfo_kb("MemAvailable:")) {
        (Some(total), Some(available)) if total > 0 => {
            (1.0 - available as f64 / total as f64).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    let (cpu_utilization, disk_io_ops) = node_load(Instant::now());
    let temperature_c = temperature_c();
    let swap_utilization = swap_utilization();
    let storage_health = storage_health();
    let battery = battery();
    // The worst domain decides, so a draining battery only ever lowers the score.
    let battery_cap = (1.0 - battery_pressure(battery)).max(MIN_HEALTH_SCORE);
    HealthReport {
        health_score: health_score(
            memory_utilization,
            swap_utilization,
            temperature_c,
            storage_health,
        )
        .min(battery_cap),
        battery_level: battery.map_or(0.0, |(level, _)| level),
        on_battery: battery.is_some_and(|(_, draining)| draining),
        storage_health,
        memory_utilization,
        cpu_utilization,
        temperature_c,
        swap_utilization,
        disk_io_ops,
        network_latency_ms: *LATENCY_MS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meminfo_values() {
        let sample = "MemTotal:       16384000 kB\nMemAvailable:    8192000 kB\n";
        assert_eq!(parse_meminfo(sample, "MemTotal:"), Some(16_384_000));
        assert_eq!(parse_meminfo(sample, "MemAvailable:"), Some(8_192_000));
        assert_eq!(parse_meminfo(sample, "Absent:"), None);
    }

    #[test]
    fn executes_matrix_work_and_reports_checksum() {
        let unit = WorkUnit {
            work_id: "w1".to_string(),
            rows: 2,
            shared: 2,
            cols: 2,
            matrix_a: vec![1.0, 2.0, 3.0, 4.0],
            matrix_b: vec![5.0, 6.0, 7.0, 8.0],
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
            request_id: String::new(),
            priority: 0,
        };
        let result = execute_work(&unit, "node-a", 4).expect("shapes match");
        assert_eq!(result.work_id, "w1");
        assert_eq!(result.node_id, "node-a");
        assert_eq!(result.rows, 2);
        assert_eq!(result.cols, 2);
        assert_eq!(result.matrix_c, vec![19.0, 22.0, 43.0, 50.0]);
        assert_eq!(result.checksum, 134.0);
    }

    #[test]
    fn rejects_payload_that_disagrees_with_declared_shape() {
        let unit = WorkUnit {
            work_id: "w1".to_string(),
            rows: 2,
            shared: 2,
            cols: 2,
            matrix_a: vec![1.0, 2.0, 3.0],
            matrix_b: vec![5.0, 6.0, 7.0, 8.0],
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
            request_id: String::new(),
            priority: 0,
        };
        assert!(execute_work(&unit, "node-a", 1).is_none());
    }

    #[test]
    fn grants_the_smaller_of_reported_and_available() {
        assert_eq!(grant_cores(8, 4), 4);
        assert_eq!(grant_cores(2, 8), 2);
        assert_eq!(grant_cores(4, 4), 4);
    }

    #[test]
    fn never_grants_zero_cores() {
        assert_eq!(grant_cores(0, 4), 1);
        assert_eq!(grant_cores(8, 0), 1);
    }

    #[test]
    fn a_reference_speed_core_scores_one() {
        assert_eq!(normalize_compute_score(6.0, 6.0), 1.0);
        assert_eq!(normalize_compute_score(12.0, 6.0), 2.0);
        assert_eq!(normalize_compute_score(3.0, 6.0), 0.5);
    }

    #[test]
    fn a_nonsensical_rate_falls_back_instead_of_scoring_high() {
        // 0.0 means "not measured" to the placement model. A negative, NaN or infinite
        // rate must not sneak through as a real score, or a broken benchmark would skew
        // placement toward whichever node failed to measure.
        assert_eq!(normalize_compute_score(0.0, 6.0), 0.0);
        assert_eq!(normalize_compute_score(-1.0, 6.0), 0.0);
        assert_eq!(normalize_compute_score(f64::NAN, 6.0), 0.0);
        assert_eq!(normalize_compute_score(f64::INFINITY, 6.0), 0.0);
        assert_eq!(normalize_compute_score(6.0, 0.0), 0.0);
        assert_eq!(normalize_compute_score(6.0, f64::NAN), 0.0);
    }

    #[test]
    fn the_compute_benchmark_reports_a_positive_rate() {
        // Runs the real kernel on a small matrix so the test stays fast. Catches a shape
        // or loop bug that would silently return 0.0 and leave every node on the thread
        // fallback. The exact rate is host-dependent, so only the sign is asserted.
        let score = measure_compute(32, 1, REFERENCE_CORE_GFLOPS).score;
        assert!(score.is_finite() && score > 0.0, "score was {score}");
    }

    #[test]
    fn confidence_is_full_when_the_runs_agree() {
        // Three identical timings: nothing disturbed the runs.
        assert_eq!(measurement_confidence(&[1.0, 1.0, 1.0]), 1.0);
        // The best run is the fastest and the median sits above it, so the fraction is < 1.
        assert!(measurement_confidence(&[1.0, 1.2, 1.4]) < 1.0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn so_drena_se_o_comando_confere_a_release() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("aic-verify-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path.to_string_lossy().into_owned()
        };
        let release = Release {
            version: "2".into(),
            url: "http://h/x.gz".into(),
            sha256: "a".repeat(64),
            image_sha256: "b".repeat(64),
            signature: format!("{}==", "A".repeat(86)),
        };
        // só aceita se o primeiro argumento for --verify e houver os cinco campos
        let ok = script("ok.sh", r#"[ "$1" = --verify ] && [ $# -eq 6 ]"#);
        let recusa = script("no.sh", "exit 3");
        assert!(update_verified(&ok, &release).await);
        assert!(!update_verified(&recusa, &release).await);
        assert!(!update_verified("/nao/existe", &release).await);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confidence_falls_as_the_runs_disagree() {
        let steady = measurement_confidence(&[1.0, 1.0, 1.0]);
        let noisy = measurement_confidence(&[1.0, 2.0, 2.0]);
        assert!(noisy < steady, "noisy={noisy} steady={steady}");
        // A badly disturbed set clamps near zero instead of going negative or NaN.
        let worst = measurement_confidence(&[1.0, 100.0, 100.0]);
        assert!(worst > 0.0 && worst < 0.1, "worst={worst}");
    }

    #[test]
    fn a_single_run_cannot_disagree_with_itself() {
        assert_eq!(measurement_confidence(&[1.0]), 1.0);
        // No runs at all is no confidence, not a perfect one.
        assert_eq!(measurement_confidence(&[]), 0.0);
    }

    #[test]
    fn so_aceita_release_nova_e_segura() {
        let release = |version: &str, url: &str| Release {
            version: version.into(),
            url: url.into(),
            sha256: "a".repeat(64),
            image_sha256: "b".repeat(64),
            signature: format!("{}==", "A".repeat(86)),
        };
        let ok = release("2", "http://h:8080/x.gz");
        assert!(accept_update(&ok, "1", ""));
        // já roda essa versão, ou já tentou (voltou por rollback): não repete
        assert!(!accept_update(&ok, "2", ""));
        assert!(!accept_update(
            &ok, "1", "2
"
        ));
        // nada de caracteres que um comando poderia interpretar
        assert!(!accept_update(&release("2; reboot", "http://h/x"), "1", ""));
        assert!(!accept_update(&release("2", "http://h/x y"), "1", ""));
        let mut bad = ok.clone();
        bad.sha256 = "xyz".into();
        assert!(!accept_update(&bad, "1", ""));
    }

    #[test]
    fn a_broken_measurement_carries_no_confidence() {
        // A zero or non-finite best reads as "not measured", never as a perfect measurement.
        assert_eq!(measurement_confidence(&[0.0, 0.0]), 0.0);
        assert_eq!(measurement_confidence(&[f64::NAN, 1.0]), 0.0);
        assert_eq!(measurement_confidence(&[f64::INFINITY, 1.0]), 0.0);
    }

    #[test]
    fn a_remeasured_capability_carries_the_current_numbers_beside_the_baseline() {
        let baseline = CapabilityReport {
            cpu_threads: 4,
            compute_score: 2.0,
            compute_median_score: 1.9,
            compute_deviation: 0.05,
            ..Default::default()
        };
        // Not remeasured yet: both current fields stay zero, so readers fall back to the
        // baseline instead of reading a copied baseline as a fresh measurement.
        let fresh = apply_current(&baseline, None);
        assert_eq!(fresh.compute_score, 2.0);
        assert_eq!(fresh.current_compute_score, 0.0);
        assert_eq!(fresh.compute_confidence, 0.0);
        // Remeasured: the baseline survives and the current numbers travel beside it.
        let measured = apply_current(
            &baseline,
            Some((
                ComputeMeasurement {
                    score: 1.5,
                    median_score: 1.4,
                    deviation: 0.05,
                    confidence: 0.8,
                },
                Instant::now(),
            )),
        );
        assert_eq!(measured.compute_score, 2.0);
        assert_eq!(measured.current_compute_score, 1.5);
        assert_eq!(measured.compute_confidence, 0.8);
        // The registration distribution belongs to the baseline family, so folding the
        // current measurement in must not move it: it is the stable number calibration uses.
        assert_eq!(measured.compute_median_score, 1.9);
        assert_eq!(measured.compute_deviation, 0.05);
    }

    #[test]
    fn the_median_sits_between_the_runs_not_at_their_average() {
        // Odd count: the middle run, not the mean (the 100.0 outlier would drag a mean up).
        assert_eq!(median_time(&[3.0, 1.0, 100.0]), 3.0);
        // Even count: the two middle runs averaged, so the pass still has one value.
        assert_eq!(median_time(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        // One run is its own median; none at all is "not measured", never zero seconds.
        assert_eq!(median_time(&[7.0]), 7.0);
        assert_eq!(median_time(&[]), 0.0);
    }

    #[test]
    fn the_deviation_is_zero_for_steady_runs_and_grows_with_the_spread() {
        // Every run the same time: no disagreement to report.
        assert_eq!(relative_deviation(&[2.0, 2.0, 2.0]), 0.0);
        // One run cannot disagree with itself; zero runs have nothing to measure.
        assert_eq!(relative_deviation(&[2.0]), 0.0);
        assert_eq!(relative_deviation(&[]), 0.0);
        // A wider spread is a larger fraction of the mean.
        let tight = relative_deviation(&[1.0, 1.0, 1.1, 1.0]);
        let loose = relative_deviation(&[1.0, 1.0, 3.0, 1.0]);
        assert!(tight > 0.0 && tight < loose);
    }

    #[test]
    fn classifies_block_devices_by_rotation_and_name() {
        // NVMe and SATA SSD both report rotational=0, so the name is the only split.
        assert_eq!(storage_class("nvme0n1", "0"), Some("nvme"));
        assert_eq!(storage_class("sda", "0"), Some("ssd"));
        assert_eq!(storage_class("sdb", "1"), Some("hdd"));
        // An SD card is not an SSD, and compressed RAM is not storage at all.
        assert_eq!(storage_class("mmcblk0", "0"), Some("mmc"));
        assert_eq!(storage_class("zram0", "0"), None);
        // The trailing newline the kernel appends must not turn a disk into "not measured".
        assert_eq!(storage_class("sdc", "1\n"), Some("hdd"));
        // Loop, ram and device-mapper exist on every node and are not usable storage.
        assert_eq!(storage_class("loop0", "1"), None);
        assert_eq!(storage_class("ram0", "0"), None);
        assert_eq!(storage_class("dm-0", "0"), None);
        // An unreadable value is "not measured", never a class.
        assert_eq!(storage_class("sdd", ""), None);
        assert_eq!(storage_class("sde", "2"), None);
    }

    #[test]
    fn storage_classes_are_distinct_and_hottest_first() {
        let pairs = vec![
            ("sdb".to_string(), "1".to_string()),
            ("sda".to_string(), "0".to_string()),
            ("loop0".to_string(), "1".to_string()),
            ("nvme0n1".to_string(), "0".to_string()),
            ("sdc".to_string(), "1".to_string()),
        ];
        assert_eq!(
            storage_classes_from(pairs),
            vec!["nvme".to_string(), "ssd".to_string(), "hdd".to_string()]
        );
        // Nothing readable means "not measured", not "no storage".
        assert!(storage_classes_from(Vec::new()).is_empty());
    }

    #[test]
    fn network_score_is_the_link_over_the_reference() {
        // The reference link is 1.0 by construction: a node that did not report a rate is
        // read as being on the reference, which is where the cost model's fallback sits.
        assert!((normalize_network_score(1000.0) - 1.0).abs() < 1e-9);
        assert!((normalize_network_score(10_000.0) - 10.0).abs() < 1e-9);
        assert!((normalize_network_score(100.0) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn a_draining_battery_costs_health_and_the_wall_costs_nothing() {
        assert_eq!(battery_pressure(None), 0.0);
        assert_eq!(battery_pressure(Some((0.1, false))), 0.0);
        assert_eq!(battery_pressure(Some((0.8, true))), 0.0);
        assert_eq!(battery_pressure(Some((0.5, true))), 0.0);
        assert!((battery_pressure(Some((0.275, true))) - 0.5).abs() < 1e-9);
        assert_eq!(battery_pressure(Some((0.05, true))), 1.0);
        assert_eq!(battery_pressure(Some((0.0, true))), 1.0);
        assert_eq!(battery_pressure(Some((f64::NAN, true))), 0.0);
    }

    #[test]
    fn a_slowed_disk_lowers_health_and_unmeasured_does_not() {
        assert!((health_score(0.0, 0.0, 40.0, 0.25) - 0.25).abs() < 1e-9);
        assert!((health_score(0.0, 0.0, 40.0, 0.0) - 1.0).abs() < 1e-9);
        assert!((health_score(0.0, 0.0, 40.0, 1.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_latency_probe_measures_the_temp_dir_and_cleans_up() {
        let latency = storage_latency_ms();
        assert!(latency > 0.0 && latency < 60_000.0, "{latency}");
        let leftovers = std::fs::read_dir(probe_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("ai-cluster-latency-{}-", std::process::id()))
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn a_tmpfs_temp_dir_is_ram_and_a_disk_is_not() {
        let mounts = "/dev/mmcblk0p1 / ext4 rw 0 0
tmpfs /tmp tmpfs rw 0 0
tmpfs /run tmpfs rw 0 0
";
        assert!(ram_backed(mounts, std::path::Path::new("/tmp")));
        assert!(ram_backed(mounts, std::path::Path::new("/tmp/sub/file")));
        // The longest mount point decides, so a disk mounted under a tmpfs is still a disk.
        assert!(!ram_backed(mounts, std::path::Path::new("/var/tmp")));
        assert!(!ram_backed(mounts, std::path::Path::new("/tmpfoo")));
        assert!(!ram_backed("", std::path::Path::new("/tmp")));
        // The root of an initramfs is RAM too.
        assert!(ram_backed(
            "rootfs / rootfs rw 0 0",
            std::path::Path::new("/var/tmp")
        ));
    }

    #[test]
    fn memory_probe_reports_a_positive_rate() {
        assert!(memory_bandwidth_score() > 0.0);
    }

    #[test]
    fn storage_probe_measures_the_temp_dir_and_cleans_up() {
        assert!(storage_score() > 0.0);
        let leftovers = std::fs::read_dir(probe_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("ai-cluster-probe-")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn achieved_rate_is_megabits_per_second() {
        // 125 MB in one second is 1000 Mbps — the 1 GbE reference.
        assert!((achieved_mbps(125_000_000, Duration::from_secs(1)) - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn network_score_reads_a_missing_rate_as_not_measured() {
        // The kernel reports -1 for an interface without a negotiated rate (veth, bridge),
        // and loopback has none at all: neither is "no network".
        assert_eq!(normalize_network_score(-1.0), 0.0);
        assert_eq!(normalize_network_score(0.0), 0.0);
        // A nonsense rate must not become a free link: an infinite score would make the
        // node cost nothing to reach and win every placement.
        assert_eq!(normalize_network_score(f64::NAN), 0.0);
        assert_eq!(normalize_network_score(f64::INFINITY), 0.0);
    }

    #[test]
    fn thermal_pressure_is_zero_below_the_warning_threshold() {
        assert_eq!(thermal_pressure(0.0), 0.0);
        assert_eq!(thermal_pressure(25.0), 0.0);
        // The threshold itself is still free: the ramp starts above it.
        assert_eq!(thermal_pressure(THERMAL_WARN_C), 0.0);
    }

    #[test]
    fn thermal_pressure_ramps_to_one_at_the_critical_temperature() {
        let mid = (THERMAL_WARN_C + THERMAL_CRITICAL_C) / 2.0;
        assert!((thermal_pressure(mid) - 0.5).abs() < 1e-9);
        assert_eq!(thermal_pressure(THERMAL_CRITICAL_C), 1.0);
        // Past the critical point the penalty saturates instead of exceeding 1.
        assert_eq!(thermal_pressure(120.0), 1.0);
    }

    #[test]
    fn thermal_pressure_ignores_a_missing_or_broken_sensor() {
        // 0 °C means "no sensor visible" in a container, not a cold node — and the score must
        // not punish what it cannot see. The same for a value that failed to parse.
        assert_eq!(thermal_pressure(0.0), 0.0);
        assert_eq!(thermal_pressure(f64::NAN), 0.0);
        assert_eq!(thermal_pressure(f64::NEG_INFINITY), 0.0);
    }

    #[test]
    fn the_worst_domain_decides_the_health_score() {
        // Memory under pressure with everything else free.
        assert!((health_score(0.9, 0.0, 40.0, 0.0) - 0.1).abs() < 1e-9);
        // Swap under pressure with memory free — an average would have hidden this one.
        assert!((health_score(0.1, 0.9, 40.0, 0.0) - 0.1).abs() < 1e-9);
        // Thermal under pressure with memory and swap free.
        assert!((health_score(0.1, 0.0, THERMAL_CRITICAL_C, 0.0) - MIN_HEALTH_SCORE).abs() < 1e-9);
        // A pristine node scores a full 1.0.
        assert!((health_score(0.0, 0.0, 40.0, 0.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_measured_node_never_scores_as_unreported() {
        // Placement reads a score of exactly 0 as "not reported" and skips the penalty, so the
        // worst possible readings must still land above it.
        let worst = health_score(1.0, 1.0, THERMAL_CRITICAL_C, 0.0);
        assert!(worst > 0.0, "worst score was {worst}");
        assert_eq!(worst, MIN_HEALTH_SCORE);
        // Values past their ranges cannot push it through the floor either.
        assert_eq!(health_score(1.5, 2.0, 200.0, 0.0), MIN_HEALTH_SCORE);
    }

    #[test]
    fn a_hot_node_scores_worse_than_a_cool_one_with_equal_memory() {
        // The whole point of the thermal term: same memory, worse condition.
        assert!(health_score(0.2, 0.0, 95.0, 0.0) < health_score(0.2, 0.0, 45.0, 0.0));
    }
}
