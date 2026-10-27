use std::time::{Duration, Instant};

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::{
    ApproveNodeRequest, CancelRequestRequest, EnqueueGraphRequest, EnqueueWorkRequest,
    GetMetricsRequest, GetTraceRequest, ListEventsRequest, ListNodesRequest,
    ListReservationsRequest, Release, ReleaseReservationRequest, ReserveRequest, RevokeNodeRequest,
    SetReleaseRequest, WatchEventsRequest, WorkReportRequest, WorkReportResponse,
};
use ai_cluster_protocol::metadata::{RECOVERY_NODE_SUFFIX, connect_operator};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "ai-cluster")]
struct Cli {
    /// Controller gRPC endpoint.
    #[arg(
        long,
        global = true,
        env = "AI_CLUSTER_CONTROLLER",
        default_value = "http://127.0.0.1:50051"
    )]
    controller: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Status,
    Nodes,
    /// A estrutura do cluster em árvore: o controller e, abaixo, os nós agrupados por papel.
    Topology,
    /// Um nó só, com a capability e a saúde completas (B4).
    Node {
        #[command(subcommand)]
        action: NodeAction,
    },
    /// Cluster-wide roll-up: node counts, queue depth and completed-unit timings.
    Metrics,
    /// Recent lifecycle events, oldest first: joins, departures, offline reaps,
    /// work enqueued and work completed.
    Events {
        /// Most recent N events. 0 means everything the controller still holds.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Depois de listar, continua mostrando os eventos novos à medida que acontecem.
        #[arg(long)]
        follow: bool,
    },
    /// Model files: manifest with version and checksums, and its verification (F12). Local.
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Follow a client request through the cluster (H3).
    Request {
        #[command(subcommand)]
        action: RequestAction,
    },
    /// Dispatch matrix work units and compare compute against transfer cost.
    Benchmark {
        /// Square matrix edge length.
        #[arg(long, default_value_t = 256)]
        size: u32,
        /// Number of work units to dispatch.
        #[arg(long, default_value_t = 1)]
        repetitions: u32,
        /// Compute the same workload in-process instead of dispatching it.
        #[arg(long)]
        local: bool,
        /// Refuse to hand a unit to a node reporting fewer cpu threads than this.
        /// 0 means no requirement.
        #[arg(long, default_value_t = 0)]
        min_threads: u32,
        /// Enqueue under this request id instead of letting the controller mint one.
        /// Pass a reservation's request id so its units land on the held node (G6).
        #[arg(long, default_value = "")]
        request_id: String,
        /// Priority class of the enqueued units: higher dispatches first, 0 is the default (G4).
        #[arg(long, default_value_t = 0)]
        priority: u32,
    },
    /// Dispatch a dependency graph (diamond) and watch the branches gate.
    Graph {
        /// Square matrix edge length for every unit in the graph.
        #[arg(long, default_value_t = 128)]
        size: u32,
    },
    /// Retém um nó para uma requisição futura (G6).
    Reserve {
        /// Nó a reter.
        node: String,
        /// Requisição para quem o nó fica retido.
        request_id: String,
        /// Quanto tempo a reserva vale, em ms. 0 usa o padrão do controller.
        #[arg(long, default_value_t = 0)]
        ttl_ms: u64,
    },
    /// Reservas ativas: nó, requisição dona e vencimento.
    Reservations,
    /// Aprova um nó novo para que ele receba trabalho (S2).
    Approve {
        /// Nó a aprovar.
        node: String,
    },
    /// Retira a confiança e a credencial de um nó e devolve o trabalho dele à fila (S5).
    Revoke {
        /// Nó a revogar.
        node: String,
    },
    /// Define a release que os nós devem rodar: um canário primeiro, os demais depois (F19).
    Rollout {
        /// Versão (ex.: 1.2.0). Vazia, com --cancel, encerra o rollout.
        #[arg(default_value = "")]
        version: String,
        /// URL do artefato (http/https, sem usuário nem query).
        #[arg(long, default_value = "")]
        url: String,
        /// SHA-256 do artefato (64 hexadecimais).
        #[arg(long, default_value = "")]
        sha256: String,
        /// SHA-256 da imagem ext4 descomprimida (ver os/image/sign-release.sh).
        #[arg(long, default_value = "")]
        image_sha256: String,
        /// Assinatura Ed25519 em base64 (ver os/image/sign-release.sh).
        #[arg(long, default_value = "")]
        signature: String,
        /// Cancela o rollout em andamento.
        #[arg(long)]
        cancel: bool,
    },
    /// Libera a reserva de um nó antes do vencimento.
    Release {
        /// Nó a liberar.
        node: String,
    },
    /// Painel de uma tela: nós, fila de trabalho e reservas (H4).
    Dashboard {
        /// Atualiza a cada N segundos até Ctrl-C. 0 imprime uma vez e sai.
        #[arg(long, default_value_t = 0)]
        watch: u64,
    },
}

/// The `request` subcommands, matching the `request trace <id>` surface in the spec.
#[derive(Subcommand, Debug)]
enum ModelAction {
    /// Writes `manifest.json` in a model directory: name, version and the SHA-256 of each file (F12).
    Manifest {
        /// Model directory (the one the gateway and the workers read).
        dir: std::path::PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        version: String,
    },
    /// Checks the files of a model directory against its `manifest.json`.
    Verify { dir: std::path::PathBuf },
}

#[derive(Subcommand, Debug)]
enum RequestAction {
    /// The units a request produced, where each is in its life, and what it cost.
    Trace {
        /// Request id, as printed by `benchmark` or `graph`.
        request_id: String,
    },
    /// Takes a request's units out of the queue. Units already on a worker finish (F11).
    Cancel {
        /// Request id, as printed by `benchmark` or `graph`.
        request_id: String,
    },
}

