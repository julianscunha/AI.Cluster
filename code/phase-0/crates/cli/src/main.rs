use std::time::Duration;

use ai_cluster_protocol::cluster::cluster_control_client::ClusterControlClient;
use ai_cluster_protocol::cluster::{
    EnqueueWorkRequest, ListNodesRequest, WorkReportRequest, WorkReportResponse,
};
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
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Status | Command::Nodes => print_nodes(&cli.controller).await?,
        Command::Events => println!("events is reserved for the telemetry phase"),
        Command::Benchmark { size, repetitions } => {
            run_benchmark(&cli.controller, size, repetitions).await?
        }
    }
    Ok(())
}

async fn print_nodes(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = ClusterControlClient::connect(endpoint.to_string()).await?;
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
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = ClusterControlClient::connect(endpoint.to_string()).await?;
    let enqueued = client
        .enqueue_work(EnqueueWorkRequest {
            rows: size,
            shared: size,
            cols: size,
            seed: 42,
            repetitions,
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
        "completed={} pending={} avg_compute_us={} avg_transfer_us={}",
        report.completed.len(),
        report.pending,
        compute_total / count,
        transfer_total / count,
    );
}
