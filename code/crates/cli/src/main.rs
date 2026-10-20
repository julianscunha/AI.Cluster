use std::time::{Duration, Instant};

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::{
    EnqueueGraphRequest, EnqueueWorkRequest, ListNodesRequest, WorkReportRequest,
    WorkReportResponse,
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
    Events,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Status | Command::Nodes => print_nodes(&cli.controller).await?,
        Command::Events => println!("events is reserved for the telemetry phase"),
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
        .enqueue_graph(EnqueueGraphRequest { size, seed: 42 })
        .await?
        .into_inner();
    println!(
        "graph enqueued={} blocked={} size={size}",
        response.enqueued, response.blocked
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
    println!("NODES {}", nodes.len());
    println!(
        "{:<20} {:<10} {:<10} {:>7} {:>9} {:>8}",
        "NODE", "STATE", "ARCH", "THREADS", "RAM_GB", "HEALTH"
    );
    for node in nodes {
        let identity = node.identity.unwrap_or_default();
        let capability = node.capability.unwrap_or_default();
        let health = node.health.unwrap_or_default();
        println!(
            "{:<20} {:<10} {:<10} {:>7} {:>9.2} {:>8.2}",
            identity.node_id,
            node.state,
            identity.architecture,
            capability.cpu_threads,
            capability.memory_bytes as f64 / 1_073_741_824.0,
            health.health_score,
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
) -> Result<(), Box<dyn std::error::Error>> {
    // Version and parameters travel with the output so two runs are comparable.
    println!(
        "benchmark_version={} scope={} size={size} repetitions={repetitions} seed=42",
        env!("CARGO_PKG_VERSION"),
        if local { "single-node" } else { "distributed" }
    );
    if local {
        return run_local_baseline(size, repetitions);
    }
    let mut client = connect_controller(endpoint.to_string()).await?;
    let enqueued = client
        .enqueue_work(EnqueueWorkRequest {
            rows: size,
            shared: size,
            cols: size,
            seed: 42,
            repetitions,
            min_cpu_threads: min_threads,
        })
        .await?
        .into_inner()
        .enqueued;
    println!("enqueued {enqueued} work units of {size}x{size}");

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
        "WORK", "NODE", "COMPUTE_US", "TRANSFER_US", "TOTAL_US"
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
        "completed={} pending={} failed={} avg_compute_us={} avg_transfer_us={}",
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
    println!("local baseline {size}x{size} repetitions={runs}");
    println!(
        "{:<12} {:>14} {:>12}",
        "SCOPE", "AVG_COMPUTE_US", "TOTAL_US"
    );
    println!(
        "{:<12} {:>14} {:>12}",
        "single-node",
        avg_us,
        total.as_micros()
    );
    Ok(())
}
