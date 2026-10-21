use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::{
    CapabilityReport, HealthReport, HealthStreamRequest, HeartbeatRequest, LeaveNodeRequest,
    NodeIdentity, RegisterNodeRequest, RequestWorkRequest, SubmitWorkResultRequest, WorkResult,
    WorkUnit,
};
use ai_cluster_protocol::metadata::connect_controller;
use tokio::time;

const DEFAULT_CONTROLLER: &str = "http://127.0.0.1:50051";
const HEARTBEAT_INTERVAL_SECS: u64 = 5;
/// How long a worker waits before asking for work again when it found none. Kept
/// short: a unit enqueued just after a poll should be picked up almost at once.
/// A worker that just finished a unit asks again immediately, without waiting.
const WORK_POLL_IDLE_MS: u64 = 200;

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
    // Fase G4: the local scheduler decides how many cores this unit gets. One unit runs
    // at a time here, so the unit gets the whole budget the node reports.
    let cores = core_budget(capability.cpu_threads);

    let mut client = connect_controller(endpoint.clone()).await?;
    let registered = client
        .register_node(RegisterNodeRequest {
            identity: Some(identity.clone()),
            capability: Some(capability.clone()),
        })
        .await?
        .into_inner();
    println!(
        "registrado node_id={node_id} aceito={} cluster_id={} controller={endpoint}",
        registered.accepted, registered.cluster_id
    );

    // Planned departure: stop taking work, then tell the controller so it can hand
    // this node's in-flight units to someone else before the process exits.
    let mut shutdown = tokio::spawn(async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    });

    // Liveness runs on its own interval, not inside the work loop. Sharing the loop
    // means a unit that takes longer than the heartbeat grace starves the heartbeat,
    // and the controller reaps a healthy worker and hands its unit to another node
    // while this one is still computing it.
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    let heartbeat_task = tokio::spawn({
        let mut client = client.clone();
        let node_id = node_id.clone();
        let identity = identity.clone();
        let capability = capability.clone();
        async move {
            let mut ticker = time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
            loop {
                tokio::select! {
                    _ = ticker.tick() => {}
                    _ = &mut stop_rx => break,
                }
                match client
                    .heartbeat(HeartbeatRequest {
                        node_id: node_id.clone(),
                        unix_ms: now_ms(),
                    })
                    .await
                {
                    Ok(beat) => {
                        if !beat.into_inner().acknowledged {
                            println!("controller não conhece {node_id}; re-registrando");
                            if let Err(err) = client
                                .register_node(RegisterNodeRequest {
                                    identity: Some(identity.clone()),
                                    capability: Some(capability.clone()),
                                })
                                .await
                            {
                                println!("heartbeat: re-registro falhou: {err}");
                            }
                        }
                    }
                    Err(err) => println!("heartbeat falhou: {err}"),
                }
                if let Err(err) = client
                    .report_health(HealthStreamRequest {
                        node_id: node_id.clone(),
                        health: Some(health()),
                    })
                    .await
                {
                    println!("relatório de saúde falhou: {err}");
                }
            }
        }
    });

    // The work poll runs on its own cadence, decoupled from the heartbeat interval.
    // Sharing the heartbeat's ticker capped throughput at one unit per worker per
    // interval, however fast the compute was: a worker that finished a unit sat idle
    // until the next heartbeat tick. A worker now asks again the moment it finishes,
    // and only waits a short backoff when it found nothing.
    loop {
        if shutdown.is_finished() {
            break;
        }
        let work = client
            .request_work(RequestWorkRequest {
                node_id: node_id.clone(),
            })
            .await?
            .into_inner();
        match work.work {
            Some(unit) => {
                let work_id = unit.work_id.clone();
                // Test hook: hold the unit before computing it so a fault-injection run
                // can kill this worker while the unit is in flight. Unset means no delay.
                let delay = work_delay();
                if !delay.is_zero() {
                    time::sleep(delay).await;
                }
                match execute_work(&unit, &node_id, cores) {
                    Some(result) => {
                        let accepted = client
                            .submit_work_result(SubmitWorkResultRequest {
                                result: Some(result),
                            })
                            .await?
                            .into_inner()
                            .accepted;
                        println!(
                            "trabalho {work_id} shape={}x{}x{} compute_done threads={cores} aceito={accepted}",
                            unit.rows, unit.shared, unit.cols
                        );
                    }
                    None => {
                        println!(
                            "trabalho {work_id} payload não corresponde ao shape declarado; descartado"
                        )
                    }
                }
            }
            None => {
                // Nothing ready: wait before asking again, but wake at once on shutdown.
                tokio::select! {
                    _ = time::sleep(Duration::from_millis(WORK_POLL_IDLE_MS)) => {}
                    _ = &mut shutdown => break,
                }
            }
        }
    }

    // Stop liveness before leaving: a beat that lands after LeaveNode would make the
    // controller re-register this node, undoing the departure.
    stop_tx.send(()).ok();
    let _ = heartbeat_task.await;

    let left = client
        .leave_node(LeaveNodeRequest {
            node_id: node_id.clone(),
        })
        .await?
        .into_inner();
    println!(
        "saiu do cluster node_id={node_id} aceito={} requeued={}",
        left.accepted, left.requeued
    );
    Ok(())
}

