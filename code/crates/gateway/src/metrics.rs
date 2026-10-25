//! What the gateway measures about generation (F05): time to first token, gaps between tokens,
//! tokens per second and the size of the KV cache. The controller never sees tokens, so these
//! live here and go out on the gateway's own `GET /metrics`.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

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
