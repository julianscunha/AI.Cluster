use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "ai-cluster")]
struct Cli {
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

fn main() {
    let cli = Cli::parse();
    println!("command={:?}", cli.command);
}
