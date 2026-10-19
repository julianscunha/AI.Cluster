use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::{
    CapabilityReport, HealthReport, HealthStreamRequest, HeartbeatRequest, NodeIdentity,
    RegisterNodeRequest, RequestWorkRequest, SubmitWorkResultRequest, WorkResult, WorkUnit,
};
use ai_cluster_protocol::metadata::connect_controller;
use tokio::time;

const DEFAULT_CONTROLLER: &str = "http://127.0.0.1:50051";
const HEARTBEAT_INTERVAL_SECS: u64 = 5;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("AI_CLUSTER_CONTROLLER").unwrap_or_else(|_| DEFAULT_CONTROLLER.to_string());
    let node_id = node_id();
    let identity = NodeIdentity {
        node_id: node_id.clone(),
        device_id: device_id(),
        architecture: std::env::consts::ARCH.to_string(),
        os_version: os_version(),
        runtime_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let capability = capability();

    let mut client = connect_controller(endpoint.clone()).await?;
    let registered = client
        .register_node(RegisterNodeRequest {
            identity: Some(identity.clone()),
            capability: Some(capability.clone()),
        })
        .await?
        .into_inner();
    println!(
        "registered node_id={node_id} accepted={} cluster_id={} controller={endpoint}",
        registered.accepted, registered.cluster_id
    );

    let mut ticker = time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
    loop {
        ticker.tick().await;
        let heartbeat = client
            .heartbeat(HeartbeatRequest {
                node_id: node_id.clone(),
                unix_ms: now_ms(),
            })
            .await?
            .into_inner();
        if !heartbeat.acknowledged {
            println!("controller does not know {node_id}; re-registering");
            client
                .register_node(RegisterNodeRequest {
                    identity: Some(identity.clone()),
                    capability: Some(capability.clone()),
                })
                .await?;
        }
        client
            .report_health(HealthStreamRequest {
                node_id: node_id.clone(),
                health: Some(health()),
            })
            .await?;

        let work = client
            .request_work(RequestWorkRequest {
                node_id: node_id.clone(),
            })
            .await?
            .into_inner();
        if let Some(unit) = work.work {
            let work_id = unit.work_id.clone();
            match execute_work(&unit, &node_id) {
                Some(result) => {
                    let accepted = client
                        .submit_work_result(SubmitWorkResultRequest {
                            result: Some(result),
                        })
                        .await?
                        .into_inner()
                        .accepted;
                    println!(
                        "work {work_id} shape={}x{}x{} compute_done accepted={accepted}",
                        unit.rows, unit.shared, unit.cols
                    );
                }
                None => println!("work {work_id} payload does not match declared shape; dropped"),
            }
        }
    }
}

/// Rebuilds the matrices from the flat payload, multiplies, and reports the checksum.
///
/// Returns `None` when the payload lengths disagree with the declared shape.
fn execute_work(unit: &WorkUnit, node_id: &str) -> Option<WorkResult> {
    let expected_a = (unit.rows as usize).checked_mul(unit.shared as usize)?;
    let expected_b = (unit.shared as usize).checked_mul(unit.cols as usize)?;
    if unit.matrix_a.len() != expected_a || unit.matrix_b.len() != expected_b {
        return None;
    }
    let a = Matrix {
        rows: unit.rows,
        cols: unit.shared,
        data: unit.matrix_a.clone(),
    };
    let b = Matrix {
        rows: unit.shared,
        cols: unit.cols,
        data: unit.matrix_b.clone(),
    };
    let started = Instant::now();
    let product = a.multiply(&b)?;
    let compute_time_us = started.elapsed().as_micros() as u64;
    // Compute the checksum before `data` is moved out of the product.
    let checksum = product.checksum();
    Some(WorkResult {
        work_id: unit.work_id.clone(),
        node_id: node_id.to_string(),
        rows: product.rows,
        cols: product.cols,
        matrix_c: product.data,
        checksum,
        compute_time_us,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn node_id() -> String {
    std::env::var("AI_CLUSTER_NODE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| read_trimmed("/etc/hostname"))
        .unwrap_or_else(|| "worker".to_string())
}

/// Device identity is separate from the logical node id and survives re-imaging.
fn device_id() -> String {
    read_trimmed("/etc/machine-id").unwrap_or_else(|| "unknown-device".to_string())
}

fn os_version() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with("PRETTY_NAME="))
                .map(|line| {
                    line.trim_start_matches("PRETTY_NAME=")
                        .trim_matches('"')
                        .to_string()
                })
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "linux".to_string())
}

fn read_trimmed(path: &str) -> Option<String> {
    let value = std::fs::read_to_string(path).ok()?.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn parse_meminfo(content: &str, key: &str) -> Option<u64> {
    content
        .lines()
        .find(|line| line.starts_with(key))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u64>().ok())
}

fn meminfo_kb(key: &str) -> Option<u64> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?, key)
}

fn capability() -> CapabilityReport {
    CapabilityReport {
        cpu_threads: std::thread::available_parallelism()
            .map(|threads| threads.get() as u32)
            .unwrap_or(0),
        memory_bytes: meminfo_kb("MemTotal:").map(|kb| kb * 1024).unwrap_or(0),
        precisions: vec!["fp32".to_string()],
        ..Default::default()
    }
}

// ponytail: health covers memory pressure only; CPU/thermal signals arrive with the benchmark phase.
fn health() -> HealthReport {
    let utilization = match (meminfo_kb("MemTotal:"), meminfo_kb("MemAvailable:")) {
        (Some(total), Some(available)) if total > 0 => {
            (1.0 - available as f64 / total as f64).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    HealthReport {
        health_score: 1.0 - utilization,
        memory_utilization: utilization,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meminfo_values() {
        let sample = "MemTotal:       16384000 kB\nMemAvailable:    8192000 kB\n";
        assert_eq!(parse_meminfo(sample, "MemTotal:"), Some(16_384_000));
        assert_eq!(parse_meminfo(sample, "MemAvailable:"), Some(8_192_000));
        assert_eq!(parse_meminfo(sample, "Absent:"), None);
    }

    #[test]
    fn executes_matrix_work_and_reports_checksum() {
        let unit = WorkUnit {
            work_id: "w1".to_string(),
            rows: 2,
            shared: 2,
            cols: 2,
            matrix_a: vec![1.0, 2.0, 3.0, 4.0],
            matrix_b: vec![5.0, 6.0, 7.0, 8.0],
        };
        let result = execute_work(&unit, "node-a").expect("shapes match");
        assert_eq!(result.work_id, "w1");
        assert_eq!(result.node_id, "node-a");
        assert_eq!(result.rows, 2);
        assert_eq!(result.cols, 2);
        assert_eq!(result.matrix_c, vec![19.0, 22.0, 43.0, 50.0]);
        assert_eq!(result.checksum, 134.0);
    }

    #[test]
    fn rejects_payload_that_disagrees_with_declared_shape() {
        let unit = WorkUnit {
            work_id: "w1".to_string(),
            rows: 2,
            shared: 2,
            cols: 2,
            matrix_a: vec![1.0, 2.0, 3.0],
            matrix_b: vec![5.0, 6.0, 7.0, 8.0],
        };
        assert!(execute_work(&unit, "node-a").is_none());
    }
}
