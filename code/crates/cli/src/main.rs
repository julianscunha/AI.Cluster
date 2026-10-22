use std::time::{Duration, Instant};

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::{
    EnqueueGraphRequest, EnqueueWorkRequest, GetMetricsRequest, GetTraceRequest, ListEventsRequest,
    ListNodesRequest, ListReservationsRequest, ReleaseReservationRequest, ReserveRequest,
    WorkReportRequest, WorkReportResponse,
};
use ai_cluster_protocol::metadata::connect_controller;
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
enum RequestAction {
    /// The units a request produced, where each is in its life, and what it cost.
    Trace {
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
        Command::Node { action } => match action {
            NodeAction::Show { id } => print_node(&cli.controller, &id).await?,
        },
        Command::Metrics => print_metrics(&cli.controller).await?,
        Command::Events { limit } => print_events(&cli.controller, limit).await?,
        Command::Request { action } => match action {
            RequestAction::Trace { request_id } => {
                print_trace(&cli.controller, &request_id).await?
            }
        },
        Command::Benchmark {
            size,
            repetitions,
            local,
            min_threads,
            request_id,
        } => {
            run_benchmark(
                &cli.controller,
                size,
                repetitions,
                local,
                min_threads,
                &request_id,
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
    let mut client = connect_controller(endpoint.to_string()).await?;
    let response = client
        .enqueue_graph(EnqueueGraphRequest {
            size,
            seed: 42,
            request_id: String::new(),
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
    let mut client = connect_controller(endpoint.to_string()).await?;
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
    let mut client = connect_controller(endpoint.to_string()).await?;
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
async fn release_reservation(endpoint: &str, node: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_controller(endpoint.to_string()).await?;
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

async fn print_nodes(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_controller(endpoint.to_string()).await?;
    let nodes = client
        .list_nodes(ListNodesRequest {})
        .await?
        .into_inner()
        .nodes;
    println!("NÓS {}", nodes.len());
    println!(
        "{:<20} {:<10} {:<10} {:>7} {:>9} {:>7} {:>8} {:>8} {:>7} {:>7} {:<12}",
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
        "ARMAZ"
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
            "{:<20} {:<10} {:<10} {:>7} {:>9.2} {:>7.2} {:>8.2} {:>8.2} {:>7.2} {:>7.2} {:<12}",
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
            storage,
        );
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
/// Os campos de saúde que o worker nunca preenche — `storage_health`, `network_latency_ms`,
/// `thermal_throttling` — ficam de fora. Imprimi-los zerados diria que valem zero, que é
/// diferente de não existirem.
async fn print_node(endpoint: &str, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_controller(endpoint.to_string()).await?;
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
        println!("NÓ {id} não encontrado");
        return Ok(());
    };
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
        "  estado={} arquitetura={}",
        node.state, identity.architecture
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
    println!(
        "  compute_median_score={median} compute_deviation={:.3}",
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
    println!(
        "  armazenamento={} precisoes={} aceleradores={}",
        list_or_dash(&capability.storage_classes),
        list_or_dash(&capability.precisions),
        list_or_dash(&capability.accelerators),
    );
    println!("  rede_score={network}");
    println!("SAÚDE");
    println!("  score={:.2}", health.health_score);
    println!(
        "  cpu={:.2}% memoria={:.2}% swap={:.2}%",
        health.cpu_utilization * 100.0,
        health.memory_utilization * 100.0,
        health.swap_utilization * 100.0,
    );
    println!(
        "  temperatura_c={:.2} iops={:.2}",
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
async fn print_events(endpoint: &str, limit: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_controller(endpoint.to_string()).await?;
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
    Ok(())
}

/// Prints the cluster-wide roll-up from the controller (Fase H1).
///
/// Emits `chave=valor` with ASCII keys rather than a column table: `metrics` is the one
/// command a script or a scraper is likely to read, and positional parsing breaks whenever
/// a label changes. The keys match the proto field names, so the output is stable.
async fn print_metrics(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_controller(endpoint.to_string()).await?;
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
    let mut client = connect_controller(endpoint.to_string()).await?;
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
    let mut client = connect_controller(endpoint.to_string()).await?;
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
    let mut client = connect_controller(endpoint.to_string()).await?;
    let response = client
        .enqueue_work(EnqueueWorkRequest {
            rows: size,
            shared: size,
            cols: size,
            seed: 42,
            repetitions,
            min_cpu_threads: min_threads,
            request_id: request_id.to_string(),
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