/// Rebuilds the matrices from the flat payload, multiplies on `cores` threads, and reports
/// the checksum.
///
/// Returns `None` when the payload lengths disagree with the declared shape.
fn execute_work(unit: &WorkUnit, node_id: &str, cores: usize) -> Option<WorkResult> {
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
    let product = a.multiply_with_threads(&b, cores)?;
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

/// How long to hold a claimed unit before computing it.
///
/// Fault-injection hook for the failure tests: without a hold, a unit is in flight
/// for microseconds and a worker cannot be reliably killed while holding one. Unset
/// or unparsable means compute immediately, which is the production behaviour.
fn work_delay() -> Duration {
    std::env::var("AI_CLUSTER_WORK_DELAY_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::ZERO)
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

/// Cores the local scheduler grants one unit on this node (Fase G4).
///
/// The worker runs one unit at a time, so that unit gets the whole budget. The count the
/// node reports is the ceiling — a node that reports 2 threads must not quietly run 8 —
/// and the host's real parallelism caps it too, so a node that reports more threads than
/// the machine has does not oversubscribe and run slower than serial. Never zero.
fn core_budget(reported_threads: u32) -> usize {
    let available = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    grant_cores(reported_threads, available)
}

/// The grant itself, split from the host lookup so it can be tested exactly.
fn grant_cores(reported_threads: u32, available: usize) -> usize {
    (reported_threads.max(1) as usize).min(available.max(1))
}

fn capability() -> CapabilityReport {
    // Report fewer threads than the host has so a placement test can stand a weak
    // node next to a strong one without a second machine. Unset means report reality.
    let cpu_threads = std::env::var("AI_CLUSTER_CPU_THREADS")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|threads| threads.get() as u32)
                .unwrap_or(0)
        });
    // Same idea for memory: a placement test can stand a small-memory node next to a
    // large one on a single host. Unset means report reality.
    let memory_bytes = std::env::var("AI_CLUSTER_MEMORY_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or_else(|| meminfo_kb("MemTotal:").map(|kb| kb * 1024).unwrap_or(0));
    CapabilityReport {
        cpu_threads,
        memory_bytes,
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
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
        };
        let result = execute_work(&unit, "node-a", 4).expect("shapes match");
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
            depends_on: Vec::new(),
            matrix_a_from: String::new(),
            matrix_b_from: String::new(),
            min_cpu_threads: 0,
        };
        assert!(execute_work(&unit, "node-a", 1).is_none());
    }

    #[test]
    fn grants_the_smaller_of_reported_and_available() {
        assert_eq!(grant_cores(8, 4), 4);
        assert_eq!(grant_cores(2, 8), 2);
        assert_eq!(grant_cores(4, 4), 4);
    }

    #[test]
    fn never_grants_zero_cores() {
        assert_eq!(grant_cores(0, 4), 1);
        assert_eq!(grant_cores(8, 0), 1);
    }
}
