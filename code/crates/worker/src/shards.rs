//! The worker's side of the resident channel (Fase J5): it keeps one stream open to the
//! controller and answers `matmul(slice, activation)` frames from weight slices held in its own
//! memory, loaded lazily from its own disk. Weights never cross the network.

use ai_cluster_common::matrix::Matrix;
use ai_cluster_engine::ShardStore;
use ai_cluster_protocol::cluster::{ShardCompute, ShardResult};
use ai_cluster_protocol::metadata::{MetadataClient, NODE_ID_HEADER};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tonic::codegen::tokio_stream::wrappers::ReceiverStream;

const RECONNECT: Duration = Duration::from_secs(2);
/// Answers waiting to go back up the stream.
const OUTBOX: usize = 64;
/// Computations running at once. Each already uses every core, so more only fight over them.
const CONCURRENT_COMPUTES: usize = 2;
/// Largest activation a frame may carry, in f64 values; the controller enforces the same cap.
const MAX_X: usize = 2 * 1024 * 1024;

/// Keeps the channel open until the task is aborted. A refusal (not approved yet, controller
/// down) is retried quietly: the message is printed once per distinct reason.
pub async fn run(mut client: MetadataClient, node_id: String, store: Arc<ShardStore>) {
    let mut last_error = String::new();
    let permits = Arc::new(tokio::sync::Semaphore::new(CONCURRENT_COMPUTES));
    loop {
        let (outbox, answers) = mpsc::channel::<ShardResult>(OUTBOX);
        let mut request = tonic::Request::new(ReceiverStream::new(answers));
        match node_id.parse() {
            Ok(value) => {
                request.metadata_mut().insert(NODE_ID_HEADER, value);
            }
            Err(_) => {
                println!("canal de fatias: node_id não cabe num cabeçalho; canal desligado");
                return;
            }
        }
        match client.shard_channel(request).await {
            Ok(response) => {
                println!("canal de fatias aberto");
                last_error.clear();
                let mut frames = response.into_inner();
                while let Ok(Some(frame)) = frames.message().await {
                    let (store, outbox, permits) = (store.clone(), outbox.clone(), permits.clone());
                    tokio::spawn(async move {
                        let _permit = permits.acquire_owned().await;
                        let answer = tokio::task::spawn_blocking(move || answer(&store, frame))
                            .await
                            .unwrap_or_else(|_| ShardResult {
                                ok: false,
                                error: "falha interna no cálculo".into(),
                                ..Default::default()
                            });
                        let _ = outbox.send(answer).await;
                    });
                }
                println!("canal de fatias fechado; reconectando");
            }
            Err(status) => {
                let reason = format!("{}: {}", status.code(), status.message());
                if reason != last_error {
                    println!("canal de fatias recusado ({reason}); tentando de novo");
                    last_error = reason;
                }
            }
        }
        tokio::time::sleep(RECONNECT).await;
    }
}

/// Computes one frame. An error is reported to the caller, never a panic or a silent drop.
fn answer(store: &ShardStore, frame: ShardCompute) -> ShardResult {
    let op_id = frame.op_id;
    let failed = |error: String| ShardResult {
        op_id,
        ok: false,
        error,
        ..Default::default()
    };
    if frame.x.len() > MAX_X
        || (frame.rows as usize).checked_mul(frame.k as usize) != Some(frame.x.len())
    {
        return failed("entrada com tamanho incoerente".into());
    }
    let started = Instant::now();
    let x = Matrix {
        rows: frame.rows,
        cols: frame.k,
        data: frame.x,
    };
    match store.compute(
        &frame.model_subdir,
        &frame.tensor,
        (frame.row_start, frame.row_end),
        frame.by_columns,
        &x,
    ) {
        Ok(y) => ShardResult {
            op_id,
            ok: true,
            error: String::new(),
            rows: y.rows,
            cols: y.cols,
            data: y.data,
            compute_us: started.elapsed().as_micros() as u64,
        },
        Err(error) => failed(error),
    }
}
