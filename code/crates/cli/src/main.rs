use std::time::{Duration, Instant};

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::{
    EnqueueGraphRequest, EnqueueWorkRequest, GetMetricsRequest, GetTraceRequest, ListEventsRequest,
    ListNodesRequest, WorkReportRequest, WorkReportResponse,
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
    },
    /// Dispatch a dependency graph (diamond) and watch the branches gate.
    Graph {
        /// Square matrix edge length for every unit in the graph.
        #[arg(long, default_value_t = 128)]
        size: u32,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Status | Command::Nodes => print_nodes(&cli.controller).await?,
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
        } => run_benchmark(&cli.controller, size, repetitions, local, min_threads).await?,
        Command::Graph { size } => run_graph(&cli.controller, size).await?,
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

async fn print_nodes(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_controller(endpoint.to_string()).await?;
    let nodes = client
        .list_nodes(ListNodesRequest {})
        .await?
        .into_inner()
        .nodes;
    println!("NÓS {}", nodes.len());
    println!(
        "{:<20} {:<10} {:<10} {:>7} {:>9} {:>7} {:>8}",
        "NÓ", "ESTADO", "ARQ", "THREADS", "RAM_GB", "COMPUTE", "SAÚDE"
    );
    for node in nodes {
        let identity = node.identity.unwrap_or_default();
        let capability = node.capability.unwrap_or_default();
        let health = node.health.unwrap_or_default();
        println!(
            "{:<20} {:<10} {:<10} {:>7} {:>9.2} {:>7.2} {:>8.2}",
            identity.node_id,
            node.state,
            identity.architecture,
            capability.cpu_threads,
            capability.memory_bytes as f64 / 1_073_741_824.0,
            capability.compute_score,
            health.health_score,
        );
    }
    Ok(())
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
        "total_compute_time_us={} avg_compute_time_us={}",
        metrics.total_compute_time_us, metrics.avg_compute_time_us
    );
    println!(
        "total_latency_us={} avg_latency_us={}",
        metrics.total_latency_us, metrics.avg_latency_us
    );
    Ok(())
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
        "{:<10} {:<14} {:<11} {:>12} {:>12} {:>12}",
        "TRABALHO", "NÓ", "ESTADO", "COMPUTE_US", "TRANSFER_US", "TOTAL_US"
    );
    let mut compute_total = 0_u64;
    let mut transfer_total = 0_u64;
    let mut latency_total = 0_u64;
    for unit in &response.units {
        compute_total += unit.compute_time_us;
        transfer_total += unit.transfer_time_us;
        latency_total += unit.total_latency_us;
        println!(
            "{:<10} {:<14} {:<11} {:>12} {:>12} {:>12}",
            unit.work_id,
            if unit.node_id.is_empty() {
                "-"
            } else {
                unit.node_id.as_str()
            },
            unit.status,
            unit.compute_time_us,
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
    Ok(())
}

async fn run_benchmark(
    endpoint: &str,
    size: u32,
    repetitions: u32,
    local: bool,
    min_threads: u32,
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
            request_id: String::new(),
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
