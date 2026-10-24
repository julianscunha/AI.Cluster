//! What the gateway needs from a language model, whatever its architecture.

use crate::cache::KvCache;
use std::sync::atomic::{AtomicBool, Ordering};

pub trait LanguageModel {
    fn context_len(&self) -> usize;

    fn vocab_size(&self) -> usize;

    fn new_cache(&self) -> KvCache;

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
        if cancel.load(Ordering::Relaxed) {
            return Err("geração cancelada".into());
        }
        let mut cache = self.new_cache();
        let mut produced = Vec::new();
        let mut logits = self.forward(prompt, &mut cache)?;
        while produced.len() < max_new {
            let next = logits
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(id, _)| id as u32)
                .ok_or("vocabulário vazio")?;
            produced.push(next);
            if Some(next) == stop || cache.len() >= self.context_len() {
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
