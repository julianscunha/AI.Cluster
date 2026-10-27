//! What the gateway measures about generation (F05): time to first token, gaps between tokens,
//! tokens per second and the size of the KV cache. The controller never sees tokens, so these
//! live here and go out on the gateway's own `GET /metrics`.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

use ai_cluster_protocol::cluster::GatewayStats;
use prost::Message;

/// Recent samples kept per series; the oldest are dropped, so percentiles describe recent load.
const KEEP: usize = 1024;

#[derive(Default)]
struct Inner {
    requests: u64,
    failed: u64,
    prompt_tokens: u64,
    generated_tokens: u64,
    /// Microseconds from the start of the request to its first token (the prompt included).
    ttft_us: VecDeque<u64>,
    /// Microseconds between one generated token and the next.
    itl_us: VecDeque<u64>,
    /// Tokens per second over each finished request, prompt excluded.
    tokens_per_second: VecDeque<f64>,
    kv_bytes_last: u64,
    kv_bytes_max: u64,
}

static METRICS: Mutex<Inner> = Mutex::new(Inner {
    requests: 0,
    failed: 0,
    prompt_tokens: 0,
    generated_tokens: 0,
    ttft_us: VecDeque::new(),
    itl_us: VecDeque::new(),
    tokens_per_second: VecDeque::new(),
    kv_bytes_last: 0,
    kv_bytes_max: 0,
});

fn lock() -> std::sync::MutexGuard<'static, Inner> {
    METRICS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn push<T>(series: &mut VecDeque<T>, value: T) {
    if series.len() == KEEP {
        series.pop_front();
    }
    series.push_back(value);
}

/// Times one generation, token by token. Built before the prompt runs, so the first token's
/// time includes it.
pub struct Sampler {
    started: Instant,
    last: Option<Instant>,
    ttft_us: Option<u64>,
    gaps_us: Vec<u64>,
    /// Tokens the client sees: the stop token is not one of them.
    tokens: usize,
}

impl Sampler {
    pub fn start() -> Self {
        Self {
            started: Instant::now(),
            last: None,
            ttft_us: None,
            gaps_us: Vec::new(),
            tokens: 0,
        }
    }

    /// Tokens the client has seen so far.
    pub fn tokens(&self) -> usize {
        self.tokens
    }

    /// Call when a token has just been chosen.
    pub fn token(&mut self) {
        self.tokens += 1;
        let now = Instant::now();
        match self.last {
            None => self.ttft_us = Some(now.duration_since(self.started).as_micros() as u64),
            Some(previous) => self
                .gaps_us
                .push(now.duration_since(previous).as_micros() as u64),
        }
        self.last = Some(now);
    }

    /// Records the finished (or abandoned) generation. The cache it ended with holds the prompt
    /// and every token that was fed back in: all of them when the model stopped (the stop token
    /// is not fed), all but the last otherwise.
    pub fn finish(self, ok: bool, prompt_tokens: usize, stopped: bool, kv_per_position: usize) {
        let tokens = self.tokens;
        let fed = if stopped {
            tokens
        } else {
            tokens.saturating_sub(1)
        };
        let kv_bytes = ((prompt_tokens + fed) * kv_per_position) as u64;
        let mut inner = lock();
        inner.requests += 1;
        if !ok {
            inner.failed += 1;
        }
        inner.prompt_tokens += prompt_tokens as u64;
        inner.generated_tokens += tokens as u64;
        if let Some(ttft) = self.ttft_us {
            push(&mut inner.ttft_us, ttft);
        }
        for gap in self.gaps_us {
            push(&mut inner.itl_us, gap);
        }
        // Decode rate: tokens after the first over the time since the first. One token has no rate.
        if let (Some(first), Some(last)) = (self.ttft_us, self.last)
            && tokens > 1
        {
            let decode_us = last.duration_since(self.started).as_micros() as u64 - first;
            if decode_us > 0 {
                push(
                    &mut inner.tokens_per_second,
                    (tokens - 1) as f64 / (decode_us as f64 / 1e6),
                );
            }
        }
        inner.kv_bytes_last = kv_bytes;
        inner.kv_bytes_max = inner.kv_bytes_max.max(kv_bytes);
    }
}