/// The `node` subcommands, matching the `node show <id>` surface in the spec (B4).
#[derive(Subcommand, Debug)]
enum NodeAction {
    /// Um nó pelo id, com tudo que o controller sabe sobre ele.
    Show {
        /// Node id, as printed by `nodes`.
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Status | Command::Nodes => print_nodes(&cli.controller).await?,
        Command::Topology => print_topology(&cli.controller).await?,
        Command::Node { action } => match action {
            NodeAction::Show { id } => print_node(&cli.controller, &id).await?,
        },
        Command::Metrics => print_metrics(&cli.controller).await?,
        Command::Model { action } => match action {
            ModelAction::Manifest { dir, name, version } => {
                let manifest = ai_cluster_engine::manifest::Manifest::build(&dir, &name, &version)?;
                std::fs::write(
                    dir.join(ai_cluster_engine::manifest::FILE),
                    manifest.to_json(),
                )?;
                for file in &manifest.files {
                    println!("{}  {:>12}  {}", file.sha256, file.bytes, file.path);
                }
                println!(
                    "manifest de {name} {version} gravado ({} arquivos)",
                    manifest.files.len()
                );
            }
            ModelAction::Verify { dir } => {
                match ai_cluster_engine::manifest::Manifest::load(&dir)? {
                    Some(manifest) => {
                        manifest.verify(&dir)?;
                        println!(
                            "ok: {} {} confere ({} arquivos)",
                            manifest.name,
                            manifest.version,
                            manifest.files.len()
                        );
                    }
                    None => return Err(format!("{}: sem manifest.json", dir.display()).into()),
                }
            }
        },
        Command::Events { limit, follow } => print_events(&cli.controller, limit, follow).await?,
        Command::Request { action } => match action {
            RequestAction::Trace { request_id } => {
                print_trace(&cli.controller, &request_id).await?
            }
            RequestAction::Cancel { request_id } => {
                let mut client = connect_operator(cli.controller.clone()).await?;
                let reply = client
                    .cancel_request(CancelRequestRequest {
                        request_id: request_id.clone(),
                    })
                    .await?
                    .into_inner();
                println!(
                    "requisicao={request_id} removidas={} em_voo={}",
                    reply.removed, reply.in_flight
                );
            }
        },
        Command::Benchmark {
            size,
            repetitions,
            local,
            min_threads,
            request_id,
            priority,
        } => {
            run_benchmark(
                &cli.controller,
                size,
                repetitions,
                local,
                min_threads,
                &request_id,
                priority,
            )
            .await?
        }
        Command::Graph { size } => run_graph(&cli.controller, size).await?,
        Command::Reserve {
            node,
            request_id,
            ttl_ms,
        } => run_reserve(&cli.controller, &node, &request_id, ttl_ms).await?,
        Command::Reservations => print_reservations(&cli.controller).await?,
        Command::Approve { node } => approve_node(&cli.controller, &node).await?,
        Command::Revoke { node } => revoke_node(&cli.controller, &node).await?,
        Command::Rollout {
            version,
            url,
            sha256,
            image_sha256,
            signature,
            cancel,
        } => {
            let mut client = connect_operator(cli.controller.clone()).await?;
            let release = (!cancel).then_some(Release {
                version: version.clone(),
                url,
                sha256,
                image_sha256,
                signature,
            });
            client.set_release(SetReleaseRequest { release }).await?;
            println!(
                "ROLLOUT {}",
                if cancel {
                    "cancelado".into()
                } else {
                    format!("versão={version}")
                }
            );
        }
        Command::Release { node } => release_reservation(&cli.controller, &node).await?,
        Command::Dashboard { watch } => print_dashboard(&cli.controller, watch).await?,
    }
    Ok(())
}

/// Enqueues the diamond graph and polls until it drains.
///
/// A single-node run cannot show branch parallelism; what it shows is that the
/// gating works: B and C wait for A, and D waits for both. Run it against two
/// workers to see B and C land on different nodes.
async fn run_graph(endpoint: &str, size: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let response = client
        .enqueue_graph(EnqueueGraphRequest {
            size,
            seed: 42,
            request_id: String::new(),
            keep_result: false,
            priority: 0,
        })
        .await?
        .into_inner();
    println!(
        "grafo enfileirado={} bloqueados={} tamanho={size} requisicao={}",
        response.enqueued, response.blocked, response.request_id
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let report = loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let report = client.work_report(WorkReportRequest {}).await?.into_inner();
        if report.pending == 0 || tokio::time::Instant::now() >= deadline {
            break report;
        }
    };
    print_report(&report);
    Ok(())
}

/// Retém um nó para uma requisição futura (G6).
///
/// A reserva é criada antes de o trabalho existir: o cliente diz "este nó é meu para a
/// requisição X" e depois enfileira a requisição X com esse mesmo id. Um `granted=false`
/// significa que o nó não está disponível para reserva — normalmente porque saiu ou está
/// drenando — e é reportado sem erro, porque não reservar não é uma falha de transporte.
async fn run_reserve(
    endpoint: &str,
    node: &str,
    request_id: &str,
    ttl_ms: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let response = client
        .reserve(ReserveRequest {
            node_id: node.to_string(),
            request_id: request_id.to_string(),
            ttl_ms,
        })
        .await?
        .into_inner();
    if response.granted {
        println!(
            "RESERVA concedida no={} requisicao={} expira_em_unix_ms={}",
            node, request_id, response.expires_at_unix_ms
        );
    } else {
        println!("RESERVA recusada no={node} motivo={}", response.message);
        return Err(format!("nó {node} não está disponível para reserva").into());
    }
    Ok(())
}

