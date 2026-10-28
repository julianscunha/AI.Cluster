//! What the gateway needs from a language model, whatever its architecture.

use crate::cache::KvCache;
use crate::sampling::{Rng, Sampling, pick};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

pub trait LanguageModel {
    fn context_len(&self) -> usize;

    fn vocab_size(&self) -> usize;

    fn new_cache(&self) -> KvCache;

    /// Bytes the KV cache holds per position (keys and values of every layer), zero if unknown.
    fn kv_bytes_per_position(&self) -> usize {
        0
    }

    /// Bytes each node keeps resident for this model's remote slices (empty when every weight is
    /// local), for checking that the nodes can hold them before serving.
    fn resident_bytes(&self) -> BTreeMap<String, u64> {
        BTreeMap::new()
    }

    /// Runs `tokens` as the next positions after what `cache` already holds, extends the cache,
    /// and returns the logits for the token that follows the last of them.
    fn forward(&self, tokens: &[u32], cache: &mut KvCache) -> Result<Vec<f64>, String>;

    /// Logits over the vocabulary for the token that follows `tokens`.
    fn next_token_logits(&self, tokens: &[u32]) -> Result<Vec<f64>, String> {
        self.forward(tokens, &mut self.new_cache())
    }

    /// Greedy continuation: up to `max_new` tokens after `prompt`, stopping early (and
    /// including it) at `stop`. The prompt runs once; each new token is one cached step.
    /// `cancel` is checked between steps: setting it ends the run with an error.
    fn generate(
        &self,
        prompt: &[u32],
        max_new: usize,
        stop: Option<u32>,
        cancel: &AtomicBool,
    ) -> Result<Vec<u32>, String> {
        self.generate_with(prompt, max_new, stop, cancel, &mut |_| {})
    }

    /// [`generate`](Self::generate) that also hands each token to `on_token` the moment it is
    /// chosen (the stop token included), so a caller can stream it.
    fn generate_with(
        &self,
        prompt: &[u32],
        max_new: usize,
        stop: Option<u32>,
        cancel: &AtomicBool,
        on_token: &mut dyn FnMut(u32),
    ) -> Result<Vec<u32>, String> {
        self.generate_sampled(prompt, max_new, stop, cancel, &Sampling::GREEDY, on_token)
    }

    /// [`generate_with`](Self::generate_with) com a escolha do token dada por `sampling`: guloso
    /// (o padrão) ou amostrado por temperatura e núcleo, reproduzível pela semente.
    fn generate_sampled(
        &self,
        prompt: &[u32],
        max_new: usize,
        stop: Option<u32>,
        cancel: &AtomicBool,
        sampling: &Sampling,
        on_token: &mut dyn FnMut(u32),
    ) -> Result<Vec<u32>, String> {
        if cancel.load(Ordering::Relaxed) {
            return Err("geração cancelada".into());
        }
        let mut rng = Rng::new(sampling.seed);
        let mut cache = self.new_cache();
        let mut produced = Vec::new();
        let mut logits = self.forward(prompt, &mut cache)?;
        while produced.len() < max_new {
            let next = pick(&logits, sampling, &mut rng).ok_or("vocabulário vazio")?;
            produced.push(next);
            on_token(next);
            // No forward for a token that will not be followed by another.
            if Some(next) == stop || produced.len() >= max_new || cache.len() >= self.context_len()
            {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                return Err("geração cancelada".into());
            }
            logits = self.forward(&[next], &mut cache)?;
        }
        Ok(produced)
    }
}
