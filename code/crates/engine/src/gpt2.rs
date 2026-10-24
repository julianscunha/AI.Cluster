//! GPT-2 forward pass on CPU, from `config.json` + `model.safetensors`.
//!
//! Weights are stored as `Matrix` (f64) because that is what the cluster's workers multiply; the
//! values come from f32 files, so only the accumulation is wider than the reference. Every dense
//! layer goes through `Matrix::multiply_with_threads`, the single place a later slice can swap
//! for a distributed product.

use crate::cache::KvCache;
use crate::model::LanguageModel;
use crate::safetensors::Tensors;
use crate::shard::{Executor, Weight, apply};
use ai_cluster_common::matrix::Matrix;
use std::path::Path;
use std::sync::Arc;

struct Linear {
    /// GPT-2 stores `Conv1D` weights as `[in, out]`, so `x * w + b` needs no transpose. Remote
    /// slices are bands of output columns.
    weight: Weight,
    bias: Vec<f64>,
    input: usize,
    output: usize,
}

struct Norm {
    gain: Vec<f64>,
    bias: Vec<f64>,
}

struct Block {
    ln_1: Norm,
    qkv: Linear,
    attn_out: Linear,
    ln_2: Norm,
    fc: Linear,
    proj: Linear,
}

pub struct Gpt2 {
    heads: usize,
    eps: f64,
    threads: usize,
    /// Token embeddings, also the (tied) output projection.
    wte: Matrix,
    wpe: Matrix,
    blocks: Vec<Block>,
    ln_f: Norm,
    executor: Option<Arc<dyn Executor>>,
}

impl Gpt2 {
    /// All weights in this process.
    pub fn load(dir: &Path) -> Result<Self, String> {
        Self::load_with(dir, None)
    }

    /// The four projections of every block cut across `nodes` (at least one) and run through
    /// `executor`; `dir` is read for the config, the shapes, the biases and the small tensors.
    pub fn load_remote(
        dir: &Path,
        executor: Arc<dyn Executor>,
        nodes: Vec<String>,
    ) -> Result<Self, String> {
        if nodes.is_empty() {
            return Err("sem nós para as fatias".into());
        }
        Self::load_with(dir, Some((executor, nodes)))
    }

