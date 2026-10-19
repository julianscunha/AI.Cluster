use ai_cluster_protocol::cluster::cluster_control_client::ClusterControlClient;
use ai_cluster_protocol::cluster::ListNodesRequest;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "ai-cluster")]
struct Cli {
    /// Controller gRPC endpoint.
    #[arg(long, global = true, env = "AI_CLUSTER_CONTROLLER", default_value = "http://127.0.0.1:50051")]
    controller: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Status,
    Nodes,
    Events,
    Benchmark,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Status | Command::Nodes => print_nodes(&cli.controller).await?,
        Command::Events => println!("events is reserved for the telemetry phase"),
        Command::Benchmark => println!("benchmark is reserved for the benchmark phase"),
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
