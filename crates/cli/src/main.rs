use clap::{Parser, Subcommand};
use real_ai_cluster_protocol::v1::{
    cluster_control_client::ClusterControlClient, ListNodesRequest,
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
                println!(
                    "{}	{}	{}	{}",
                    node.node_id, node.state, node.architecture, node.hostname
                );
            }
        }
    }

    Ok(())
}
