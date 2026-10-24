//! Llama-family forward pass on CPU (SmolLM2): RMSNorm, rotary embeddings, grouped-query
//! attention, SwiGLU MLP, tied output embedding. Same conventions as `gpt2`: weights as f64
//! `Matrix`, every dense layer through `Matrix::multiply_with_threads`.
//!
//! The seven projections of a layer can instead live on other nodes (`Llama::load_remote`): each
//! is cut by output rows into slices and run through an [`Executor`]. Embeddings, norms and the
//! output projection stay local.

use crate::cache::KvCache;
use crate::gpt2::{add_in_place, project_rows, softmax};
use crate::model::LanguageModel;
use crate::safetensors::Tensors;
use crate::shard::{Executor, Weight, apply, transposed};
use ai_cluster_common::matrix::Matrix;
use std::path::Path;
use std::sync::Arc;

/// Largest context a config may declare; the KV cache grows with it.
const MAX_CONTEXT: usize = 131_072;

struct Block {
    attn_norm: Vec<f64>,
    q: Weight,
    k: Weight,
    v: Weight,
    o: Weight,
    mlp_norm: Vec<f64>,
    gate: Weight,
    up: Weight,
    down: Weight,
}

pub struct Llama {
    heads: usize,
    kv_heads: usize,
    head_dim: usize,
    eps: f64,
    threads: usize,
    /// `theta^(-2i/head_dim)` for the rotated pairs `(i, i + head_dim/2)`.
    inv_freq: Vec<f64>,
    /// Token embeddings, also the (tied) output projection.
    embed: Matrix,
    context: usize,
    blocks: Vec<Block>,
    norm: Vec<f64>,
    executor: Option<Arc<dyn Executor>>,
}

impl Llama {
    /// All weights in this process.
    pub fn load(dir: &Path) -> Result<Self, String> {
        Self::load_with(dir, None)
    }

    /// Projections cut across `nodes` (at least one) and run through `executor`; `dir` is read
    /// only for the config, the shapes and the small local tensors.
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
        let (width, layers) = (int("hidden_size")?, int("num_hidden_layers")?);
        let (heads, kv_heads) = (int("num_attention_heads")?, int("num_key_value_heads")?);
        let inner = int("intermediate_size")?;
        let context = int("max_position_embeddings")?;
        let eps = config["rms_norm_eps"].as_f64().unwrap_or(1e-5);
        let theta = config["rope_theta"].as_f64().unwrap_or(10_000.0);
        if width == 0
            || layers == 0
            || inner == 0
            || context == 0
            || context > MAX_CONTEXT
            || !theta.is_finite()
            || theta <= 0.0
            || !eps.is_finite()
            || eps < 0.0
            || config
                .get("head_dim")
                .is_some_and(|d| d.as_u64() != Some((width / heads.max(1)) as u64))
        {
            return Err("config.json com valores fora do suportado".into());
        }
        // Variants this slice does not implement fail loudly instead of computing the wrong thing.
        if config["hidden_act"].as_str() != Some("silu")
            || !config["rope_scaling"].is_null()
            || config["attention_bias"].as_bool().unwrap_or(false)
            || config["mlp_bias"].as_bool().unwrap_or(false)
            || !config["tie_word_embeddings"].as_bool().unwrap_or(false)
        {
            return Err(
                "só silu, sem rope_scaling, sem bias e com embeddings amarrados são suportados"
                    .into(),
            );
        }
        if heads == 0
            || kv_heads == 0
            || !width.is_multiple_of(heads)
            || !heads.is_multiple_of(kv_heads)
            || !(width / heads).is_multiple_of(2)
        {
            return Err("cabeças de atenção incoerentes com a largura".into());
        }
        let head_dim = width / heads;

        let tensors = Tensors::open(&dir.join("model.safetensors"))?;
        let (executor, nodes) = match remote {
            Some((executor, nodes)) => (Some(executor), nodes),
            None => (None, Vec::new()),
        };
        let weight = |name: String, rows: usize, cols: usize| -> Result<Weight, String> {
            if tensors.shape(&name) != Some(&[rows, cols][..]) {
                return Err(format!("{name}: forma diferente de {rows}x{cols}"));
            }
            if nodes.is_empty() {
                return Ok(Weight::Local(transposed(&tensors.matrix(&name)?)));
            }
            Weight::split(&nodes, &name, rows, false)
        };
        let vector = |name: String, len: usize| -> Result<Vec<f64>, String> {
            let v = tensors.vector(&name)?;
            if v.len() != len {
                return Err(format!("{name}: tamanho {}, esperava {len}", v.len()));
            }
            Ok(v)
        };