/// Lista as reservas ativas: nó, requisição dona e vencimento.
async fn print_reservations(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reservations = client
        .list_reservations(ListReservationsRequest {})
        .await?
        .into_inner()
        .reservations;
    println!("RESERVAS {}", reservations.len());
    println!("{:<20} {:<16} {:>14}", "NÓ", "REQUISIÇÃO", "EXPIRA_EM_MS");
    for reservation in reservations {
        println!(
            "{:<20} {:<16} {:>14}",
            reservation.node_id, reservation.request_id, reservation.expires_at_unix_ms
        );
    }
    Ok(())
}

/// Libera a reserva de um nó antes do vencimento. Um nó sem reserva não é erro.
async fn approve_node(endpoint: &str, node: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let approved = client
        .approve_node(ApproveNodeRequest {
            node_id: node.to_string(),
        })
        .await?
        .into_inner()
        .approved;
    if approved {
        println!("NÓ aprovado no={node}");
    } else {
        println!("NÓ não encontrado no={node}");
    }
    Ok(())
}

async fn revoke_node(endpoint: &str, node: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client
        .revoke_node(RevokeNodeRequest {
            node_id: node.to_string(),
        })
        .await?
        .into_inner();
    if reply.revoked {
        println!("NÓ revogado no={node} reenfileiradas={}", reply.requeued);
    } else {
        println!("NÓ não encontrado no={node}");
    }
    Ok(())
}

async fn release_reservation(endpoint: &str, node: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let released = client
        .release_reservation(ReleaseReservationRequest {
            node_id: node.to_string(),
        })
        .await?
        .into_inner()
        .released;
    if released {
        println!("RESERVA liberada no={node}");
    } else {
        println!("RESERVA não encontrada no={node}");
    }
    Ok(())
}

/// Texto vindo do controller, seguro para a tela do operador: controle (ESC, CSI, OSC) e os
/// caracteres invisíveis que reordenam ou escondem texto viram `?`. O nó escolhe o próprio id e a
/// própria versão ao se registrar, mesmo pendente, então a CLI não pode confiar neles.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            let hidden = matches!(
                c,
                '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
            );
            if c.is_control() || hidden { '?' } else { c }
        })
        .collect()
}

fn sanitize_node(
    mut node: ai_cluster_protocol::cluster::NodeSnapshot,
) -> ai_cluster_protocol::cluster::NodeSnapshot {
    node.state = clean(&node.state);
    if let Some(identity) = node.identity.as_mut() {
        for field in [
            &mut identity.node_id,
            &mut identity.device_id,
            &mut identity.architecture,
            &mut identity.os_version,
            &mut identity.runtime_version,
        ] {
            *field = clean(field);
        }
    }
    if let Some(capability) = node.capability.as_mut() {
        for list in [
            &mut capability.storage_classes,
            &mut capability.precisions,
            &mut capability.accelerators,
        ] {
            list.iter_mut().for_each(|item| *item = clean(item));
        }
        capability.storage_device = clean(&capability.storage_device);
        for disk in &mut capability.storage_devices {
            disk.name = clean(&disk.name);
        }
    }
    node
}

fn sanitize_gateway(
    mut gateway: ai_cluster_protocol::cluster::GatewayInfo,
) -> ai_cluster_protocol::cluster::GatewayInfo {
    gateway.gateway_id = clean(&gateway.gateway_id);
    gateway.url = clean(&gateway.url);
    for model in &mut gateway.models {
        model.model_id = clean(&model.model_id);
        model
            .shard_nodes
            .iter_mut()
            .for_each(|node| *node = clean(node));
    }
    gateway
}

async fn print_nodes(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let nodes = client
        .list_nodes(ListNodesRequest {})
        .await?
        .into_inner()
        .nodes
        .into_iter()
        .map(sanitize_node)
        .collect::<Vec<_>>();
    println!("NÓS {}", nodes.len());
    println!(
        "{:<20} {:<10} {:<10} {:>7} {:>9} {:>7} {:>8} {:>8} {:>7} {:>7} {:<10} {:<12} {:>11} {:<10} {:<8}",
        "NÓ",
        "ESTADO",
        "ARQ",
        "THREADS",
        "RAM_GB",
        "COMPUTE",
        "SAÚDE",
        "CPU_%",
        "TEMP_C",
        "SWAP_%",
        "CONFIANÇA",
        "ARMAZ",
        "ARMAZ_SCORE",
        "VERSÃO",
        "MEDIDA"
    );
    for node in nodes {
        let identity = node.identity.unwrap_or_default();
        let capability = node.capability.unwrap_or_default();
        let health = node.health.unwrap_or_default();
        // Empty means "not measured" — a node whose /sys/block could not be read — so it
        // must not look like a node with no storage.
        let storage = if capability.storage_classes.is_empty() {
            "-".to_string()
        } else {
            capability.storage_classes.join(",")
        };
        println!(
            "{:<20} {:<10} {:<10} {:>7} {:>9.2} {:>7.2} {:>8.2} {:>8.2} {:>7.2} {:>7.2} {:<10} {:<12} {:>11} {:<10} {:<8}",
            identity.node_id,
            node.state,
            identity.architecture,
            capability.cpu_threads,
            capability.memory_bytes as f64 / 1_073_741_824.0,
            capability.compute_score,
            health.health_score,
            health.cpu_utilization * 100.0,
            health.temperature_c,
            health.swap_utilization * 100.0,
            if node.trusted { "aprovado" } else { "pendente" },
            storage,
            // Zero is "not measured" (the probe could not write), never a slow disk.
            if capability.storage_score > 0.0 {
                format!("{:.2}", capability.storage_score)
            } else {
                "-".to_string()
            },
            if identity.runtime_version.is_empty() {
                "-"
            } else {
                &identity.runtime_version
            },
            if node.attested { "confere" } else { "-" },
        );
    }
    Ok(())
}

