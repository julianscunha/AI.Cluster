//! Resident channels to workers (Fase J5): the controller relays `matmul(slice, activation)` frames
//! over a long-lived stream each worker opens, instead of making the worker poll for them.
//!
//! The hub holds one session per node and the operations waiting for an answer. It trusts nothing
//! it is told by a stream: an answer counts only for an operation that was sent to that same node,
//! a replaced or closed session fails what was waiting on it, and every queue is bounded.

use ai_cluster_protocol::cluster::{ShardCompute, ShardResult};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, oneshot};
use tonic::Status;

/// Frames queued toward one worker. Past it `submit` fails at once instead of waiting.
const CHANNEL_DEPTH: usize = 64;
/// Operações em voo para um worker (`AI_CLUSTER_SHARD_MAX_PENDING`, 1 a 1024, padrão 64): acima
/// disso `ShardMatmul` responde 503 (`ResourceExhausted`).
fn max_pending_per_node() -> usize {
    static VALUE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("AI_CLUSTER_SHARD_MAX_PENDING")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(64)
            .clamp(1, 1024)
    })
}

#[derive(Debug)]
struct Session {
    id: u64,
    frames: mpsc::Sender<Result<ShardCompute, Status>>,
}

#[derive(Debug)]
struct Pending {
    node: String,
    reply: oneshot::Sender<ShardResult>,
}

#[derive(Debug, Default)]
struct State {
    sessions: HashMap<String, Session>,
    pending: HashMap<u64, Pending>,
}

#[derive(Debug, Default)]
pub struct ShardHub {
    next: AtomicU64,
    state: Mutex<State>,
}

pub type Frames = mpsc::Receiver<Result<ShardCompute, Status>>;

impl ShardHub {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Opens the node's session, replacing (and failing the operations of) any earlier one.
    pub fn attach(&self, node: &str) -> (u64, Frames) {
        let (frames, receiver) = mpsc::channel(CHANNEL_DEPTH);
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let mut state = self.lock();
        state.pending.retain(|_, p| p.node != node);
        state
            .sessions
            .insert(node.to_string(), Session { id, frames });
        (id, receiver)
    }

    /// Ends the session `id` of `node` if it is still the current one; a stream that was already
    /// replaced must not tear down its successor.
    pub fn detach(&self, node: &str, id: u64) {
        let mut state = self.lock();
        if state.sessions.get(node).is_some_and(|s| s.id == id) {
            state.sessions.remove(node);
            state.pending.retain(|_, p| p.node != node);
        }
    }

    /// Drops the node's session and everything waiting on it (revoked or leaving).
    pub fn drop_node(&self, node: &str) {
        let mut state = self.lock();
        state.sessions.remove(node);
        state.pending.retain(|_, p| p.node != node);
    }

    /// Sends `frame` to `node`, returning the operation id and the receiver for its answer.
    pub fn submit(
        &self,
        node: &str,
        mut frame: ShardCompute,
    ) -> Result<(u64, oneshot::Receiver<ShardResult>), Status> {
        let mut state = self.lock();
        let Some(session) = state.sessions.get(node) else {
            return Err(Status::failed_precondition(format!(
                "worker {node} sem canal de fatias aberto"
            )));
        };
        if state.pending.values().filter(|p| p.node == node).count() >= max_pending_per_node() {
            return Err(Status::resource_exhausted(
                "operações demais em voo neste worker",
            ));
        }
        let op = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        frame.op_id = op;
        match session.frames.try_send(Ok(frame)) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return Err(Status::resource_exhausted("fila do canal cheia"));
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(Status::unavailable("canal do worker fechado"));
            }
        }
        let (reply, answer) = oneshot::channel();
        state.pending.insert(
            op,
            Pending {
                node: node.to_string(),
                reply,
            },
        );
        Ok((op, answer))
    }

    /// Delivers an answer, only if `op` was sent to `node`.
    pub fn complete(&self, node: &str, result: ShardResult) {
        let mut state = self.lock();
        if state
            .pending
            .get(&result.op_id)
            .is_some_and(|p| p.node == node)
            && let Some(pending) = state.pending.remove(&result.op_id)
        {
            let _ = pending.reply.send(result);
        }
    }

    /// Forgets an operation nobody is waiting for any more (timeout or failed sibling).
    /// O worker é avisado para pular a operação que ainda espera na fila dele.
    pub fn cancel(&self, op: u64) {
        let mut state = self.lock();
        if let Some(pending) = state.pending.remove(&op)
            && let Some(session) = state.sessions.get(&pending.node)
        {
            // Fila cheia ou canal fechado: o aviso é só uma economia, nada depende dele.
            let _ = session.frames.try_send(Ok(ShardCompute {
                op_id: op,
                cancel: true,
                ..Default::default()
            }));
        }
    }

    #[cfg(test)]
    fn pending(&self) -> usize {
        self.lock().pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> ShardCompute {
        ShardCompute::default()
    }

    fn answer(op: u64) -> ShardResult {
        ShardResult {
            op_id: op,
            ok: true,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn an_answer_reaches_only_the_operation_of_that_node() {
        let hub = ShardHub::default();
        let (_session, mut frames) = hub.attach("a");
        let _other = hub.attach("b");
        let (op, answered) = hub.submit("a", frame()).unwrap();
        assert_eq!(frames.recv().await.unwrap().unwrap().op_id, op);

        // Node b cannot answer node a's operation.
        hub.complete("b", answer(op));
        assert_eq!(hub.pending(), 1);
        hub.complete("a", answer(op));
        assert!(answered.await.unwrap().ok);
        assert_eq!(hub.pending(), 0);
    }

    #[tokio::test]
    async fn a_replaced_or_closed_session_fails_what_waited_on_it() {
        let hub = ShardHub::default();
        let (first, _frames) = hub.attach("a");
        let (_, waiting) = hub.submit("a", frame()).unwrap();
        let (_second, _new_frames) = hub.attach("a");
        assert!(
            waiting.await.is_err(),
            "a sessão nova invalida as operações da antiga"
        );

        // The old stream ending must not tear down its successor.
        hub.detach("a", first);
        assert!(hub.submit("a", frame()).is_ok());
        hub.drop_node("a");
        assert!(hub.submit("a", frame()).is_err());
    }

    #[test]
    fn a_node_without_a_channel_or_with_a_full_queue_is_refused() {
        let hub = ShardHub::default();
        assert_eq!(
            hub.submit("ghost", frame()).unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
        let (_session, _frames) = hub.attach("a");
        for _ in 0..CHANNEL_DEPTH {
            assert!(hub.submit("a", frame()).is_ok());
        }
        assert_eq!(
            hub.submit("a", frame()).unwrap_err().code(),
            tonic::Code::ResourceExhausted
        );
    }

    #[test]
    fn cancel_forgets_the_operation() {
        let hub = ShardHub::default();
        let (_session, _frames) = hub.attach("a");
        let (op, _answer) = hub.submit("a", frame()).unwrap();
        hub.cancel(op);
        assert_eq!(hub.pending(), 0);
    }

    #[tokio::test]
    async fn cancel_tells_the_worker_to_skip_the_operation() {
        let hub = ShardHub::default();
        let (_session, mut frames) = hub.attach("a");
        let (op, _answer) = hub.submit("a", frame()).unwrap();
        assert!(!frames.recv().await.unwrap().unwrap().cancel);
        hub.cancel(op);
        let told = frames.recv().await.unwrap().unwrap();
        assert!(told.cancel && told.op_id == op);
    }
}