        let embed = tensors.matrix("model.embed_tokens.weight")?;
        if embed.cols as usize != width || embed.rows == 0 {
            return Err("embeddings incoerentes com hidden_size".into());
        }
        let kv_width = kv_heads * head_dim;
        let blocks = (0..layers)
            .map(|n| {
                let at = |part: &str| format!("model.layers.{n}.{part}.weight");
                Ok(Block {
                    attn_norm: vector(at("input_layernorm"), width)?,
                    q: weight(at("self_attn.q_proj"), width, width)?,
                    k: weight(at("self_attn.k_proj"), kv_width, width)?,
                    v: weight(at("self_attn.v_proj"), kv_width, width)?,
                    o: weight(at("self_attn.o_proj"), width, width)?,
                    mlp_norm: vector(at("post_attention_layernorm"), width)?,
                    gate: weight(at("mlp.gate_proj"), inner, width)?,
                    up: weight(at("mlp.up_proj"), inner, width)?,
                    down: weight(at("mlp.down_proj"), width, inner)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            heads,
            kv_heads,
            head_dim,
            eps,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
            inv_freq: (0..head_dim / 2)
                .map(|i| theta.powf(-2.0 * i as f64 / head_dim as f64))
                .collect(),
            embed,
            context,
            blocks,
            norm: vector("model.norm.weight".into(), width)?,
            executor,
        })
    }

    fn linear(&self, x: &Matrix, weights: &[&Weight]) -> Result<Vec<Matrix>, String> {
        apply(self.executor.as_ref(), self.threads, x, weights)
    }

    /// Rotates each `head_dim`-wide head of every row by its position (the "rotate half"
    /// layout: element `i` pairs with `i + head_dim/2`). `first` is the position of row 0.
    fn rope(&self, m: &mut Matrix, first: usize) {
        let (half, width) = (self.head_dim / 2, m.cols as usize);
        for (t, row) in m.data.chunks_exact_mut(width).enumerate() {
            let position = (first + t) as f64;
            for head in row.chunks_exact_mut(self.head_dim) {
                for (i, freq) in self.inv_freq.iter().enumerate() {
                    let (sin, cos) = (position * freq).sin_cos();
                    let (a, b) = (head[i], head[i + half]);
                    head[i] = a * cos - b * sin;
                    head[i + half] = b * cos + a * sin;
                }
            }
        }
    }

    /// Grouped-query causal attention: `heads` query heads share `kv_heads` key/value heads. The
    /// rotated keys and the values of the new rows are appended to `cache` for `layer`.
    fn attention(
        &self,
        block: &Block,
        input: &Matrix,
        cache: &mut KvCache,
        layer: usize,
    ) -> Result<Matrix, String> {
        let tokens = input.rows as usize;
        let past = cache.len;
        let [mut q, mut k, v]: [Matrix; 3] = self
            .linear(input, &[&block.q, &block.k, &block.v])?
            .try_into()
            .map_err(|_| "saídas de q, k e v")?;
        self.rope(&mut q, past);
        self.rope(&mut k, past);
        cache.keys[layer].extend_from_slice(&k.data);
        cache.values[layer].extend_from_slice(&v.data);
        let (keys, values) = (&cache.keys[layer], &cache.values[layer]);

        let (hd, kv_width) = (self.head_dim, self.kv_heads * self.head_dim);
        let width = self.heads * hd;
        let group = self.heads / self.kv_heads;
        let scale = 1.0 / (hd as f64).sqrt();
        let mut mixed = Matrix::zeros(tokens as u32, width as u32);
        let mut weights = vec![0.0; past + tokens];
        for head in 0..self.heads {
            let (q_at, kv_at) = (head * hd, (head / group) * hd);
            for t in 0..tokens {
                let query = &q.data[t * width + q_at..t * width + q_at + hd];
                let visible = past + t + 1;
                for (s, weight) in weights[..visible].iter_mut().enumerate() {
                    let key = &keys[s * kv_width + kv_at..s * kv_width + kv_at + hd];
                    *weight = query.iter().zip(key).map(|(a, b)| a * b).sum::<f64>() * scale;
                }
                softmax(&mut weights[..visible]);
                let out = &mut mixed.data[t * width + q_at..t * width + q_at + hd];
                for (s, weight) in weights[..visible].iter().enumerate() {
                    let value = &values[s * kv_width + kv_at..s * kv_width + kv_at + hd];
                    for (o, v) in out.iter_mut().zip(value) {
                        *o += weight * v;
                    }
                }
            }
        }
        let mut out = self.linear(&mixed, &[&block.o])?;
        out.pop().ok_or_else(|| "saída de o".to_string())
    }
}

fn rms_norm(x: &Matrix, gain: &[f64], eps: f64) -> Matrix {
    let mut out = x.clone();
    for row in out.data.chunks_exact_mut(gain.len()) {
        let mean_square = row.iter().map(|v| v * v).sum::<f64>() / row.len() as f64;
        let inv = 1.0 / (mean_square + eps).sqrt();
        for (value, g) in row.iter_mut().zip(gain) {
            *value = *value * inv * g;
        }
    }
    out
}

fn silu(x: f64) -> f64 {
    x / (1.0 + (-x).exp())
}

impl LanguageModel for Llama {
    fn context_len(&self) -> usize {
        self.context
    }