/// Nearest-rank percentile of `values` in `0..=1`; zero for an empty series.
fn percentile(values: &VecDeque<u64>, p: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted: Vec<u64> = values.iter().copied().collect();
    sorted.sort_unstable();
    let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

/// The numbers the controller shows in the dashboard: the counters and the percentiles. The cache
/// counts and the rates are filled by the caller, which owns the cache and the sample ring.
pub fn stats() -> GatewayStats {
    let inner = lock();
    let seconds = |us: u64| us as f64 / 1e6;
    let rates = &inner.tokens_per_second;
    GatewayStats {
        requests: inner.requests,
        failed: inner.failed,
        prompt_tokens: inner.prompt_tokens,
        generated_tokens: inner.generated_tokens,
        ttft_p50_seconds: seconds(percentile(&inner.ttft_us, 0.5)),
        ttft_p95_seconds: seconds(percentile(&inner.ttft_us, 0.95)),
        itl_p50_seconds: seconds(percentile(&inner.itl_us, 0.5)),
        itl_p95_seconds: seconds(percentile(&inner.itl_us, 0.95)),
        decode_tokens_per_second: if rates.is_empty() {
            0.0
        } else {
            rates.iter().sum::<f64>() / rates.len() as f64
        },
        kv_bytes_last: inner.kv_bytes_last,
        kv_bytes_max: inner.kv_bytes_max,
        ..Default::default()
    }
}

/// Starts the counters from a saved base, so a restart does not zero them. Percentiles and the KV
/// size are not restored: they describe recent load, which the new process has not seen.
pub fn restore(base: &GatewayStats) {
    let mut inner = lock();
    inner.requests = base.requests;
    inner.failed = base.failed;
    inner.prompt_tokens = base.prompt_tokens;
    inner.generated_tokens = base.generated_tokens;
}

/// Reads a state file written by [`save_state`]. A missing or unreadable file is "no base".
pub fn load_state(path: &std::path::Path) -> Option<GatewayStats> {
    GatewayStats::decode(std::fs::read(path).ok()?.as_slice()).ok()
}

/// Writes the stats to a sibling and renames it into place, so a crash leaves the old file whole.
pub fn save_state(path: &std::path::Path, stats: &GatewayStats) {
    let temp = path.with_extension("tmp");
    if let Err(err) =
        std::fs::write(&temp, stats.encode_to_vec()).and_then(|()| std::fs::rename(&temp, path))
    {
        eprintln!(
            "salvar estado do gateway em {} falhou: {err}",
            path.display()
        );
    }
}

/// Counter samples at report time, to turn totals into rates. Windows are asked in seconds; a
/// window the history does not cover yet has no rate (never extrapolated from a short span).
#[derive(Default)]
pub struct RateRing {
    /// `(seconds, requests, generated_tokens)`, oldest first.
    samples: VecDeque<(u64, u64, u64)>,
}

impl RateRing {
    /// Longest window asked, plus slack for the tick not landing exactly on it.
    const KEEP_SECS: u64 = 300 + 60;

    pub fn sample(&mut self, now_s: u64, requests: u64, tokens: u64) {
        self.samples.push_back((now_s, requests, tokens));
        // Keep one sample older than the longest window so it can still be the start of that window.
        while self.samples.len() > 2
            && self
                .samples
                .get(1)
                .is_some_and(|next| next.0 + Self::KEEP_SECS < now_s)
        {
            self.samples.pop_front();
        }
    }

    /// `(requests per second, tokens per second)` over the last `window_s`, or zeros when the
    /// oldest sample is younger than the window.
    pub fn rates(&self, now_s: u64, window_s: u64) -> (f64, f64) {
        let Some(&(now, requests, tokens)) = self.samples.back() else {
            return (0.0, 0.0);
        };
        let start = self
            .samples
            .iter()
            .rev()
            .find(|sample| sample.0 + window_s <= now_s.max(now));
        match start {
            Some(&(then, r0, t0)) if now > then => {
                let span = (now - then) as f64;
                (
                    requests.saturating_sub(r0) as f64 / span,
                    tokens.saturating_sub(t0) as f64 / span,
                )
            }
            _ => (0.0, 0.0),
        }
    }
}

/// The Prometheus text for `GET /metrics`.
pub fn render() -> String {
    let inner = lock();
    let seconds = |us: u64| us as f64 / 1e6;
    let rates = &inner.tokens_per_second;
    let mean_rate = if rates.is_empty() {
        0.0
    } else {
        rates.iter().sum::<f64>() / rates.len() as f64
    };
    let mut out = String::new();
    let mut line = |name: &str, kind: &str, help: &str, value: String| {
        out.push_str(&format!(
            "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}\n"
        ));
    };
    line(
        "ai_cluster_inference_requests_total",
        "counter",
        "Generations finished or abandoned.",
        inner.requests.to_string(),
    );
    line(
        "ai_cluster_inference_failed_total",
        "counter",
        "Generations that failed, timed out or were cancelled.",
        inner.failed.to_string(),
    );
    line(
        "ai_cluster_inference_prompt_tokens_total",
        "counter",
        "Prompt tokens run.",
        inner.prompt_tokens.to_string(),
    );
    line(
        "ai_cluster_inference_generated_tokens_total",
        "counter",
        "Tokens generated.",
        inner.generated_tokens.to_string(),
    );
    for (p, label) in [(0.5, "p50"), (0.95, "p95")] {
        line(
            &format!("ai_cluster_inference_ttft_seconds_{label}"),
            "gauge",
            "Time to first token over recent requests.",
            seconds(percentile(&inner.ttft_us, p)).to_string(),
        );
        line(
            &format!("ai_cluster_inference_itl_seconds_{label}"),
            "gauge",
            "Gap between generated tokens over recent requests.",
            seconds(percentile(&inner.itl_us, p)).to_string(),
        );
    }
    line(
        "ai_cluster_inference_tokens_per_second",
        "gauge",
        "Mean decode rate over recent requests.",
        mean_rate.to_string(),
    );
    line(
        "ai_cluster_inference_kv_cache_bytes_last",
        "gauge",
        "KV cache size at the end of the latest generation.",
        inner.kv_bytes_last.to_string(),
    );
    line(
        "ai_cluster_inference_kv_cache_bytes_max",
        "gauge",
        "Largest KV cache seen since the gateway started.",
        inner.kv_bytes_max.to_string(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank_and_empty_is_zero() {
        assert_eq!(percentile(&VecDeque::new(), 0.5), 0);
        assert_eq!(percentile(&VecDeque::from([7]), 0.95), 7);
        let values: VecDeque<u64> = (1..=20).collect();
        assert_eq!(percentile(&values, 0.5), 10);
        assert_eq!(percentile(&values, 0.95), 19);
    }

    #[test]
    fn a_janela_so_tem_taxa_quando_o_historico_a_cobre() {
        let mut ring = RateRing::default();
        ring.sample(0, 0, 0);
        ring.sample(15, 30, 300);
        ring.sample(30, 60, 600);
        // 30 s de história: a janela de 1 min ainda não está coberta
        assert_eq!(ring.rates(30, 60), (0.0, 0.0));
        ring.sample(60, 120, 1_200);
        // agora há uma amostra de 60 s atrás: 120 requisições e 1200 tokens em 60 s
        let (r, t) = ring.rates(60, 60);
        assert!((r - 2.0).abs() < 1e-9 && (t - 20.0).abs() < 1e-9, "{r} {t}");
        // contador que voltou (reinício sem base) não dá taxa negativa
        ring.sample(120, 5, 50);
        assert_eq!(ring.rates(120, 60).0, 0.0);
    }

    #[test]
    fn o_anel_descarta_o_que_passa_da_maior_janela_mas_guarda_uma_amostra_de_apoio() {
        let mut ring = RateRing::default();
        for step in 0..60u64 {
            ring.sample(step * 15, step * 10, step * 100);
        }
        // 900 s de amostras a cada 15 s: o anel não cresce sem limite
        assert!(ring.samples.len() <= 30, "{}", ring.samples.len());
        let (r, _) = ring.rates(59 * 15, 300);
        assert!((r - 10.0 / 15.0).abs() < 1e-9, "{r}");
    }

    #[test]
    fn o_estado_salvo_volta_como_base_dos_contadores() {
        let dir = std::env::temp_dir().join(format!("aic-gwstate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gw.state");
        assert!(load_state(&path).is_none(), "sem arquivo, sem base");
        let saved = GatewayStats {
            requests: 7,
            failed: 1,
            prompt_tokens: 70,
            generated_tokens: 700,
            ..Default::default()
        };
        save_state(&path, &saved);
        let loaded = load_state(&path).expect("o arquivo salvo carrega");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!((loaded.requests, loaded.generated_tokens), (7, 700));
        restore(&loaded);
        assert!(stats().requests >= 7, "os contadores partem da base");
    }

    #[test]
    fn a_sampler_splits_the_first_token_from_the_gaps() {
        let mut sampler = Sampler::start();
        std::thread::sleep(std::time::Duration::from_millis(5));
        sampler.token();
        std::thread::sleep(std::time::Duration::from_millis(5));
        sampler.token();
        sampler.token();
        assert!(sampler.ttft_us.is_some_and(|ttft| ttft >= 5_000));
        assert_eq!(sampler.gaps_us.len(), 2);
        assert!(sampler.gaps_us[0] >= 5_000);
    }

    #[test]
    fn a_finished_generation_shows_up_in_the_text() {
        let mut sampler = Sampler::start();
        sampler.token();
        sampler.token();
        sampler.finish(true, 3, false, 4096);
        let text = render();
        assert!(text.contains("ai_cluster_inference_requests_total"));
        assert!(text.contains("ai_cluster_inference_kv_cache_bytes_max"));
        assert!(!text.contains("ai_cluster_inference_requests_total 0\n"));
    }
}