/// O papel de um nó, pelo que o controller sabe: o id de recuperação (`-rec`) é o boot de rede,
/// que só marca presença; um cache de fatias maior que zero é um nó que guarda partes de modelo.
fn node_role(node_id: &str, shard_cache_bytes: u64) -> &'static str {
    if node_id.ends_with(RECOVERY_NODE_SUFFIX) {
        "Recuperação (só presença, não recebe trabalho)"
    } else if shard_cache_bytes > 0 {
        "Computação + fatias de modelo"
    } else {
        "Computação"
    }
}

/// A árvore do cluster: o controller na raiz, um ramo por papel, um nó por folha. Linhas puras para
/// poder testar. Papéis vazios não aparecem. O gateway não se registra no controller, então não
/// entra aqui.
fn topology_lines(
    controller: &str,
    nodes: &[ai_cluster_protocol::cluster::NodeSnapshot],
    gateways: &[ai_cluster_protocol::cluster::GatewayInfo],
) -> Vec<String> {
    let mut groups: Vec<(&'static str, Vec<String>)> = Vec::new();
    for node in nodes {
        let identity = node.identity.clone().unwrap_or_default();
        let capability = node.capability.clone().unwrap_or_default();
        let role = node_role(&identity.node_id, capability.shard_cache_bytes);
        let mut facts = format!(
            "{} {} {}t {:.1}GB v{}",
            node.state,
            identity.architecture,
            capability.cpu_threads,
            capability.memory_bytes as f64 / 1_073_741_824.0,
            if identity.runtime_version.is_empty() {
                "-"
            } else {
                &identity.runtime_version
            },
        );
        if capability.shard_cache_bytes > 0 {
            facts.push_str(&format!(" cache={}MB", capability.shard_cache_bytes >> 20));
        }
        if !node.trusted {
            facts.push_str(" PENDENTE de aprovação");
        }
        let line = format!("{}  {facts}", identity.node_id);
        match groups.iter_mut().find(|(name, _)| *name == role) {
            Some((_, lines)) => lines.push(line),
            None => groups.push((role, vec![line])),
        }
    }
    // O gateway entra como mais um ramo: o que ele serve e em que nós ficam as fatias.
    if !gateways.is_empty() {
        let lines = gateways
            .iter()
            .map(|gateway| {
                let models = gateway
                    .models
                    .iter()
                    .map(|model| {
                        if model.shard_nodes.is_empty() {
                            format!("{} (inteiro no gateway)", model.model_id)
                        } else {
                            format!(
                                "{} → fatias em {}",
                                model.model_id,
                                model.shard_nodes.join(",")
                            )
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                format!(
                    "{}  {}  {}  {}",
                    gateway.gateway_id,
                    gateway.url,
                    if gateway.alive { "VIVO" } else { "SEM SINAL" },
                    if models.is_empty() {
                        "sem modelos"
                    } else {
                        &models
                    },
                )
            })
            .collect();
        groups.push(("Gateway de inferência", lines));
    }
    let mut out = vec![format!("controller {controller}  ({} nós)", nodes.len())];
    let last_group = groups.len().saturating_sub(1);
    for (g, (role, lines)) in groups.iter().enumerate() {
        let (branch, pad) = if g == last_group {
            ("└─", "   ")
        } else {
            ("├─", "│  ")
        };
        out.push(format!("{branch} {role} ({})", lines.len()));
        for (i, line) in lines.iter().enumerate() {
            let leaf = if i + 1 == lines.len() {
                "└─"
            } else {
                "├─"
            };
            out.push(format!("{pad}{leaf} {line}"));
        }
    }
    out
}

async fn print_topology(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let nodes = client
        .list_nodes(ListNodesRequest {})
        .await?
        .into_inner()
        .nodes
        .into_iter()
        .map(sanitize_node)
        .collect::<Vec<_>>();
    // Controller antigo não conhece o RPC: árvore sem o ramo, sem erro. Qualquer outra falha (rede,
    // permissão) é avisada, para a ausência do gateway não passar por "não há gateway".
    let gateways = match client
        .list_gateways(ai_cluster_protocol::cluster::ListGatewaysRequest {})
        .await
    {
        Ok(response) => response
            .into_inner()
            .gateways
            .into_iter()
            .map(sanitize_gateway)
            .collect(),
        Err(status) if status.code() == tonic::Code::Unimplemented => Vec::new(),
        Err(status) => {
            eprintln!(
                "aviso: não consegui listar os gateways: {}",
                clean(status.message())
            );
            Vec::new()
        }
    };
    for line in topology_lines(endpoint, &nodes, &gateways) {
        println!("{line}");
    }
    Ok(())
}

/// Um nó só, com tudo que o controller sabe sobre ele (B4).
///
/// A tabela de `nodes` cabe numa linha por nó, então deixa de fora o que não cabe: o compute
/// medido, as classes de armazenamento, a pressão de memória e a reserva do nó. Este é o
/// lugar onde esses campos aparecem juntos.
///
/// Sem RPC de nó único: `ListNodes` já traz todos numa chamada só, então filtrar aqui não
/// custa uma ida a mais ao controller. Um id desconhecido é informativo, não erro — o mesmo
/// tratamento de um trace de requisição desconhecida.
///
/// Os campos de saúde que o worker nunca preenche —
/// `thermal_throttling` — ficam de fora. Imprimi-los zerados diria que valem zero, que é
/// diferente de não existirem.
async fn print_node(endpoint: &str, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let node = client
        .list_nodes(ListNodesRequest {})
        .await?
        .into_inner()
        .nodes
        .into_iter()
        .find(|node| {
            node.identity
                .as_ref()
                .is_some_and(|identity| identity.node_id == id)
        });
    let Some(node) = node else {
        println!("NÓ {} não encontrado", clean(id));
        return Ok(());
    };
    let node = sanitize_node(node);
    let identity = node.identity.unwrap_or_default();
    let capability = node.capability.unwrap_or_default();
    let health = node.health.unwrap_or_default();
    let reservation = client
        .list_reservations(ListReservationsRequest {})
        .await?
        .into_inner()
        .reservations
        .into_iter()
        .find(|reservation| reservation.node_id == id);

    println!("NÓ {}", identity.node_id);
    println!(
        "  estado={} arquitetura={} confianca={}",
        node.state,
        identity.architecture,
        if node.trusted { "aprovado" } else { "pendente" }
    );
    println!(
        "  dispositivo={} so={} runtime={}",
        identity.device_id, identity.os_version, identity.runtime_version
    );
    println!(
        "  registrado_em_unix_ms={} ultimo_heartbeat_unix_ms={}",
        node.registered_at_unix_ms, node.last_heartbeat_unix_ms
    );
    println!("CAPABILITY");
    println!(
        "  threads={} memoria_bytes={} compute_score={:.4}",
        capability.cpu_threads, capability.memory_bytes, capability.compute_score
    );
    // Zero means "not remeasured", never "compute zero" — the same convention the ARMAZ
    // column uses for "not measured". A pinned node never remeasures, so it stays `-`.
    let (current, confidence) = if capability.current_compute_score > 0.0 {
        (
            format!("{:.4}", capability.current_compute_score),
            format!("{:.2}", capability.compute_confidence),
        )
    } else {
        ("-".to_string(), "-".to_string())
    };
    println!("  current_compute_score={current} compute_confidence={confidence}");
    // The registration benchmark's distribution: the median is the typical run and the
    // deviation how much the runs disagreed. Zero median means "not measured" (a pinned
    // score runs no benchmark), never "compute zero".
    let median = if capability.compute_median_score > 0.0 {
        format!("{:.4}", capability.compute_median_score)
    } else {
        "-".to_string()
    };
    // Aggregate memory bandwidth with the cores busy together; against the single-thread score above
    // it says whether memory scales with the cores. Zero is "not measured".
    println!(
        "  memory_bandwidth_loaded_score={:.2}",
        capability.memory_bandwidth_loaded_score
    );
    // O_DIRECT read rate and the block device the storage numbers were taken on; zero / empty
    // mean "not measured" (tmpfs refuses O_DIRECT; containers have no block device to name).
    println!(
        "  storage_read_score={:.4} storage_device={}",
        capability.storage_read_score,
        if capability.storage_device.is_empty() {
            "-"
        } else {
            &capability.storage_device
        }
    );
    // Each measured disk, not only the one the probes ran on; zero is "not measured".
    for disk in &capability.storage_devices {
        println!(
            "  disco {}: escrita={:.4} leitura={:.4}",
            disk.name, disk.write_score, disk.read_score
        );
    }
    // Packet loss the TCP stack hid (retransmits) and run-to-run jitter of the link probe; zero
    // jitter is "stable or not measured", the same convention as the deviation above.
    println!(
        "  network_retransmit_ratio={:.4} network_jitter={:.3}",
        capability.network_retransmit_ratio, capability.network_jitter
    );
    // Baseline confidence: 1 = the registration runs agreed, low = the node was disturbed then.
    let baseline = if capability.baseline_confidence > 0.0 {
        format!("{:.3}", capability.baseline_confidence)
    } else {
        "-".to_string()
    };
    println!(
        "  compute_median_score={median} compute_deviation={:.3} baseline_confidence={baseline}",
        capability.compute_deviation
    );
    // Zero means "not measured" — a container's veth has no negotiated link rate, and a
    // pinned node is never read — never "no network", the same convention as the compute
    // fields above. The number is the link relative to the 1 GbE reference.
    let network = if capability.network_score > 0.0 {
        format!("{:.2}", capability.network_score)
    } else {
        "-".to_string()
    };
    let storage_score = if capability.storage_score > 0.0 {
        format!("{:.2}", capability.storage_score)
    } else {
        "-".to_string()
    };
    let memory_score = if capability.memory_bandwidth_score > 0.0 {
        format!("{:.2}", capability.memory_bandwidth_score)
    } else {
        "-".to_string()
    };
    let storage_latency = if capability.storage_latency_ms > 0.0 {
        format!("{:.3}", capability.storage_latency_ms)
    } else {
        "-".to_string()
    };
    println!(
        "  armazenamento={} armaz_score={storage_score} armaz_lat_ms={storage_latency} mem_bw={memory_score} precisoes={} aceleradores={}",
        list_or_dash(&capability.storage_classes),
        list_or_dash(&capability.precisions),
        list_or_dash(&capability.accelerators),
    );
    let achieved = if capability.network_achieved_score > 0.0 {
        format!("{:.2}", capability.network_achieved_score)
    } else {
        "-".to_string()
    };
    let current_network = if capability.current_network_score > 0.0 {
        format!("{:.2}", capability.current_network_score)
    } else {
        "-".to_string()
    };
    println!("  rede_score={network} rede_alcancada={achieved} rede_atual={current_network}");
    println!("SAÚDE");
    println!("  score={:.2}", health.health_score);
    println!(
        "  cpu={:.2}% memoria={:.2}% swap={:.2}%",
        health.cpu_utilization * 100.0,
        health.memory_utilization * 100.0,
        health.swap_utilization * 100.0,
    );
    // Zero is "not measured" (no idle remeasurement yet), never a dead disk.
    let storage_health = if health.storage_health > 0.0 {
        format!("{:.2}", health.storage_health)
    } else {
        "-".to_string()
    };
    // Zero is "not measured" (no idle probe yet), never an instant link.
    let latency = if health.network_latency_ms > 0.0 {
        format!("{:.3}", health.network_latency_ms)
    } else {
        "-".to_string()
    };
    // No battery in view is not an empty one.
    let battery = if health.battery_level > 0.0 || health.on_battery {
        format!(
            "{:.0}%{}",
            health.battery_level * 100.0,
            if health.on_battery {
                " descarregando"
            } else {
                ""
            }
        )
    } else {
        "-".to_string()
    };
    println!(
        "  temperatura_c={:.2} iops={:.2} armaz_saude={storage_health} latencia_ms={latency} bateria={battery}",
        health.temperature_c, health.disk_io_ops
    );
    match reservation {
        Some(reservation) => {
            println!("RESERVA");
            println!(
                "  requisicao={} expira_em_unix_ms={}",
                reservation.request_id, reservation.expires_at_unix_ms
            );
        }
        None => println!("RESERVA nenhuma"),
    }
    Ok(())
}

/// Joins a list for display, or `-` when it is empty. Empty means "not measured" for the
/// storage classes, so it must not read as a blank value.
fn list_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(",")
    }
}

/// Prints the controller's recent lifecycle events, oldest first.
async fn print_events(
    endpoint: &str,
    limit: u32,
    follow: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    // assina antes de listar: nada que aconteça entre as duas chamadas se perde (pode repetir um)
    let mut live = if follow {
        Some(
            client
                .watch_events(WatchEventsRequest {})
                .await?
                .into_inner(),
        )
    } else {
        None
    };
    let events = client
        .list_events(ListEventsRequest { limit })
        .await?
        .into_inner()
        .events;
    println!("EVENTOS {}", events.len());
    println!("{:<14} {:<16} DETALHE", "EM_UNIX_MS", "TIPO");
    for event in events {
        println!(
            "{:<14} {:<16} {}",
            event.at_unix_ms, event.kind, event.detail
        );
    }
    if let Some(live) = live.as_mut() {
        while let Some(event) = live.message().await? {
            println!(
                "{:<14} {:<16} {}",
                event.at_unix_ms, event.kind, event.detail
            );
        }
    }
    Ok(())
}

/// Prints the cluster-wide roll-up from the controller (Fase H1).
///
/// Emits `chave=valor` with ASCII keys rather than a column table: `metrics` is the one
/// command a script or a scraper is likely to read, and positional parsing breaks whenever
/// a label changes. The keys match the proto field names, so the output is stable.
async fn print_metrics(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let metrics = client.get_metrics(GetMetricsRequest {}).await?.into_inner();
    println!(
        "nodes_total={} nodes_available={}",
        metrics.nodes_total, metrics.nodes_available
    );
    println!(
        "units_completed={} units_pending={} units_blocked={} units_failed={}",
        metrics.units_completed, metrics.units_pending, metrics.units_blocked, metrics.units_failed
    );
    println!(
        "total_compute_time_us={} avg_compute_time_us={} total_predicted_compute_us={}",
        metrics.total_compute_time_us,
        metrics.avg_compute_time_us,
        metrics.total_predicted_compute_us
    );
    println!(
        "total_latency_us={} avg_latency_us={}",
        metrics.total_latency_us, metrics.avg_latency_us
    );
    // H1: a média esconde a cauda. p50 é a mediana; p95 mostra o que a cauda sente.
    println!(
        "p50_compute_time_us={} p95_compute_time_us={}",
        metrics.p50_compute_time_us, metrics.p95_compute_time_us
    );
    println!(
        "p50_latency_us={} p95_latency_us={}",
        metrics.p50_latency_us, metrics.p95_latency_us
    );
    Ok(())
}

/// Painel de uma tela (H4): nós, fila de trabalho e reservas ativas no mesmo lugar.
///
/// Junta o que o controlador já expõe. Os campos do painel da spec que ainda não têm
/// fonte — TTFT, ITL, tokens/s, tokens evitados, KV cache, previsto vs real — não
/// aparecem: imprimi-los zerados diria que valem zero, que é diferente de não existirem.
///
/// Sem `--watch` a vista é impressa uma vez e o comando sai, para poder ser lido por um
/// script. Com `--watch N` ela se repete a cada N segundos até Ctrl-C, e a tela é limpa
/// entre uma volta e outra para o painel não virar um log.
async fn print_dashboard(
    endpoint: &str,
    watch_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    loop {
        let nodes = client
            .list_nodes(ListNodesRequest {})
            .await?
            .into_inner()
            .nodes;
        let metrics = client.get_metrics(GetMetricsRequest {}).await?.into_inner();
        let reservations = client
            .list_reservations(ListReservationsRequest {})
            .await?
            .into_inner()
            .reservations;

        if watch_secs > 0 {
            // Home + limpa a tela, para a volta seguinte sobrescrever a anterior.
            print!("\x1b[2J\x1b[H");
        }

        println!(
            "PAINEL nós_disponíveis={}/{}",
            metrics.nodes_available, metrics.nodes_total
        );
        println!();
        println!(
            "{:<20} {:<12} {:>7} {:>9} {:>8} {:>8} {:>7} {:>7}",
            "NÓ", "ESTADO", "THREADS", "RAM_GB", "COMPUTE", "SAÚDE", "CPU_%", "TEMP_C"
        );
        for node in nodes {
            let identity = node.identity.unwrap_or_default();
            let capability = node.capability.unwrap_or_default();
            let health = node.health.unwrap_or_default();
            println!(
                "{:<20} {:<12} {:>7} {:>9.2} {:>8.2} {:>8.2} {:>7.2} {:>7.2}",
                identity.node_id,
                node.state,
                capability.cpu_threads,
                capability.memory_bytes as f64 / 1_073_741_824.0,
                capability.compute_score,
                health.health_score,
                health.cpu_utilization * 100.0,
                health.temperature_c,
            );
        }
        println!();
        println!(
            "TRABALHO concluídas={} pendentes={} bloqueadas={} falhas={}",
            metrics.units_completed,
            metrics.units_pending,
            metrics.units_blocked,
            metrics.units_failed
        );
        println!(
            "TEMPO compute_total_us={} compute_media_us={} latencia_total_us={} latencia_media_us={} previsto_total_us={}",
            metrics.total_compute_time_us,
            metrics.avg_compute_time_us,
            metrics.total_latency_us,
            metrics.avg_latency_us,
            metrics.total_predicted_compute_us
        );
        println!();
        println!("RESERVAS {}", reservations.len());
        for reservation in reservations {
            println!(
                "  {:<20} requisição={} expira_em_unix_ms={}",
                reservation.node_id, reservation.request_id, reservation.expires_at_unix_ms
            );
        }

        if watch_secs == 0 {
            return Ok(());
        }
        // Ctrl-C encerra o processo pelo sinal padrão: não é preciso tratá-lo aqui.
        tokio::time::sleep(Duration::from_secs(watch_secs)).await;
    }
}

/// Prints the units a client request produced and what each one cost (H3).
///
/// The command that answers "why did this request take so long": every unit's latency is
/// split into the time the worker spent computing and the time spent getting the matrices
/// there and back. A unit still in flight or failed has no timings yet, so it shows zero —
/// its row is there to say the unit exists and where it is, not to report a cost.
async fn print_trace(endpoint: &str, request_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let response = client
        .get_trace(GetTraceRequest {
            request_id: request_id.to_string(),
        })
        .await?
        .into_inner();
    println!(
        "REQUISIÇÃO {} UNIDADES {}",
        response.request_id,
        response.units.len()
    );
    if response.units.is_empty() {
        println!("(nenhuma unidade: id desconhecido ou trace perdido por restart)");
        return Ok(());
    }
    println!(
        "{:<10} {:<14} {:<11} {:>12} {:>12} {:>12} {:>12}",
        "TRABALHO", "NÓ", "ESTADO", "COMPUTE_US", "PREDITO_US", "TRANSFER_US", "TOTAL_US"
    );
    let mut compute_total = 0_u64;
    let mut predicted_total = 0_u64;
    let mut transfer_total = 0_u64;
    let mut latency_total = 0_u64;
    for unit in &response.units {
        compute_total += unit.compute_time_us;
        predicted_total += unit.predicted_compute_us;
        transfer_total += unit.transfer_time_us;
        latency_total += unit.total_latency_us;
        println!(
            "{:<10} {:<14} {:<11} {:>12} {:>12} {:>12} {:>12}",
            unit.work_id,
            if unit.node_id.is_empty() {
                "-"
            } else {
                unit.node_id.as_str()
            },
            unit.status,
            unit.compute_time_us,
            unit.predicted_compute_us,
            unit.transfer_time_us,
            unit.total_latency_us,
        );
    }
    println!(
        "unidades={} compute_somado_us={} transfer_somado_us={} latencia_somada_us={}",
        response.units.len(),
        compute_total,
        transfer_total,
        latency_total,
    );
    // H5: previsto vs real. Só faz sentido quando algo foi previsto: um nó sem compute
    // score reporta zero, e zero ali significa "não previsto", não "instantâneo". Uma
    // unidade não concluída também conta zero dos dois lados e sai da conta sozinha.
    if predicted_total > 0 {
        let error_pct =
            (compute_total as f64 - predicted_total as f64) / predicted_total as f64 * 100.0;
        println!(
            "PREVISTO somado_us={} erro_pct={:.1}",
            predicted_total, error_pct
        );
    }
    Ok(())
}

async fn run_benchmark(
    endpoint: &str,
    size: u32,
    repetitions: u32,
    local: bool,
    min_threads: u32,
    request_id: &str,
    priority: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    // Version and parameters travel with the output so two runs are comparable.
    println!(
        "versao_benchmark={} escopo={} tamanho={size} repeticoes={repetitions} semente=42",
        env!("CARGO_PKG_VERSION"),
        if local { "no-unico" } else { "distribuido" }
    );
    if local {
        return run_local_baseline(size, repetitions);
    }
    let mut client = connect_operator(endpoint.to_string()).await?;
    let response = client
        .enqueue_work(EnqueueWorkRequest {
            rows: size,
            shared: size,
            cols: size,
            seed: 42,
            repetitions,
            min_cpu_threads: min_threads,
            request_id: request_id.to_string(),
            priority,
        })
        .await?
        .into_inner();
    println!(
        "enfileiradas {} unidades de trabalho de {size}x{size} requisicao={}",
        response.enqueued, response.request_id
    );

    // Poll until the queue drains. The deadline keeps a stuck worker from hanging the CLI.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let report = loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let report = client.work_report(WorkReportRequest {}).await?.into_inner();
        if report.pending == 0 || tokio::time::Instant::now() >= deadline {
            break report;
        }
    };
    print_report(&report);
    Ok(())
}

fn print_report(report: &WorkReportResponse) {
    println!(
        "{:<10} {:<14} {:>12} {:>12} {:>12}",
        "TRABALHO", "NÓ", "COMPUTE_US", "TRANSFER_US", "TOTAL_US"
    );
    let mut compute_total = 0_u64;
    let mut transfer_total = 0_u64;
    for entry in &report.completed {
        compute_total += entry.compute_time_us;
        transfer_total += entry.transfer_time_us;
        println!(
            "{:<10} {:<14} {:>12} {:>12} {:>12}",
            entry.work_id,
            entry.node_id,
            entry.compute_time_us,
            entry.transfer_time_us,
            entry.total_latency_us,
        );
    }
    let count = report.completed.len().max(1) as u64;
    println!(
        "concluidas={} pendentes={} falharam={} media_compute_us={} media_transfer_us={}",
        report.completed.len(),
        report.pending,
        report.failed,
        compute_total / count,
        transfer_total / count,
    );
}

/// Runs the same matrices in-process on this host as the comparison baseline.
///
/// The distributed run's transfer time only means something next to the time the
/// same work takes without dispatch, serialization or network.
fn run_local_baseline(size: u32, repetitions: u32) -> Result<(), Box<dyn std::error::Error>> {
    let a = Matrix::seeded(size, size, 42);
    let b = Matrix::seeded(size, size, 42 ^ 0x9E37_79B9_7F4A_7C15);
    let runs = repetitions.max(1);
    let mut total = Duration::ZERO;
    for _ in 0..runs {
        let started = Instant::now();
        let product = a.multiply(&b).expect("square matrices align");
        total += started.elapsed();
        // Keep the optimiser from deleting the work.
        std::hint::black_box(product.checksum());
    }
    let avg_us = total.as_micros() / u128::from(runs);
    println!("baseline local {size}x{size} repeticoes={runs}");
    println!(
        "{:<12} {:>14} {:>12}",
        "ESCOPO", "MEDIA_COMPUTE_US", "TOTAL_US"
    );
    println!(
        "{:<12} {:>14} {:>12}",
        "no-unico",
        avg_us,
        total.as_micros()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_cluster_protocol::cluster::{CapabilityReport, NodeIdentity, NodeSnapshot};

    fn node(id: &str, shard_cache_bytes: u64, trusted: bool) -> NodeSnapshot {
        NodeSnapshot {
            identity: Some(NodeIdentity {
                node_id: id.into(),
                architecture: "arm".into(),
                runtime_version: "11.0".into(),
                ..Default::default()
            }),
            capability: Some(CapabilityReport {
                cpu_threads: 4,
                shard_cache_bytes,
                ..Default::default()
            }),
            state: "READY".into(),
            trusted,
            ..Default::default()
        }
    }

    #[test]
    fn texto_do_controller_nao_leva_controle_nem_invisiveis_para_a_tela() {
        assert_eq!(clean("ok-1"), "ok-1");
        assert_eq!(clean("a\u{1b}[2Jb"), "a?[2Jb");
        assert_eq!(clean("x\u{202E}y\u{200B}z\u{7f}"), "x?y?z?");
        let id = sanitize_node(node("n\u{1b}]52;c;x\u{7}", 0, true))
            .identity
            .unwrap()
            .node_id;
        assert!(!id.chars().any(char::is_control), "{id:?}");
    }

    #[test]
    fn o_papel_vem_do_id_de_recuperacao_e_do_cache_de_fatias() {
        assert!(node_role("pi-rec", 1 << 30).starts_with("Recuperação"));
        assert_eq!(node_role("pi", 1 << 30), "Computação + fatias de modelo");
        assert_eq!(node_role("pi", 0), "Computação");
    }

    #[test]
    fn a_arvore_agrupa_por_papel_e_marca_o_pendente() {
        let nodes = [
            node("a", 2 << 30, true),
            node("b", 0, false),
            node("c", 0, true),
        ];
        let tree = topology_lines("http://x:1", &nodes, &[]);
        assert_eq!(tree[0], "controller http://x:1  (3 nós)");
        assert_eq!(tree[1], "├─ Computação + fatias de modelo (1)");
        assert!(tree[2].starts_with("│  └─ a  READY arm 4t"));
        assert!(tree[2].contains("cache=2048MB"));
        assert_eq!(tree[3], "└─ Computação (2)");
        assert!(tree[4].starts_with("   ├─ b ") && tree[4].contains("PENDENTE"));
        assert!(tree[5].starts_with("   └─ c "));
        assert_eq!(topology_lines("u", &[], &[]), vec!["controller u  (0 nós)"]);
    }

    #[test]
    fn o_gateway_aparece_como_ramo_com_modelos_e_fatias() {
        use ai_cluster_protocol::cluster::{GatewayInfo, GatewayModel};
        let gateway = GatewayInfo {
            gateway_id: "gw1".into(),
            url: "http://gw:8080".into(),
            alive: true,
            models: vec![
                GatewayModel {
                    model_id: "gpt2".into(),
                    shard_nodes: vec![],
                },
                GatewayModel {
                    model_id: "smollm2".into(),
                    shard_nodes: vec!["a".into(), "b".into()],
                },
            ],
            ..Default::default()
        };
        let tree = topology_lines("u", &[node("a", 0, true)], &[gateway]);
        assert_eq!(tree[3], "└─ Gateway de inferência (1)");
        assert!(tree[4].contains("gw1  http://gw:8080  VIVO"));
        assert!(tree[4].contains("gpt2 (inteiro no gateway); smollm2 → fatias em a,b"));
    }
}