    fn vocab_size(&self) -> usize {
        self.embed.rows as usize
    }

    fn new_cache(&self) -> KvCache {
        KvCache::new(self.blocks.len())
    }

    fn forward(&self, tokens: &[u32], cache: &mut KvCache) -> Result<Vec<f64>, String> {
        if tokens.is_empty() {
            return Err("prompt vazio".into());
        }
        if cache.keys.len() != self.blocks.len() {
            return Err("cache de outro modelo".into());
        }
        let past = cache.len;
        if past + tokens.len() > self.context {
            return Err(format!(
                "{} tokens excedem o contexto de {}",
                past + tokens.len(),
                self.context
            ));
        }
        let width = self.embed.cols as usize;
        let mut x = Matrix::zeros(tokens.len() as u32, width as u32);
        for (row, token) in tokens.iter().enumerate() {
            let token = *token as usize;
            if token >= self.vocab_size() {
                return Err(format!("token {token} fora do vocabulário"));
            }
            x.data[row * width..(row + 1) * width]
                .copy_from_slice(&self.embed.data[token * width..(token + 1) * width]);
        }

        for (layer, block) in self.blocks.iter().enumerate() {
            let normed = rms_norm(&x, &block.attn_norm, self.eps);
            let attended = self.attention(block, &normed, cache, layer)?;
            add_in_place(&mut x, &attended);

            let normed = rms_norm(&x, &block.mlp_norm, self.eps);
            let [mut hidden, up]: [Matrix; 2] = self
                .linear(&normed, &[&block.gate, &block.up])?
                .try_into()
                .map_err(|_| "saídas de gate e up")?;
            for (g, u) in hidden.data.iter_mut().zip(&up.data) {
                *g = silu(*g) * u;
            }
            let down = self
                .linear(&hidden, &[&block.down])?
                .pop()
                .ok_or("saída de down")?;
            add_in_place(&mut x, &down);
        }
        cache.len = past + tokens.len();

        let last = tokens.len() - 1;
        let normed = rms_norm(&x, &self.norm, self.eps);
        let last_row = &normed.data[last * width..(last + 1) * width];
        Ok(project_rows(&self.embed, last_row, self.threads))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(head_dim: usize) -> Llama {
        Llama {
            heads: 1,
            kv_heads: 1,
            head_dim,
            eps: 1e-5,
            threads: 1,
            inv_freq: (0..head_dim / 2)
                .map(|i| 10_000f64.powf(-2.0 * i as f64 / head_dim as f64))
                .collect(),
            embed: Matrix::zeros(0, 0),
            context: 0,
            blocks: Vec::new(),
            norm: Vec::new(),
            executor: None,
        }
    }

    #[test]
    fn rms_norm_scales_to_unit_mean_square() {
        let x = Matrix {
            rows: 1,
            cols: 4,
            data: vec![1.0, 2.0, 3.0, 4.0],
        };
        let y = rms_norm(&x, &[1.0; 4], 0.0);
        let mean_square = y.data.iter().map(|v| v * v).sum::<f64>() / 4.0;
        assert!((mean_square - 1.0).abs() < 1e-12);
    }

    #[test]
    fn silu_matches_known_points() {
        assert_eq!(silu(0.0), 0.0);
        assert!((silu(1.0) - 0.731_058_578_6).abs() < 1e-9);
    }

    #[test]
    fn rope_is_a_rotation_that_leaves_position_zero_alone() {
        let llama = model(4);
        let row = vec![1.0, 2.0, 3.0, 4.0];
        let mut at_zero = Matrix {
            rows: 1,
            cols: 4,
            data: row.clone(),
        };
        llama.rope(&mut at_zero, 0);
        assert_eq!(at_zero.data, row);

        let mut later = Matrix {
            rows: 1,
            cols: 4,
            data: row.clone(),
        };
        llama.rope(&mut later, 7);
        let norm = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>();
        assert!((norm(&later.data) - norm(&row)).abs() < 1e-12);
        assert_ne!(later.data, row);
        // Row 1 of a two-row matrix starting at 6 is the same as a single row starting at 7.
        let mut two = Matrix {
            rows: 2,
            cols: 4,
            data: [row.clone(), row.clone()].concat(),
        };
        llama.rope(&mut two, 6);
        assert_eq!(&two.data[4..], &later.data[..]);
    }

    #[test]
    fn transposing_swaps_rows_and_columns() {
        let m = Matrix {
            rows: 2,
            cols: 3,
            data: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
        };
        let t = transposed(&m);
        assert_eq!((t.rows, t.cols), (3, 2));
        assert_eq!(t.data, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }
}
