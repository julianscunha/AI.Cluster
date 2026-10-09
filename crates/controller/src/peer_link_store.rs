use std::{
    collections::{HashMap, VecDeque},
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::warn;

const HISTORY_VERSION: u32 = 1;
const MAX_OBSERVATIONS_PER_LINK: usize = 32;
const AGGREGATION_WINDOW: Duration = Duration::from_secs(10 * 60);
const DEFAULT_HISTORY_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone)]
pub struct PeerLinkMetrics {
    pub rtt_us: f64,
    pub round_trip_mb_per_sec: f64,
    pub sample_count: u32,
    pub measured_at: Instant,
    pub observation_count: u32,
    pub failure_count: u32,
    pub timeout_count: u32,
    pub failure_rate: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOutcome {
    Failure,
    Timeout,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LinkObservation {
    observed_at_unix_ms: i64,
    outcome: ObservationOutcome,
    rtt_us: Option<f64>,
    round_trip_mb_per_sec: Option<f64>,
    sample_count: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ObservationOutcome {
    Success,
    Failure,
    Timeout,
    Invalid,
}

#[derive(Debug, Default)]
struct StoreState {
    links: HashMap<(String, String), VecDeque<LinkObservation>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedState {
    version: u32,
    links: Vec<PersistedLink>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedLink {
    source_node_id: String,
    destination_node_id: String,
    observations: VecDeque<LinkObservation>,
}

pub struct PeerLinkStore {
    path: Option<PathBuf>,
    state: Mutex<StoreState>,
}

impl PeerLinkStore {
    pub fn from_env() -> Self {
        let path = env::var_os("RAC_PEER_LINK_HISTORY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/peer-link-history.json"));
        Self::open(path)
    }

    pub fn open(path: PathBuf) -> Self {
        let state = match load_state(&path) {
            Ok(state) => state,
            Err(error) => {
                warn!(
                    path = %path.display(),
                    %error,
                    "não foi possível carregar o histórico de enlaces; iniciando sem histórico persistido"
                );
                StoreState::default()
            }
        };
        Self {
            path: Some(path),
            state: Mutex::new(state),
        }
    }

    pub fn in_memory() -> Self {
        Self {
            path: None,
            state: Mutex::new(StoreState::default()),
        }
    }

    pub async fn fresh_metrics(
        &self,
        source_node_id: &str,
        destination_node_id: &str,
        ttl: Duration,
    ) -> Option<PeerLinkMetrics> {
        let state = self.state.lock().await;
        let history = state
            .links
            .get(&(source_node_id.to_owned(), destination_node_id.to_owned()))?;
        let last = history.back()?;

        if last.outcome != ObservationOutcome::Success
            || observation_age_ms(last) > ttl.as_millis() as i64
        {
            return None;
        }

        metrics_from_history(history, AGGREGATION_WINDOW)
    }

    pub async fn historical_metrics(
        &self,
        source_node_id: &str,
        destination_node_id: &str,
        max_age: Duration,
    ) -> Option<PeerLinkMetrics> {
        let state = self.state.lock().await;
        let history = state
            .links
            .get(&(source_node_id.to_owned(), destination_node_id.to_owned()))?;
        metrics_from_history(history, max_age)
    }

    pub async fn record_success(
        &self,
        source_node_id: &str,
        destination_node_id: &str,
        rtt_us: f64,
        round_trip_mb_per_sec: f64,
        sample_count: u32,
    ) -> Option<PeerLinkMetrics> {
        self.record(
            source_node_id,
            destination_node_id,
            LinkObservation {
                observed_at_unix_ms: now_ms(),
                outcome: ObservationOutcome::Success,
                rtt_us: Some(rtt_us),
                round_trip_mb_per_sec: Some(round_trip_mb_per_sec),
                sample_count,
            },
        )
        .await;

        self.historical_metrics(source_node_id, destination_node_id, AGGREGATION_WINDOW)
            .await
    }

    pub async fn record_failure(
        &self,
        source_node_id: &str,
        destination_node_id: &str,
        outcome: ProbeOutcome,
    ) -> Option<PeerLinkMetrics> {
        let outcome = match outcome {
            ProbeOutcome::Failure => ObservationOutcome::Failure,
            ProbeOutcome::Timeout => ObservationOutcome::Timeout,
            ProbeOutcome::Invalid => ObservationOutcome::Invalid,
        };

        self.record(
            source_node_id,
            destination_node_id,
            LinkObservation {
                observed_at_unix_ms: now_ms(),
                outcome,
                rtt_us: None,
                round_trip_mb_per_sec: None,
                sample_count: 0,
            },
        )
        .await;

        self.historical_metrics(source_node_id, destination_node_id, DEFAULT_HISTORY_WINDOW)
            .await
    }

    async fn record(
        &self,
        source_node_id: &str,
        destination_node_id: &str,
        observation: LinkObservation,
    ) {
        let mut state = self.state.lock().await;
        let history = state
            .links
            .entry((source_node_id.to_owned(), destination_node_id.to_owned()))
            .or_default();

        history.push_back(observation);
        while history.len() > MAX_OBSERVATIONS_PER_LINK {
            history.pop_front();
        }

        if let Some(path) = &self.path {
            if let Err(error) = persist_state(path, &state) {
                warn!(
                    path = %path.display(),
                    source_node_id,
                    destination_node_id,
                    %error,
                    "falha ao persistir telemetria do enlace; mantendo observação em memória"
                );
            }
        }
    }
}

fn metrics_from_history(
    history: &VecDeque<LinkObservation>,
    max_age: Duration,
) -> Option<PeerLinkMetrics> {
    let now = now_ms();
    let max_age_ms = max_age.as_millis().min(i64::MAX as u128) as i64;
    let recent: Vec<&LinkObservation> = history
        .iter()
        .filter(|observation| now.saturating_sub(observation.observed_at_unix_ms) <= max_age_ms)
        .collect();

    let successful: Vec<&LinkObservation> = recent
        .iter()
        .copied()
        .filter(|observation| observation.outcome == ObservationOutcome::Success)
        .filter(|observation| {
            observation.rtt_us.is_some_and(f64::is_finite)
                && observation.rtt_us.is_some_and(|value| value >= 0.0)
                && observation
                    .round_trip_mb_per_sec
                    .is_some_and(f64::is_finite)
                && observation
                    .round_trip_mb_per_sec
                    .is_some_and(|value| value > 0.0)
        })
        .rev()
        .take(8)
        .collect();

    let latest_success = successful.first()?;
    let rtt_us = successful
        .iter()
        .filter_map(|observation| observation.rtt_us)
        .sum::<f64>()
        / successful.len() as f64;
    let round_trip_mb_per_sec = successful
        .iter()
        .filter_map(|observation| observation.round_trip_mb_per_sec)
        .sum::<f64>()
        / successful.len() as f64;

    if !rtt_us.is_finite()
        || rtt_us < 0.0
        || !round_trip_mb_per_sec.is_finite()
        || round_trip_mb_per_sec <= 0.0
    {
        return None;
    }

    let failure_count = recent
        .iter()
        .filter(|observation| {
            matches!(
                observation.outcome,
                ObservationOutcome::Failure | ObservationOutcome::Invalid
            )
        })
        .count() as u32;
    let timeout_count = recent
        .iter()
        .filter(|observation| observation.outcome == ObservationOutcome::Timeout)
        .count() as u32;
    let observation_count = recent.len() as u32;
    let failure_rate = if observation_count == 0 {
        0.0
    } else {
        (failure_count + timeout_count) as f64 / observation_count as f64
    };
    let age_ms = observation_age_ms(latest_success).max(0) as u64;
    let measured_at = Instant::now()
        .checked_sub(Duration::from_millis(age_ms))
        .unwrap_or_else(Instant::now);

    Some(PeerLinkMetrics {
        rtt_us,
        round_trip_mb_per_sec,
        sample_count: latest_success.sample_count,
        measured_at,
        observation_count,
        failure_count,
        timeout_count,
        failure_rate,
    })
}

fn observation_age_ms(observation: &LinkObservation) -> i64 {
    now_ms().saturating_sub(observation.observed_at_unix_ms)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn load_state(path: &Path) -> io::Result<StoreState> {
    if !path.exists() {
        return Ok(StoreState::default());
    }

    let bytes = fs::read(path)?;
    let persisted: PersistedState = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    if persisted.version != HISTORY_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "versão de histórico incompatível: {} (esperada {})",
                persisted.version, HISTORY_VERSION
            ),
        ));
    }

    let now = now_ms();
    let max_age_ms = DEFAULT_HISTORY_WINDOW.as_millis() as i64;
    let mut state = StoreState::default();

    for link in persisted.links {
        if link.source_node_id.is_empty() || link.destination_node_id.is_empty() {
            continue;
        }

        let mut observations: VecDeque<LinkObservation> = link
            .observations
            .into_iter()
            .filter(|observation| now.saturating_sub(observation.observed_at_unix_ms) <= max_age_ms)
            .collect();
        while observations.len() > MAX_OBSERVATIONS_PER_LINK {
            observations.pop_front();
        }

        if !observations.is_empty() {
            state.links.insert(
                (link.source_node_id, link.destination_node_id),
                observations,
            );
        }
    }

    Ok(state)
}

fn persist_state(path: &Path, state: &StoreState) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    let persisted = PersistedState {
        version: HISTORY_VERSION,
        links: state
            .links
            .iter()
            .map(
                |((source_node_id, destination_node_id), observations)| PersistedLink {
                    source_node_id: source_node_id.clone(),
                    destination_node_id: destination_node_id.clone(),
                    observations: observations.clone(),
                },
            )
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&persisted)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("peer-link-history.json");
    let temporary_path = path.with_file_name(format!("{file_name}.tmp"));

