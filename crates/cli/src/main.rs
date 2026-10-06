use clap::{Parser, Subcommand};
use real_ai_cluster_protocol::v1::{
    cluster_control_client::ClusterControlClient, ListNodesRequest, SubmitWorkRequest, WorkUnit,
};
use tonic::Request;

#[derive(Parser, Debug)]
#[command(name = "ai-cluster", version, about = "CLI do Real.AI.Cluster")]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:50051")]
    controller: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Status,
    Nodes,
    Work {
        #[command(subcommand)]
        command: WorkCommand,
    },
}

#[derive(Subcommand, Debug)]
enum WorkCommand {
    Run {
        #[arg(long, default_value = "cpu_spin")]
        operation: String,

        #[arg(long, default_value_t = 1_000_000)]
        iterations: u64,

        #[arg(long, default_value_t = 0)]
        priority: i32,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Status => println!("Real.AI.Cluster: consulta de status"),
        Command::Nodes => {
            let mut client = ClusterControlClient::connect(cli.controller).await?;
            let response = client
                .list_nodes(Request::new(ListNodesRequest {}))
                .await?
                .into_inner();

            println!("NODES: {}", response.nodes.len());

            for node in response.nodes {
                let capability = node.capability.unwrap_or_default();
                println!(
                    "{}	state={}	arch={}	cores={}	ram={} MB	cpu={:.0} ops/s	storage={}",
                    node.node_id,
                    node.state,
                    node.architecture,
                    capability.cpu_logical_cores,
                    capability.memory_total_bytes / 1024 / 1024,
                    capability.cpu_ops_per_sec,
                    capability.storage.len(),
                );
            }
        }
        Command::Work { command } => match command {
            WorkCommand::Run {
                operation,
                iterations,
                priority,
            } => {
                let mut client = ClusterControlClient::connect(cli.controller).await?;

                let payload = if operation == "cpu_spin" {
                    iterations.to_le_bytes().to_vec()
                } else {
                    Vec::new()
                };

                let work = WorkUnit {
                    work_id: format!("wu-{}", unix_ms()),
                    operation,
                    payload,
                    priority,
                };

                let result = client
                    .submit_work(Request::new(SubmitWorkRequest { work: Some(work) }))
                    .await?
                    .into_inner();

                println!(
                    "accepted={} node={} operation={} duration={}us output_bytes={}",
                    result.accepted,
                    result.assigned_node_id,
                    result.operation,
                    result.duration_us,
                    result.output.len()
                );
            }
        },
    }

    Ok(())
}

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
