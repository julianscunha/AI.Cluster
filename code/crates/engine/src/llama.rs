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
use std::collections::BTreeMap;
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
    /// Viés de q, k e v (Qwen2): somado depois da projeção e antes do rope.
    qkv_bias: Option<[Vec<f64>; 3]>,
}

pub struct Llama {
    heads: usize,
    kv_heads: usize,
    head_dim: usize,
    eps: f64,
    threads: usize,
    /// `theta^(-2i/head_dim)` for the rotated pairs `(i, i + head_dim/2)`.
    inv_freq: Vec<f64>,
    /// Token embeddings; also the output projection when the config ties them (`head` is `None`).
    embed: Matrix,
    /// The separate output projection (`lm_head.weight`, vocab x width) of models that do not tie it.
    head: Option<Matrix>,
    context: usize,
    blocks: Vec<Block>,
    norm: Vec<f64>,
    executor: Option<Arc<dyn Executor>>,
    /// Janela deslizante uniforme (Mistral): cada token vê só os últimos `window` (ele incluído).
    window: Option<usize>,
}

/// `theta^(-2i/head_dim)` para os pares girados, com o `rope_scaling` da configuração aplicado:
/// `default` (nenhum), `linear` (divide as frequências pelo fator) e `llama3` (Llama 3.1+: mantém as
/// altas, divide as baixas e suaviza as do meio). Outro tipo (yarn, dynamic, longrope) é recusado.
fn rope_frequencies(
    config: &serde_json::Value,
    theta: f64,
    head_dim: usize,
) -> Result<Vec<f64>, String> {
    let base: Vec<f64> = (0..head_dim / 2)
        .map(|i| theta.powf(-2.0 * i as f64 / head_dim as f64))
        .collect();
    let scaling = &config["rope_scaling"];
    if scaling.is_null() {
        return Ok(base);
    }
    let kind = scaling
        .get("rope_type")
        .or_else(|| scaling.get("type"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("default");
    let number = |key: &str| {
        scaling[key]
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| format!("rope_scaling {kind} sem {key} válido"))
    };
    match kind {
        "default" => Ok(base),
        "linear" => {
            let factor = number("factor")?;
            Ok(base.iter().map(|freq| freq / factor).collect())
        }
        "llama3" => {
            let (factor, low, high) = (
                number("factor")?,
                number("low_freq_factor")?,
                number("high_freq_factor")?,
            );
            let original = number("original_max_position_embeddings")?;
            if high <= low {
                return Err(
                    "rope_scaling llama3: high_freq_factor deve passar de low_freq_factor".into(),
                );
            }
            let (low_wavelen, high_wavelen) = (original / low, original / high);
            Ok(base
                .iter()
                .map(|&freq| {
                    let wavelen = std::f64::consts::TAU / freq;
                    if wavelen < high_wavelen {
                        freq
                    } else if wavelen > low_wavelen {
                        freq / factor
                    } else {
                        let smooth = (original / wavelen - low) / (high - low);
                        (1.0 - smooth) * freq / factor + smooth * freq
                    }
                })
                .collect())
        }
        other => Err(format!(
            "rope_scaling '{other}' não é suportado (só default, linear e llama3)"
        )),
    }
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
            || config["attention_bias"].as_bool().unwrap_or(false)
            || config["mlp_bias"].as_bool().unwrap_or(false)
        {
            return Err("só silu e sem bias de atenção ou de MLP são suportados".into());
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
        let inv_freq = rope_frequencies(&config, theta, head_dim)?;
        // Sem a chave, o padrão do `transformers` é não amarrar: a saída é o `lm_head.weight`.
        let tied = config["tie_word_embeddings"].as_bool().unwrap_or(false);

        let tensors = Tensors::open(&dir.join("model.safetensors"))?;
        // O Qwen2 não declara `attention_bias` no config: o viés de q/k/v é implícito e vem nos tensores.
        // Viés em o_proj (Llama com attention_bias) não é suportado e seria ignorado.
        if tensors
            .shape("model.layers.0.self_attn.o_proj.bias")
            .is_some()
        {
            return Err("viés em o_proj não é suportado pelo motor próprio".into());
        }
        // Janela deslizante: só a uniforme (todas as camadas). O Qwen2 declara `sliding_window` mas o
        // desliga com `use_sliding_window: false`; camadas alternadas (Gemma 2) são recusadas.
        if config.get("layer_types").is_some() {
            return Err("janela deslizante por camada não é suportada".into());
        }
        let window = match config["sliding_window"].as_u64() {
            Some(w) if config["use_sliding_window"].as_bool() != Some(false) => {
                Some(usize::try_from(w).map_err(|_| "sliding_window grande demais")?)
            }
            _ => None,
        }
        .filter(|w| *w > 0 && *w < context);
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
            Weight::split(&nodes, &name, rows, cols, false)
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
        let head = if tied {
            None
        } else {
            let head = tensors
                .matrix("lm_head.weight")
                .map_err(|e| format!("embeddings não amarrados pedem lm_head.weight: {e}"))?;
            if head.rows != embed.rows || head.cols != embed.cols {
                return Err("lm_head.weight com forma diferente dos embeddings".into());
            }
            Some(head)
        };
        let kv_width = kv_heads * head_dim;
        let blocks = (0..layers)
            .map(|n| {
                let at = |part: &str| format!("model.layers.{n}.{part}.weight");
                let bias_at = |part: &str| format!("model.layers.{n}.{part}.bias");
                let qkv_bias = if tensors.shape(&bias_at("self_attn.q_proj")).is_some() {
                    Some([
                        vector(bias_at("self_attn.q_proj"), width)?,
                        vector(bias_at("self_attn.k_proj"), kv_width)?,
                        vector(bias_at("self_attn.v_proj"), kv_width)?,
                    ])
                } else {
                    None
                };
                Ok(Block {
                    qkv_bias,
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
            inv_freq,
            embed,
            head,
            context,
            blocks,
            norm: vector("model.norm.weight".into(), width)?,
            executor,
            window,
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
        let [mut q, mut k, mut v]: [Matrix; 3] = self
            .linear(input, &[&block.q, &block.k, &block.v])?
            .try_into()
            .map_err(|_| "saídas de q, k e v")?;
        if let Some([bq, bk, bv]) = &block.qkv_bias {
            for (m, bias) in [(&mut q, bq), (&mut k, bk), (&mut v, bv)] {
                for row in m.data.chunks_exact_mut(bias.len()) {
                    row.iter_mut().zip(bias).for_each(|(x, b)| *x += b);
                }
            }
        }
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
                let first = self.window.map_or(0, |w| visible.saturating_sub(w));
                for (s, weight) in weights[first..visible].iter_mut().enumerate() {
                    let key =
                        &keys[(first + s) * kv_width + kv_at..(first + s) * kv_width + kv_at + hd];
                    *weight = query.iter().zip(key).map(|(a, b)| a * b).sum::<f64>() * scale;
                }
                softmax(&mut weights[first..visible]);
                let out = &mut mixed.data[t * width + q_at..t * width + q_at + hd];
                for (s, weight) in weights[first..visible].iter().enumerate() {
                    let value = &values
                        [(first + s) * kv_width + kv_at..(first + s) * kv_width + kv_at + hd];
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

    fn kv_bytes_per_position(&self) -> usize {
        self.blocks.len() * 2 * self.kv_heads * self.head_dim * std::mem::size_of::<f64>()
    }

    fn resident_bytes(&self) -> BTreeMap<String, u64> {
        crate::shard::resident_bytes(
            self.blocks
                .iter()
                .flat_map(|b| [&b.q, &b.k, &b.v, &b.o, &b.gate, &b.up, &b.down]),
        )
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
        Ok(project_rows(
            self.head.as_ref().unwrap_or(&self.embed),
            last_row,
            self.threads,
        ))
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
            head: None,
            context: 0,
            blocks: Vec::new(),
            norm: Vec::new(),
            executor: None,
            window: None,
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

    /// Um modelo minúsculo em disco (V=4, W=4, 2 cabeças, 1 camada) para exercitar a carga.
    fn tiny_model(name: &str, config_extra: &str, head: Option<f32>) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aic-llama-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.json"),
            format!(
                r#"{{"hidden_size":4,"num_hidden_layers":1,"num_attention_heads":2,"num_key_value_heads":2,"intermediate_size":4,"max_position_embeddings":16,"hidden_act":"silu"{config_extra}}}"#
            ),
        )
        .unwrap();
        let mut tensors: Vec<(&str, Vec<usize>)> = vec![
            ("model.embed_tokens.weight", vec![4, 4]),
            ("model.layers.0.input_layernorm.weight", vec![4]),
            ("model.layers.0.self_attn.q_proj.weight", vec![4, 4]),
            ("model.layers.0.self_attn.k_proj.weight", vec![4, 4]),
            ("model.layers.0.self_attn.v_proj.weight", vec![4, 4]),
            ("model.layers.0.self_attn.o_proj.weight", vec![4, 4]),
            ("model.layers.0.post_attention_layernorm.weight", vec![4]),
            ("model.layers.0.mlp.gate_proj.weight", vec![4, 4]),
            ("model.layers.0.mlp.up_proj.weight", vec![4, 4]),
            ("model.layers.0.mlp.down_proj.weight", vec![4, 4]),
            ("model.norm.weight", vec![4]),
        ];
        if config_extra.contains("qwen2") {
            tensors.push(("model.layers.0.self_attn.q_proj.bias", vec![4]));
            tensors.push(("model.layers.0.self_attn.k_proj.bias", vec![4]));
            tensors.push(("model.layers.0.self_attn.v_proj.bias", vec![4]));
        }
        if config_extra.contains("oproj") {
            tensors.push(("model.layers.0.self_attn.o_proj.bias", vec![4]));
        }
        if head.is_some() {
            tensors.push(("lm_head.weight", vec![4, 4]));
        }
        let (mut header, mut data, mut offset) = (String::new(), Vec::<u8>::new(), 0usize);
        for (index, (tensor, shape)) in tensors.iter().enumerate() {
            let count: usize = shape.iter().product();
            let values: Vec<f32> = (0..count)
                .map(|i| {
                    if *tensor == "lm_head.weight" {
                        head.unwrap() * (1.0 + i as f32 * 0.1)
                    } else if shape.len() == 1 {
                        1.0
                    } else {
                        ((i * 7 + index * 3) % 11) as f32 * 0.1 - 0.5
                    }
                })
                .collect();
            for value in values {
                data.extend(value.to_le_bytes());
            }
            let end = offset + count * 4;
            header.push_str(&format!(
                r#"{}"{tensor}":{{"dtype":"F32","shape":{shape:?},"data_offsets":[{offset},{end}]}}"#,
                if header.is_empty() { "" } else { "," }
            ));
            offset = end;
        }
        let header = format!("{{{header}}}");
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend(header.as_bytes());
        bytes.extend(data);
        std::fs::write(dir.join("model.safetensors"), bytes).unwrap();
        dir
    }

    #[test]
    fn a_saida_nao_amarrada_usa_o_lm_head_e_a_amarrada_nao_o_pede() {
        let tied = tiny_model("tied", r#","tie_word_embeddings":true"#, None);
        let tied_logits = Llama::load(&tied)
            .unwrap()
            .next_token_logits(&[1, 2])
            .unwrap();

        // Sem a chave o padrão é não amarrar: o lm_head é obrigatório.
        let missing = tiny_model("nohead", "", None);
        let error = Llama::load(&missing).err().expect("falta o lm_head");
        assert!(error.contains("lm_head.weight"), "{error}");

        let untied = tiny_model("untied", r#","tie_word_embeddings":false"#, Some(0.5));
        let model = Llama::load(&untied).unwrap();
        let untied_logits = model.next_token_logits(&[1, 2]).unwrap();
        assert_eq!(untied_logits.len(), 4);
        assert_ne!(
            untied_logits, tied_logits,
            "a saída vem do lm_head, não dos embeddings"
        );
        for dir in [tied, missing, untied] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn o_vies_de_qkv_do_qwen_muda_a_saida_e_o_de_o_proj_e_recusado() {
        let plain = tiny_model("plain-b", r#","tie_word_embeddings":true"#, None);
        let biased = tiny_model(
            "qwen-b",
            r#","model_type":"qwen2","tie_word_embeddings":true"#,
            None,
        );
        let logits = |dir: &std::path::Path| {
            Llama::load(dir)
                .unwrap()
                .next_token_logits(&[1, 2])
                .unwrap()
        };
        assert_ne!(logits(&plain), logits(&biased), "o viés entra na conta");

        let output = tiny_model("oproj-b", r#","tie_word_embeddings":true,"oproj":1"#, None);
        let error = Llama::load(&output).err().expect("viés de o_proj recusado");
        assert!(error.contains("o_proj"), "{error}");
        for dir in [plain, biased, output] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn a_janela_deslizante_limita_o_que_cada_token_ve() {
        let open = tiny_model("win-off", r#","tie_word_embeddings":true"#, None);
        let windowed = tiny_model(
            "win-on",
            r#","tie_word_embeddings":true,"sliding_window":1"#,
            None,
        );
        let logits = |dir: &std::path::Path, ids: &[u32]| {
            Llama::load(dir).unwrap().next_token_logits(ids).unwrap()
        };
        // Janela 1: o último token só vê a si mesmo, então o que veio antes não importa.
        assert_eq!(logits(&windowed, &[1, 3]), logits(&windowed, &[2, 3]));
        // Sem janela o passado conta.
        assert_ne!(logits(&open, &[1, 3]), logits(&open, &[2, 3]));
        for dir in [open, windowed] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn rope_scaling_default_e_linear_e_llama3() {
        let config = |scaling: &str| serde_json::from_str::<serde_json::Value>(scaling).unwrap();
        let base = rope_frequencies(&config(r#"{}"#), 500_000.0, 128).unwrap();
        assert_eq!(base.len(), 64);
        assert_eq!(base[0], 1.0);

        let linear = rope_frequencies(
            &config(r#"{"rope_scaling":{"type":"linear","factor":4.0}}"#),
            500_000.0,
            128,
        )
        .unwrap();
        assert!(
            base.iter()
                .zip(&linear)
                .all(|(b, l)| (b / 4.0 - l).abs() < 1e-18)
        );

        // Llama 3.1 8B: fator 8, low 1, high 4, contexto original 8192.
        let llama3 = rope_frequencies(
            &config(
                r#"{"rope_scaling":{"rope_type":"llama3","factor":8.0,"low_freq_factor":1.0,"high_freq_factor":4.0,"original_max_position_embeddings":8192}}"#,
            ),
            500_000.0,
            128,
        )
        .unwrap();
        assert_eq!(llama3[0], base[0], "a frequência mais alta não muda");
        assert!(
            (llama3[63] - base[63] / 8.0).abs() < 1e-18,
            "a mais baixa é dividida pelo fator"
        );
        // Nunca passa do original nem cai abaixo da dividida, e a razão só cresce com a frequência.
        let mut ratios = Vec::new();
        for (scaled, original) in llama3.iter().zip(&base) {
            assert!(*scaled <= original + 1e-18 && *scaled >= original / 8.0 - 1e-18);
            ratios.push(scaled / original);
        }
        assert!(
            ratios.windows(2).all(|w| w[0] >= w[1] - 1e-12),
            "{ratios:?}"
        );
        assert!(
            ratios.iter().any(|r| *r > 0.125 + 1e-6 && *r < 1.0 - 1e-6),
            "faixa do meio suavizada"
        );

        for bad in [
            r#"{"rope_scaling":{"type":"yarn","factor":2.0}}"#,
            r#"{"rope_scaling":{"type":"linear"}}"#,
            r#"{"rope_scaling":{"rope_type":"llama3","factor":8.0,"low_freq_factor":4.0,"high_freq_factor":1.0,"original_max_position_embeddings":8192}}"#,
        ] {
            assert!(
                rope_frequencies(&config(bad), 10_000.0, 8).is_err(),
                "{bad}"
            );
        }
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