    fn load_with(
        dir: &Path,
        remote: Option<(Arc<dyn Executor>, Vec<String>)>,
    ) -> Result<Self, String> {
        let (executor, nodes) = match remote {
            Some((executor, nodes)) => (Some(executor), nodes),
            None => (None, Vec::new()),
        };
        let config: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("config.json"))
                .map_err(|e| format!("config.json: {e}"))?,
        )
        .map_err(|e| format!("config.json: {e}"))?;
        let int = |key: &str| {
            config[key]
                .as_u64()
                .map(|v| v as usize)
                .ok_or_else(|| format!("config.json sem {key}"))
        };
        let (layers, heads) = (int("n_layer")?, int("n_head")?);
        let eps = config["layer_norm_epsilon"].as_f64().unwrap_or(1e-5);
        if config["activation_function"].as_str() != Some("gelu_new") {
            return Err("só a ativação gelu_new é suportada".into());
        }

        let tensors = Tensors::open(&dir.join("model.safetensors"))?;
        // Checkpoints saved from the LM-head class prefix every name with `transformer.`.
        let prefix = if tensors.shape("wte.weight").is_some() {
            ""
        } else {
            "transformer."
        };
        let name = |suffix: &str| format!("{prefix}{suffix}");
        let linear = |base: String| -> Result<Linear, String> {
            let weight = format!("{base}.weight");
            let [input, output] = tensors
                .shape(&weight)
                .and_then(|shape| <[usize; 2]>::try_from(shape).ok())
                .ok_or_else(|| format!("{weight}: esperava 2 dimensões"))?;
            Ok(Linear {
                weight: if nodes.is_empty() {
                    Weight::Local(tensors.matrix(&weight)?)
                } else {
                    Weight::split(&nodes, &weight, output, true)?
                },
                bias: tensors.vector(&format!("{base}.bias"))?,
                input,
                output,
            })
        };
        let norm = |base: String| -> Result<Norm, String> {
            Ok(Norm {
                gain: tensors.vector(&format!("{base}.weight"))?,
                bias: tensors.vector(&format!("{base}.bias"))?,
            })
        };

        let wte = tensors.matrix(&name("wte.weight"))?;
        let wpe = tensors.matrix(&name("wpe.weight"))?;
        let width = wte.cols as usize;
        if width == 0 || heads == 0 || !width.is_multiple_of(heads) || wpe.cols as usize != width {
            return Err("n_head não divide a largura do modelo".into());
        }
        let blocks = (0..layers)
            .map(|n| {
                let block = Block {
                    ln_1: norm(name(&format!("h.{n}.ln_1")))?,
                    qkv: linear(name(&format!("h.{n}.attn.c_attn")))?,
                    attn_out: linear(name(&format!("h.{n}.attn.c_proj")))?,
                    ln_2: norm(name(&format!("h.{n}.ln_2")))?,
                    fc: linear(name(&format!("h.{n}.mlp.c_fc")))?,
                    proj: linear(name(&format!("h.{n}.mlp.c_proj")))?,
                };
                let biases_fit = [&block.qkv, &block.attn_out, &block.fc, &block.proj]
                    .iter()
                    .all(|l| l.bias.len() == l.output);
                let norms_fit = [&block.ln_1, &block.ln_2]
                    .iter()
                    .all(|n| n.gain.len() == width && n.bias.len() == width);
                let shapes_fit = biases_fit
                    && norms_fit
                    && block.qkv.input == width
                    && block.qkv.output == 3 * width
                    && block.attn_out.input == width
                    && block.attn_out.output == width
                    && block.fc.input == width
                    && block.proj.output == width
                    && block.fc.output == block.proj.input;
                if shapes_fit {
                    Ok(block)
                } else {
                    Err(format!("h.{n}: formas incoerentes"))
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        let ln_f = norm(name("ln_f"))?;
        if ln_f.gain.len() != width || ln_f.bias.len() != width {
            return Err("ln_f: formas incoerentes".into());
        }
        Ok(Self {
            heads,
            eps,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
            wte,
            wpe,
            blocks,
            ln_f,
            executor,
        })
    }
}

impl LanguageModel for Gpt2 {
    fn context_len(&self) -> usize {
        self.wpe.rows as usize
    }

    fn vocab_size(&self) -> usize {
        self.wte.rows as usize
    }

    fn new_cache(&self) -> KvCache {
        KvCache::new(self.blocks.len())
    }

    /// Runs `tokens` as the next positions after what `cache` already holds, extends the cache,
    /// and returns the logits for the token that follows the last of them.
    fn forward(&self, tokens: &[u32], cache: &mut KvCache) -> Result<Vec<f64>, String> {
        if tokens.is_empty() {
            return Err("prompt vazio".into());
        }
        if cache.keys.len() != self.blocks.len() {
            return Err("cache de outro modelo".into());
        }
        let past = cache.len;
        if past + tokens.len() > self.context_len() {
            return Err(format!(
                "{} tokens excedem o contexto de {}",
                past + tokens.len(),
                self.context_len()
            ));
        }
        let width = self.wte.cols as usize;
        let mut x = Matrix::zeros(tokens.len() as u32, width as u32);
        for (row, token) in tokens.iter().enumerate() {
            let token = *token as usize;
            if token >= self.vocab_size() {
                return Err(format!("token {token} fora do vocabulário"));
            }
            for col in 0..width {
                x.data[row * width + col] =
                    self.wte.data[token * width + col] + self.wpe.data[(past + row) * width + col];
            }
        }

        for (layer, block) in self.blocks.iter().enumerate() {
            let normed = layer_norm(&x, &block.ln_1, self.eps);
            let attended = self.attention(block, &normed, cache, layer)?;
            add_in_place(&mut x, &attended);
            let hidden = self.linear(&layer_norm(&x, &block.ln_2, self.eps), &block.fc)?;
            let hidden = Matrix {
                data: hidden.data.iter().map(|v| gelu(*v)).collect(),
                ..hidden
            };
            add_in_place(&mut x, &self.linear(&hidden, &block.proj)?);
        }
        cache.len = past + tokens.len();

        let last = tokens.len() - 1;
        let normed = layer_norm(&x, &self.ln_f, self.eps);
        let last_row = &normed.data[last * width..(last + 1) * width];
        Ok(project_rows(&self.wte, last_row, self.threads))
    }
}

impl Gpt2 {
    /// Multi-head causal self-attention over the normalised input, ending in the output
    /// projection. The new keys and values are appended to `cache` for `layer`, and each new
    /// position attends to everything cached before it plus itself.
    fn attention(
        &self,
        block: &Block,
        input: &Matrix,
        cache: &mut KvCache,
        layer: usize,
    ) -> Result<Matrix, String> {
        let tokens = input.rows as usize;
        let width = input.cols as usize;
        let head_dim = width / self.heads;
        let qkv = self.linear(input, &block.qkv)?;
        let stride = 3 * width;
        let scale = 1.0 / (head_dim as f64).sqrt();
        let past = cache.len;
        for row in qkv.data.chunks_exact(stride) {
            cache.keys[layer].extend_from_slice(&row[width..2 * width]);
            cache.values[layer].extend_from_slice(&row[2 * width..]);
        }
        let (keys, values) = (&cache.keys[layer], &cache.values[layer]);

        let mut mixed = Matrix::zeros(tokens as u32, width as u32);
        let mut weights = vec![0.0; past + tokens];
        for head in 0..self.heads {
            let at = head * head_dim;
            for t in 0..tokens {
                let q = &qkv.data[t * stride + at..t * stride + at + head_dim];
                // Causal: new position t sees the cache and the new positions up to itself.
                let visible = past + t + 1;
                for (s, weight) in weights[..visible].iter_mut().enumerate() {
                    let k = &keys[s * width + at..s * width + at + head_dim];
                    *weight = q.iter().zip(k).map(|(a, b)| a * b).sum::<f64>() * scale;
                }
                softmax(&mut weights[..visible]);
                let out = &mut mixed.data[t * width + at..t * width + at + head_dim];
                for (s, weight) in weights[..visible].iter().enumerate() {
                    let v = &values[s * width + at..s * width + at + head_dim];
                    for (o, v) in out.iter_mut().zip(v) {
                        *o += weight * v;
                    }
                }
            }
        }
        self.linear(&mixed, &block.attn_out)
    }

    fn linear(&self, x: &Matrix, layer: &Linear) -> Result<Matrix, String> {
        linear(x, layer, self.executor.as_ref(), self.threads)
    }
}

fn linear(
    x: &Matrix,
    layer: &Linear,
    executor: Option<&Arc<dyn Executor>>,
    threads: usize,
) -> Result<Matrix, String> {
    let mut y = apply(executor, threads, x, &[&layer.weight])?
        .pop()
        .ok_or("saída de projeção ausente")?;
    for row in y.data.chunks_exact_mut(layer.bias.len()) {
        for (value, bias) in row.iter_mut().zip(&layer.bias) {
            *value += bias;
        }
    }
    Ok(y)
}

fn layer_norm(x: &Matrix, norm: &Norm, eps: f64) -> Matrix {
    let width = x.cols as usize;
    let mut out = x.clone();
    for row in out.data.chunks_exact_mut(width) {
        let mean = row.iter().sum::<f64>() / width as f64;
        let variance = row.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / width as f64;
        let inv = 1.0 / (variance + eps).sqrt();
        for (i, value) in row.iter_mut().enumerate() {
            *value = (*value - mean) * inv * norm.gain[i] + norm.bias[i];
        }
    }
    out
}

/// `table * row`: the dot product of `row` with every row of `table` (the tied output
/// projection), split over `threads`. Each logit is one dot product, so the split does not
/// change any value.
pub(crate) fn project_rows(table: &Matrix, row: &[f64], threads: usize) -> Vec<f64> {
    let (rows, width) = (table.rows as usize, row.len());
    let band = rows.div_ceil(threads.max(1)).max(1);
    let mut logits = vec![0.0; rows];
    std::thread::scope(|scope| {
        for (out, rows) in logits.chunks_mut(band).zip(table.data.chunks(band * width)) {
            scope.spawn(move || {
                for (logit, embedding) in out.iter_mut().zip(rows.chunks_exact(width)) {
                    *logit = embedding.iter().zip(row).map(|(a, b)| a * b).sum();
                }
            });
        }
    });
    logits
}

pub(crate) fn softmax(values: &mut [f64]) {
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut total = 0.0;
    for value in values.iter_mut() {
        *value = (*value - max).exp();
        total += *value;
    }
    for value in values.iter_mut() {
        *value /= total;
    }
}

/// GPT-2's `gelu_new`: the tanh approximation.
fn gelu(x: f64) -> f64 {
    0.5 * x * (1.0 + ((2.0 / std::f64::consts::PI).sqrt() * (x + 0.044715 * x.powi(3))).tanh())
}

pub(crate) fn add_in_place(x: &mut Matrix, delta: &Matrix) {
    for (a, b) in x.data.iter_mut().zip(&delta.data) {
        *a += b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softmax_sums_to_one_and_is_stable_for_large_inputs() {
        let mut values = [1000.0, 1001.0, 999.0];
        softmax(&mut values);
        assert!((values.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert!(values[1] > values[0] && values[0] > values[2]);
    }

    #[test]
    fn layer_norm_centres_and_scales() {
        let x = Matrix {
            rows: 1,
            cols: 4,
            data: vec![1.0, 2.0, 3.0, 4.0],
        };
        let norm = Norm {
            gain: vec![1.0; 4],
            bias: vec![0.0; 4],
        };
        let y = layer_norm(&x, &norm, 0.0);
        assert!(y.data.iter().sum::<f64>().abs() < 1e-12);
        let variance = y.data.iter().map(|v| v * v).sum::<f64>() / 4.0;
        assert!((variance - 1.0).abs() < 1e-12);
    }

    #[test]
    fn gelu_matches_known_points() {
        assert_eq!(gelu(0.0), 0.0);
        assert!((gelu(1.0) - 0.841192).abs() < 1e-5);
        assert!((gelu(-10.0)).abs() < 1e-6);
    }

    #[test]
    fn linear_adds_the_bias_to_every_row() {
        let layer = Linear {
            weight: Weight::Local(Matrix {
                rows: 2,
                cols: 2,
                data: vec![1.0, 0.0, 0.0, 1.0],
            }),
            bias: vec![10.0, 20.0],
            input: 2,
            output: 2,
        };
        let x = Matrix {
            rows: 2,
            cols: 2,
            data: vec![1.0, 2.0, 3.0, 4.0],
        };
        assert_eq!(
            linear(&x, &layer, None, 2).unwrap().data,
            vec![11.0, 22.0, 13.0, 24.0]
        );
    }
}
