//! Lifecycle event log.
//!
//! The controller already prints these facts to stdout. Recording them makes the
//! same timeline readable by a client, which is what an operator has instead of a
//! container log.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use ai_cluster_protocol::cluster::Event;

/// Events held before the oldest is dropped.
///
/// ponytail: a bounded ring buffer, not a store. Enough to explain a recent
/// incident; ship events to a sink when history has to outlive the process.
const CAPACITY: usize = 256;

/// The file is rewritten down to the last `CAPACITY` events once it holds this many lines, so
/// it stays small without a rotation scheme.
const COMPACT_AT: usize = 4 * CAPACITY;

/// Where events are also written (`AI_CLUSTER_EVENTS`) and how many lines the file holds.
#[derive(Debug)]
struct Sink {
    path: PathBuf,
    lines: usize,
}

#[derive(Debug)]
pub struct EventLog {
    inner: Mutex<VecDeque<Event>>,
    sink: Mutex<Option<Sink>>,
    /// Barramento: cada evento gravado também sai para quem assinou (`subscribe`).
    bus: tokio::sync::broadcast::Sender<Event>,
}

/// `at<TAB>kind<TAB>detail`, one event per line. A tab or newline in a field would break the
/// line, so it becomes a space.
fn encode(event: &Event) -> String {
    let clean = |text: &str| text.replace(['\t', '\n', '\r'], " ");
    format!(
        "{}\t{}\t{}\n",
        event.at_unix_ms,
        clean(&event.kind),
        clean(&event.detail)
    )
}

fn decode(line: &str) -> Option<Event> {
    let mut fields = line.splitn(3, '\t');
    Some(Event {
        at_unix_ms: fields.next()?.parse().ok()?,
        kind: fields.next()?.to_string(),
        detail: fields.next()?.to_string(),
    })
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLog {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::with_capacity(CAPACITY)),
            sink: Mutex::new(None),
            bus: tokio::sync::broadcast::channel(CAPACITY).0,
        }
    }

    /// Os eventos que forem gravados a partir de agora. Quem não acompanha o ritmo perde os mais
    /// antigos (o receptor avisa com `Lagged`).
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.bus.subscribe()
    }

    /// Keeps the events in `path` too: loads the last `CAPACITY` from it now and appends every
    /// new one, so the timeline of an incident survives a restart of the controller. A file
    /// that cannot be read or written costs history, not the controller.
    pub fn persisted(self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let events: Vec<Event> = text.lines().filter_map(decode).collect();
        let lines = events.len();
        {
            let mut inner = self.lock();
            for event in events.into_iter().rev().take(CAPACITY).rev() {
                inner.push_back(event);
            }
        }
        *self
            .sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Sink { path, lines });
        self
    }

    fn append(&self, event: &Event, held: &VecDeque<Event>) {
        let mut guard = self
            .sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(sink) = guard.as_mut() else { return };
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sink.path)
            .and_then(|mut file| file.write_all(encode(event).as_bytes()));
        if written.is_err() {
            return;
        }
        sink.lines += 1;
        if sink.lines >= COMPACT_AT {
            let text: String = held.iter().map(encode).collect();
            let tmp = sink.path.with_extension("tmp");
            if std::fs::write(&tmp, text)
                .and_then(|_| std::fs::rename(&tmp, &sink.path))
                .is_ok()
            {
                sink.lines = held.len();
            }
        }
    }

    /// Appends one event, dropping the oldest once the buffer is full.
    pub fn record(&self, at_unix_ms: u64, kind: &str, detail: impl Into<String>) {
        let mut inner = self.lock();
        if inner.len() == CAPACITY {
            // The audit trail must outlive routine churn (a burst of joins and drops), so the
            // oldest ordinary event goes first; only a buffer of pure audit loses its oldest.
            let victim = inner.iter().position(|held| held.kind != "audit");
            match victim {
                Some(index) => inner.remove(index),
                None => inner.pop_front(),
            };
        }
        let event = Event {
            at_unix_ms,
            kind: kind.to_string(),
            detail: detail.into(),
        };
        inner.push_back(event.clone());
        self.append(&event, &inner);
        // sem assinantes o envio falha, e tudo bem
        let _ = self.bus.send(event);
    }

    /// The most recent `limit` events, oldest first. `limit` of 0 means every
    /// event still held.
    pub fn recent(&self, limit: usize) -> Vec<Event> {
        let inner = self.lock();
        let skip = if limit == 0 {
            0
        } else {
            inner.len().saturating_sub(limit)
        };
        inner.iter().skip(skip).cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<Event>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_of_ordinary_events_does_not_push_the_audit_trail_out() {
        let log = EventLog::new();
        log.record(0, "audit", "revoke node-x de 10.0.0.1");
        for index in 1..=(CAPACITY * 2) {
            log.record(index as u64, "node_online", format!("n{index}"));
        }

        let held = log.recent(0);
        assert_eq!(held.len(), CAPACITY);
        assert_eq!(held[0].kind, "audit");
    }

    #[test]
    fn drops_the_oldest_once_full_and_keeps_the_newest() {
        let log = EventLog::new();
        for index in 0..(CAPACITY + 5) {
            log.record(index as u64, "test", format!("e{index}"));
        }

        let held = log.recent(0);
        assert_eq!(held.len(), CAPACITY);
        assert_eq!(held[0].detail, "e5");
        assert_eq!(
            held.last().expect("non-empty").detail,
            format!("e{}", CAPACITY + 4)
        );
    }

    #[test]
    fn events_survive_a_restart_and_the_file_stays_bounded() {
        let path = std::env::temp_dir().join(format!("aic-events-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let log = EventLog::new().persisted(&path);
        log.record(1, "node_joined", "node-a\tcom tab\ne quebra");
        log.record(2, "node_left", "node-a");

        let restarted = EventLog::new().persisted(&path);
        let held = restarted.recent(0);
        assert_eq!(held.len(), 2);
        assert_eq!(held[0].kind, "node_joined");
        assert_eq!(held[0].detail, "node-a com tab e quebra");
        // New events continue the same timeline.
        restarted.record(3, "node_offline", "node-a");
        assert_eq!(EventLog::new().persisted(&path).recent(0).len(), 3);

        // Far past the compaction point the file is cut back, and the newest events remain.
        for index in 0..(COMPACT_AT + 10) {
            restarted.record(10 + index as u64, "bulk", format!("e{index}"));
        }
        let lines = std::fs::read_to_string(&path).unwrap().lines().count();
        assert!(lines < COMPACT_AT, "file kept {lines} lines");
        let again = EventLog::new().persisted(&path).recent(0);
        assert_eq!(again.last().unwrap().detail, format!("e{}", COMPACT_AT + 9));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recent_returns_the_last_n_oldest_first() {
        let log = EventLog::new();
        for index in 0..10 {
            log.record(index as u64, "test", format!("e{index}"));
        }

        let last_three: Vec<String> = log
            .recent(3)
            .iter()
            .map(|event| event.detail.clone())
            .collect();
        assert_eq!(last_three, vec!["e7", "e8", "e9"]);

        // A limit above what is held returns everything rather than padding.
        assert_eq!(log.recent(99).len(), 10);
    }
}