    fs::write(&temporary_path, bytes)?;
    fs::rename(&temporary_path, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temporary_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rac-peer-link-{label}-{}-{}.json",
            std::process::id(),
            now_ms()
        ))
    }

    #[tokio::test]
    async fn historico_persiste_e_contabiliza_falhas_apos_reabrir() {
        let path = temporary_path("reopen");
        let store = PeerLinkStore::open(path.clone());

        store
            .record_success("worker-a", "worker-b", 120.0, 25.0, 3)
            .await;
        store
            .record_success("worker-a", "worker-b", 100.0, 30.0, 3)
            .await;
        store
            .record_failure("worker-a", "worker-b", ProbeOutcome::Timeout)
            .await;

        assert!(store
            .fresh_metrics("worker-a", "worker-b", Duration::from_secs(30))
            .await
            .is_none());

        drop(store);
        let reopened = PeerLinkStore::open(path.clone());
        let metrics = reopened
            .historical_metrics("worker-a", "worker-b", DEFAULT_HISTORY_WINDOW)
            .await
            .expect("a telemetria histórica deve sobreviver à reinicialização");

        assert!((metrics.rtt_us - 110.0).abs() < f64::EPSILON);
        assert!((metrics.round_trip_mb_per_sec - 27.5).abs() < f64::EPSILON);
        assert_eq!(metrics.timeout_count, 1);
        assert_eq!(metrics.observation_count, 3);
        assert!((metrics.failure_rate - (1.0 / 3.0)).abs() < 0.0001);

        drop(reopened);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn limite_de_observacoes_por_enlace_e_respeitado() {
        let store = PeerLinkStore::in_memory();
        for value in 0..(MAX_OBSERVATIONS_PER_LINK + 5) {
            store
                .record_success("worker-a", "worker-b", 100.0 + value as f64, 10.0, 3)
                .await;
        }

        let state = store.state.lock().await;
        let history = state
            .links
            .get(&("worker-a".to_owned(), "worker-b".to_owned()))
            .expect("enlace registrado");
        assert_eq!(history.len(), MAX_OBSERVATIONS_PER_LINK);
    }

    #[tokio::test]
    async fn falha_recente_invalida_cache_quente_mas_preserva_estimativa_historica() {
        let store = PeerLinkStore::in_memory();
        store
            .record_success("worker-a", "worker-b", 100.0, 10.0, 3)
            .await;

        assert!(store
            .fresh_metrics("worker-a", "worker-b", Duration::from_secs(30))
            .await
            .is_some());

        let fallback = store
            .record_failure("worker-a", "worker-b", ProbeOutcome::Failure)
            .await
            .expect("última medição válida deve servir como fallback");

        assert!(store
            .fresh_metrics("worker-a", "worker-b", Duration::from_secs(30))
            .await
            .is_none());
        assert_eq!(fallback.failure_count, 1);
        assert!((fallback.failure_rate - 0.5).abs() < 0.0001);
    }
}
