//! The same Work Unit over each transport (F09 gate): serialize once, round-trip it through an
//! echo peer, report the median time and the throughput. Run from `code/`:
//!
//!     cargo run --release -p ai-cluster-transport --example bench [edge]
//!
//! `edge` is the side of the square matrices in the unit (default 256, about 1 MiB of payload).

use std::time::{Duration, Instant};

use ai_cluster_protocol::cluster::WorkUnit;
use ai_cluster_transport::{Transport, echo, tcp_accept, tcp_connect};
use prost::Message;

const ROUNDS: usize = 40;

fn work_unit(edge: u32) -> WorkUnit {
    let cells = (edge * edge) as usize;
    WorkUnit {
        work_id: "w-bench".into(),
        rows: edge,
        shared: edge,
        cols: edge,
        matrix_a: vec![1.5; cells],
        matrix_b: vec![0.5; cells],
        ..Default::default()
    }
}

async fn measure<T: Transport>(link: &mut T, payload: &[u8]) -> (Duration, Duration) {
    let mut times = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let started = Instant::now();
        link.send(payload).await.expect("send");
        let back = link.receive().await.expect("receive").expect("open");
        assert_eq!(back.len(), payload.len());
        times.push(started.elapsed());
    }
    times.sort();
    let rtt = link.probe().await.expect("probe");
    (times[times.len() / 2], rtt)
}

fn report(name: &str, payload: usize, median: Duration, rtt: Duration) {
    // A round trip moves the payload twice.
    let mb_per_s = 2.0 * payload as f64 / 1e6 / median.as_secs_f64();
    println!(
        "{name:<6} unidade={:.2} MiB  ida e volta p50={:>8.0} us  vazao={:>7.0} MB/s  ping={:>6.0} us",
        payload as f64 / 1_048_576.0,
        median.as_secs_f64() * 1e6,
        mb_per_s,
        rtt.as_secs_f64() * 1e6
    );
}

#[tokio::main]
async fn main() {
    let edge: u32 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(256);
    let payload = work_unit(edge).encode_to_vec();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (client, server) = tokio::join!(tcp_connect(addr), tcp_accept(&listener));
    let (mut client, mut server) = (client.unwrap(), server.unwrap());
    tokio::spawn(async move { echo(&mut server).await });
    let (median, rtt) = measure(&mut client, &payload).await;
    report("tcp", payload.len(), median, rtt);

    #[cfg(unix)]
    {
        use ai_cluster_transport::{unix_accept, unix_connect};
        let path = std::env::temp_dir().join(format!("aic-bench-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let (client, server) = tokio::join!(unix_connect(&path), unix_accept(&listener));
        let (mut client, mut server) = (client.unwrap(), server.unwrap());
        tokio::spawn(async move { echo(&mut server).await });
        let (median, rtt) = measure(&mut client, &payload).await;
        report("unix", payload.len(), median, rtt);
        let _ = std::fs::remove_file(&path);
    }
}
