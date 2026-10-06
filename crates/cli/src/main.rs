use std::collections::BTreeSet;

use clap::{Parser, Subcommand};
use real_ai_cluster_protocol::v1::{
    cluster_control_client::ClusterControlClient, DistributedMatMulRequest, ListNodesRequest,
    SubmitWorkRequest, WorkUnit,
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
    Benchmark {
        #[command(subcommand)]
        command: BenchmarkCommand,
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

#[derive(Subcommand, Debug)]
enum BenchmarkCommand {
    DistributedMatmul {
        #[arg(long, default_value_t = 128)]
        rows: usize,

        #[arg(long, default_value_t = 128)]
        inner: usize,

        #[arg(long, default_value_t = 128)]
        cols: usize,

        #[arg(
            long,
            default_value_t = 0,
            help = "máximo de Workers; 0 = todos os elegíveis"
        )]
        max_workers: u32,
    },

    Scaling {
        #[arg(long, default_value_t = 128)]
        rows: usize,

        #[arg(long, default_value_t = 128)]
        inner: usize,

        #[arg(long, default_value_t = 128)]
        cols: usize,

        #[arg(
            long,
            default_value = "1,2,3",
            help = "quantidades de Workers a comparar, separadas por vírgula"
        )]
        worker_counts: String,
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
                    "{}	state={}	active={}	arch={}	cores={}	ram={} MB	cpu={:.0} ops/s	rtt={:.0}us	storage={}	worker={}",
                    node.node_id,
                    node.state,
                    node.active_work_units,
                    node.architecture,
                    capability.cpu_logical_cores,
                    capability.memory_total_bytes / 1024 / 1024,
                    capability.cpu_ops_per_sec,
                    capability.network_rtt_us,
                    capability.storage.len(),
                    node.worker_endpoint,
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
        Command::Benchmark { command } => match command {
            BenchmarkCommand::DistributedMatmul {
                rows,
                inner,
                cols,
                max_workers,
            } => {
                validate_dimensions(rows, inner, cols)?;

                let result =
                    run_distributed_matmul(&cli.controller, rows, inner, cols, max_workers).await?;

                print_matmul_result(&result);
            }

            BenchmarkCommand::Scaling {
                rows,
                inner,
                cols,
                worker_counts,
            } => {
                validate_dimensions(rows, inner, cols)?;

                let worker_counts = parse_worker_counts(&worker_counts)?;
                println!(
                    "MATMUL SCALING {}x{}x{} | workers={:?}",
                    rows, inner, cols, worker_counts
                );
                println!(
                    "workers_used	duration_us	compute_us	overhead_us	speedup	efficiency	input_bytes_sent	checksum"
                );

                let mut baseline_duration_us = None;
                let mut baseline_checksum = None;

                for max_workers in worker_counts {
                    let result =
                        run_distributed_matmul(&cli.controller, rows, inner, cols, max_workers).await?;

                    let checksum = checksum_f32_bytes(&result.matrix_c);

                    if max_workers == 1
                        || (baseline_duration_us.is_none() && result.workers_used == 1)
                    {
                        baseline_duration_us = Some(result.duration_us);
                        baseline_checksum = Some(checksum);
                    }

                    if let Some(expected_checksum) = baseline_checksum {
                        if (checksum - expected_checksum).abs() > 0.001 {
                            return Err(format!(
                                "checksum divergente: baseline={expected_checksum:.6} atual={checksum:.6}"
                            )
                            .into());
                        }
                    }

                    let speedup = baseline_duration_us
                        .filter(|duration| *duration > 0 && result.duration_us > 0)
                        .map(|duration| duration as f64 / result.duration_us as f64)
                        .unwrap_or(0.0);

                    let efficiency = if result.workers_used > 0 {
                        speedup / result.workers_used as f64
                    } else {
                        0.0
                    };

                    let overhead_us = result
                        .duration_us
                        .saturating_sub(result.compute_duration_us);

                    println!(
                        "{}	{}	{}	{}	{:.3}	{:.3}	{}	{checksum:.6}",
                        result.workers_used,
                        result.duration_us,
                        result.compute_duration_us,
                        overhead_us,
                        speedup,
                        efficiency,
                        result.input_bytes_sent,
                    );
                }
            }
        },
    }

    Ok(())
}

fn validate_dimensions(
    rows: usize,
    inner: usize,
    cols: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if rows == 0 || inner == 0 || cols == 0 {
        return Err("dimensões devem ser maiores que zero".into());
    }

    Ok(())
}

fn parse_worker_counts(value: &str) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let mut counts = BTreeSet::new();

    for item in value.split(',') {
        let count: u32 = item.trim().parse()?;

        if count == 0 {
            return Err("a quantidade de Workers deve ser maior que zero".into());
        }

        counts.insert(count);
    }

    if !counts.contains(&1) {
        counts.insert(1);
    }

    Ok(counts.into_iter().collect())
}

async fn run_distributed_matmul(
    controller: &str,
    rows: usize,
    inner: usize,
    cols: usize,
    max_workers: u32,
) -> Result<real_ai_cluster_protocol::v1::DistributedMatMulResponse, Box<dyn std::error::Error>> {
    let mut client = ClusterControlClient::connect(controller).await?;

    let matrix_a: Vec<f32> = (0..rows * inner)
        .map(|index| ((index % 7) as f32) - 3.0)
        .collect();

    let matrix_b: Vec<f32> = (0..inner * cols)
        .map(|index| ((index % 5) as f32) - 2.0)
        .collect();

    let result = client
        .distributed_mat_mul(Request::new(DistributedMatMulRequest {
            work_id: format!("matmul-{}", unix_ms()),
            rows: rows as u32,
            inner: inner as u32,
            cols: cols as u32,
            matrix_a: f32_to_bytes(&matrix_a),
            matrix_b: f32_to_bytes(&matrix_b),
            max_workers,
        }))
        .await?
        .into_inner();

    Ok(result)
}

fn print_matmul_result(result: &real_ai_cluster_protocol::v1::DistributedMatMulResponse) {
    let overhead_us = result
        .duration_us
        .saturating_sub(result.compute_duration_us);

    println!(
        "accepted={} workers={} duration={}us compute={}us overhead~={}us input_bytes_sent={} output_bytes={} checksum={:.6}",
        result.accepted,
        result.workers_used,
        result.duration_us,
        result.compute_duration_us,
        overhead_us,
        result.input_bytes_sent,
        result.matrix_c.len(),
        checksum_f32_bytes(&result.matrix_c),
    );
}

fn f32_to_bytes(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn checksum_f32_bytes(bytes: &[u8]) -> f64 {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().expect("chunk com 4 bytes")) as f64)
        .sum()
}

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
