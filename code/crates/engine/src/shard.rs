//! Weight slices that live somewhere else (Fase J5).
//!
//! A projection `[out, in]` is cut by output rows into slices; each slice sits resident on one
//! node and answers `x * slice^T`. The engine only sees the [`Executor`] trait, so the model code
//! is the same whether the slices are local files ([`LocalExecutor`]) or workers behind the
//! controller. [`ShardStore`] is what a node runs: it loads a slice from its own disk on first
//! use and keeps it, so weights never travel.

use crate::safetensors::read_band;
use ai_cluster_common::matrix::Matrix;
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Largest slice and largest output one computation may hold in memory.
const MAX_SLICE_BYTES: usize = 256 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

/// Name of the weights file inside a model directory. Fixed, never taken from a caller.
const MODEL_FILE: &str = "model.safetensors";

/// Rows `row_start..row_end` of `tensor` (an `[out, in]` projection), or with `by_columns` the
/// columns of an `[in, out]` one (GPT-2's `Conv1D`), to be computed by `node`.
#[derive(Clone, Debug, PartialEq)]
pub struct Slice {
    pub node: String,
    pub tensor: String,
    pub row_start: u32,
    pub row_end: u32,
    pub by_columns: bool,
}

/// A projection: held here as `[in, out]`, or cut into slices that live on other nodes.
pub(crate) enum Weight {
    Local(Matrix),
    /// Slices in output order; their outputs concatenate by columns. The second field is how many
    /// elements one output row (or column) of a slice holds, to size what a node keeps resident.
    Remote(Vec<Slice>, usize),
}

impl Weight {
    /// `extent` output rows (or columns) cut into equal bands, one per node, in node order; each
    /// output row (or column) holds `width` elements.
    pub(crate) fn split(
        nodes: &[String],
        tensor: &str,
        extent: usize,
        width: usize,
        by_columns: bool,
    ) -> Result<Self, String> {
        if extent < nodes.len() {
            return Err(format!(
                "{tensor}: {extent} linhas para {} nós",
                nodes.len()
            ));
        }
        // A faixa de um nó vira uma fatia residente de uma vez só: acima do teto o worker a recusaria
        // na primeira chamada, então o modelo é recusado já na carga.
        let widest = extent.div_ceil(nodes.len()) as u64 * width as u64 * size_of::<f64>() as u64;
        if widest > MAX_SLICE_BYTES as u64 {
            return Err(format!(
                "{tensor}: a faixa de cada nó teria {} MiB, acima do teto de {} MiB por fatia; use mais nós",
                widest >> 20,
                MAX_SLICE_BYTES >> 20
            ));
        }
        let bounds = |i: usize| (i * extent / nodes.len()) as u32;
        Ok(Self::Remote(
            nodes
                .iter()
                .enumerate()
                .map(|(i, node)| Slice {
                    node: node.clone(),
                    tensor: tensor.to_string(),
                    row_start: bounds(i),
                    row_end: bounds(i + 1),
                    by_columns,
                })
                .collect(),
            width,
        ))
    }
}

/// What each node keeps resident for these weights: `f64` per element, as the worker stores a
/// slice. A node that holds bands of several weights adds them up.
pub(crate) fn resident_bytes<'a>(
    weights: impl IntoIterator<Item = &'a Weight>,
) -> BTreeMap<String, u64> {
    let mut need = BTreeMap::new();
    for weight in weights {
        if let Weight::Remote(parts, width) = weight {
            for part in parts {
                let rows = u64::from(part.row_end - part.row_start);
                *need.entry(part.node.clone()).or_insert(0) +=
                    rows * *width as u64 * size_of::<f64>() as u64;
            }
        }
    }
    need
}

/// `x * w` for each weight, in order. Every remote weight goes out in one executor call, so
/// projections that share an input (q, k, v; gate, up) cost one round trip.
pub(crate) fn apply(
    executor: Option<&Arc<dyn Executor>>,
    threads: usize,
    x: &Matrix,
    weights: &[&Weight],
) -> Result<Vec<Matrix>, String> {
    let slices: Vec<Slice> = weights
        .iter()
        .filter_map(|w| match w {
            Weight::Remote(parts, _) => Some(parts.iter().cloned()),
            Weight::Local(_) => None,
        })
        .flatten()
        .collect();
    let mut remote = if slices.is_empty() {
        Vec::new().into_iter()
    } else {
        let executor = executor.ok_or("fatias remotas sem executor")?;
        let outputs = executor.matmul(x, &slices)?;
        if outputs.len() != slices.len() {
            return Err("o executor devolveu um número errado de saídas".into());
        }
        outputs.into_iter()
    };
    weights
        .iter()
        .map(|weight| match weight {
            Weight::Local(w) => x
                .multiply_with_threads(w, threads)
                .ok_or_else(|| "formas incompatíveis na multiplicação".to_string()),
            Weight::Remote(parts, _) => {
                let pieces: Vec<Matrix> = remote.by_ref().take(parts.len()).collect();
                join_columns(&pieces, x.rows, parts)
            }
        })
        .collect()
}

/// Concatenates the per-slice outputs of one projection side by side.
fn join_columns(pieces: &[Matrix], rows: u32, parts: &[Slice]) -> Result<Matrix, String> {
    if pieces.len() != parts.len() {
        return Err("faltam saídas de fatias".into());
    }
    let mut width = 0u32;
    for (piece, part) in pieces.iter().zip(parts) {
        if piece.rows != rows || piece.cols != part.row_end - part.row_start {
            return Err(format!("{}: saída com forma inesperada", part.tensor));
        }
        width += piece.cols;
    }
    let mut data = Vec::with_capacity(rows as usize * width as usize);
    for r in 0..rows as usize {
        for piece in pieces {
            let w = piece.cols as usize;
            data.extend_from_slice(&piece.data[r * w..(r + 1) * w]);
        }
    }
    Ok(Matrix {
        rows,
        cols: width,
        data,
    })
}

/// Runs `x * slice^T` for each slice, returning one `[x.rows, slice rows]` matrix per slice in
/// the same order.
pub trait Executor: Send + Sync {
    fn matmul(&self, x: &Matrix, slices: &[Slice]) -> Result<Vec<Matrix>, String>;
}

type Key = (String, String, u32, u32, bool);

/// Resident slices with the tick of their last use, and the bytes they hold.
type Cache = (HashMap<Key, (Arc<Matrix>, u64)>, usize);

/// Slices resident on this node, loaded lazily from `root/<subdir>/model.safetensors`.
pub struct ShardStore {
    root: PathBuf,
    cap_bytes: usize,
    threads: usize,
    /// Loaded slices (already transposed to `[in, rows]`); the least recently used make room.
    cache: Mutex<Cache>,
    clock: AtomicU64,
    /// One disk load at a time: a load briefly holds the raw rows, their f64 form and the
    /// transposed copy, and concurrent loads of the same slice would each do all of that.
    loading: Mutex<()>,
}

impl ShardStore {
    pub fn new(root: PathBuf, cap_bytes: usize, threads: usize) -> Self {
        Self {
            root,
            cap_bytes,
            threads,
            cache: Mutex::new((HashMap::new(), 0)),
            clock: AtomicU64::new(0),
            loading: Mutex::new(()),
        }
    }

    /// `root/subdir/model.safetensors`, refusing anything that could leave `root`: only plain
    /// path components are accepted, and the resolved path must still sit under the resolved
    /// root (a symlink or junction would pass the lexical check).
    fn model_path(&self, subdir: &str) -> Result<PathBuf, String> {
        let relative = Path::new(subdir);
        if subdir.is_empty()
            || !relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err("subdiretório de modelo inválido".into());
        }
        let root = self
            .root
            .canonicalize()
            .map_err(|e| format!("raiz de modelos: {e}"))?;
        // The file is resolved too: a symlink at the file itself would pass a check on the directory.
        let file = root
            .join(relative)
            .join(MODEL_FILE)
            .canonicalize()
            .map_err(|_| format!("modelo {subdir}: arquivo de pesos não encontrado"))?;
        if !file.starts_with(&root) {
            return Err("subdiretório de modelo fora da raiz".into());
        }
        Ok(file)
    }

    fn slice(
        &self,
        subdir: &str,
        tensor: &str,
        row_start: u32,
        row_end: u32,
        by_columns: bool,
    ) -> Result<Arc<Matrix>, String> {
        if !is_projection(tensor) {
            return Err("tensor não permitido: só projeções de camada saem do disco".into());
        }
        // "m", "m/" and "m//" are one directory: the key uses the components, not the raw string,
        // so spelling variants cannot fill the cache with copies of the same slice.
        let normal: Vec<_> = Path::new(subdir).components().collect();
        let subdir = normal
            .iter()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let key = (
            subdir.clone(),
            tensor.to_string(),
            row_start,
            row_end,
            by_columns,
        );
        let lookup = |cache: &mut Cache| {
            let tick = self.clock.fetch_add(1, Ordering::Relaxed);
            cache.0.get_mut(&key).map(|(found, used)| {
                *used = tick;
                found.clone()
            })
        };
        let lock = || {
            self.cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        };
        if let Some(found) = lookup(&mut lock()) {
            return Ok(found);
        }
        let path = self.model_path(&subdir)?;
        let _load = self
            .loading
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Whoever held the gate may have loaded exactly this slice.
        if let Some(found) = lookup(&mut lock()) {
            return Ok(found);
        }
        let room = self.cap_bytes.min(MAX_SLICE_BYTES);
        let band = read_band(
            &path,
            tensor,
            row_start as usize,
            row_end as usize,
            by_columns,
            room,
        )?;
        // Rows of `[out, in]` need the transpose; columns of `[in, out]` already are `[in, band]`.
        let loaded = Arc::new(if by_columns { band } else { transposed(&band) });
        let mut cache = lock();
        let bytes = loaded.data.len() * 8;
        if bytes > self.cap_bytes {
            return Err("fatia maior que a memória de fatias deste nó".into());
        }
        // Make room by dropping the least recently used slices; one still being multiplied lives
        // on in its caller's `Arc` and is merely loaded again on its next use.
        while cache.1 + bytes > self.cap_bytes {
            let Some(oldest) = cache
                .0
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some((evicted, _)) = cache.0.remove(&oldest) {
                cache.1 -= evicted.data.len() * 8;
            }
        }
        cache.1 += bytes;
        let tick = self.clock.fetch_add(1, Ordering::Relaxed);
        cache.0.insert(key, (loaded.clone(), tick));
        Ok(loaded)
    }

    /// `x * slice^T` for rows `row_start..row_end` of `tensor` (`x * slice` for columns).
    pub fn compute(
        &self,
        subdir: &str,
        tensor: &str,
        (row_start, row_end): (u32, u32),
        by_columns: bool,
        x: &Matrix,
    ) -> Result<Matrix, String> {
        let weights = self.slice(subdir, tensor, row_start, row_end, by_columns)?;
        if (x.rows as usize)
            .checked_mul(weights.cols as usize)
            .and_then(|n| n.checked_mul(8))
            .is_none_or(|n| n > MAX_OUTPUT_BYTES)
        {
            return Err(format!("{tensor}: saída acima de {MAX_OUTPUT_BYTES} bytes"));
        }
        x.multiply_with_threads(&weights, self.threads)
            .ok_or_else(|| {
                format!(
                    "{tensor}: entrada com {} colunas, a fatia espera {}",
                    x.cols, weights.rows
                )
            })
    }
}

/// The only tensors a slice request may read: the per-layer projections of GPT-2 and Llama
/// models. Embeddings, norms and anything else on the worker's disk stay out of reach.
pub fn is_projection(name: &str) -> bool {
    const SUFFIXES: [&str; 11] = [
        ".attn.c_attn.weight",
        ".attn.c_proj.weight",
        ".mlp.c_fc.weight",
        ".mlp.c_proj.weight",
        ".self_attn.q_proj.weight",
        ".self_attn.k_proj.weight",
        ".self_attn.v_proj.weight",
        ".self_attn.o_proj.weight",
        ".mlp.gate_proj.weight",
        ".mlp.up_proj.weight",
        ".mlp.down_proj.weight",
    ];
    SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

/// `[out, in]` -> `[in, out]`, the layout `x * w` needs.
pub(crate) fn transposed(m: &Matrix) -> Matrix {
    let (rows, cols) = (m.rows as usize, m.cols as usize);
    let mut data = vec![0.0; m.data.len()];
    for r in 0..rows {
        for c in 0..cols {
            data[c * rows + r] = m.data[r * cols + c];
        }
    }
    Matrix {
        rows: m.cols,
        cols: m.rows,
        data,
    }
}

/// The slices computed in this process, from local files. Used by tests to prove the sharded
/// forward equals the local one, and available as a fallback with no workers.
pub struct LocalExecutor {
    store: ShardStore,
    subdir: String,
}

impl LocalExecutor {
    pub fn new(root: PathBuf, subdir: &str, threads: usize) -> Self {
        Self {
            store: ShardStore::new(root, 4 << 30, threads),
            subdir: subdir.to_string(),
        }
    }
}

impl Executor for LocalExecutor {
    fn matmul(&self, x: &Matrix, slices: &[Slice]) -> Result<Vec<Matrix>, String> {
        slices
            .iter()
            .map(|s| {
                self.store.compute(
                    &self.subdir,
                    &s.tensor,
                    (s.row_start, s.row_end),
                    s.by_columns,
                    x,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_dir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("aic-shard-{}-{name}", std::process::id()));
        std::fs::create_dir_all(root.join("m")).unwrap();
        let header =
            r#"{"h.0.mlp.c_fc.weight":{"dtype":"F32","shape":[3,2],"data_offsets":[0,24]}}"#;
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend(header.as_bytes());
        for v in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0] {
            bytes.extend(v.to_le_bytes());
        }
        std::fs::write(root.join("m").join(MODEL_FILE), bytes).unwrap();
        root
    }

    #[test]
    fn a_slice_computes_x_times_its_rows_transposed() {
        let root = model_dir("compute");
        let store = ShardStore::new(root.clone(), 1 << 20, 1);
        // w = [[1,2],[3,4],[5,6]]; rows 1..3 are [[3,4],[5,6]]; x = [1, 1] -> [7, 11].
        let x = Matrix {
            rows: 1,
            cols: 2,
            data: vec![1.0, 1.0],
        };
        let y = store
            .compute("m", "h.0.mlp.c_fc.weight", (1, 3), false, &x)
            .unwrap();
        assert_eq!((y.rows, y.cols, y.data), (1, 2, vec![7.0, 11.0]));
        // Wrong input width is an error, not a panic.
        let wrong = Matrix {
            rows: 1,
            cols: 3,
            data: vec![1.0; 3],
        };
        assert!(
            store
                .compute("m", "h.0.mlp.c_fc.weight", (1, 3), false, &wrong)
                .is_err()
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn only_layer_projections_can_be_read() {
        let store = ShardStore::new(model_dir("allow"), 1 << 20, 1);
        let x = Matrix {
            rows: 1,
            cols: 2,
            data: vec![1.0, 1.0],
        };
        for name in [
            "model.embed_tokens.weight",
            "wte.weight",
            "h.0.ln_1.weight",
            "../x.weight",
        ] {
            let error = store.compute("m", name, (0, 1), false, &x).unwrap_err();
            assert!(error.contains("não permitido"), "{name}: {error}");
        }
    }

    #[test]
    fn a_column_slice_multiplies_without_a_transpose() {
        let root = model_dir("columns");
        let store = ShardStore::new(root.clone(), 1 << 20, 1);
        // w = [[1,2],[3,4],[5,6]] as [in=3, out=2]; column 1 is [2,4,6]; x = [1,1,1] -> [12].
        let x = Matrix {
            rows: 1,
            cols: 3,
            data: vec![1.0, 1.0, 1.0],
        };
        let y = store
            .compute("m", "h.0.mlp.c_fc.weight", (1, 2), true, &x)
            .unwrap();
        assert_eq!((y.rows, y.cols, y.data), (1, 1, vec![12.0]));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn resident_bytes_soma_o_que_cada_no_guarda() {
        // 10 linhas em 3 nós (3, 3, 4 pela divisão), 5 elementos por linha, duas projeções
        let nodes: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let w = Weight::split(&nodes, "t", 10, 5, false).unwrap();
        let need = resident_bytes([&w, &w]);
        assert_eq!(need["a"], 2 * 3 * 5 * 8);
        assert_eq!(need["b"], 2 * 3 * 5 * 8);
        assert_eq!(need["c"], 2 * 4 * 5 * 8);
        assert_eq!(need.values().sum::<u64>(), 2 * 10 * 5 * 8);
        assert!(
            resident_bytes([&Weight::Local(Matrix {
                rows: 1,
                cols: 1,
                data: vec![0.0]
            })])
            .is_empty()
        );
    }

    #[test]
    fn faixa_acima_do_teto_por_fatia_e_recusada_na_carga() {
        let nodes: Vec<String> = ["a", "b"].map(String::from).to_vec();
        // 8192 linhas x 8192 colunas x 8 bytes = 512 MiB no total, 256 MiB por nó: no teto
        assert!(Weight::split(&nodes, "t", 8192, 8192, false).is_ok());
        assert!(Weight::split(&nodes, "t", 8194, 8192, false).is_err());
    }

    #[test]
    fn slice_outputs_join_side_by_side_in_slice_order() {
        let part = |end: u32| Slice {
            node: "n".into(),
            tensor: "t".into(),
            row_start: 0,
            row_end: end,
            by_columns: false,
        };
        let a = Matrix {
            rows: 2,
            cols: 1,
            data: vec![1.0, 2.0],
        };
        let b = Matrix {
            rows: 2,
            cols: 2,
            data: vec![10.0, 11.0, 20.0, 21.0],
        };
        let joined = join_columns(&[a.clone(), b.clone()], 2, &[part(1), part(2)]).unwrap();
        assert_eq!((joined.rows, joined.cols), (2, 3));
        assert_eq!(joined.data, vec![1.0, 10.0, 11.0, 2.0, 20.0, 21.0]);
        // A piece with the wrong shape, or a missing one, is an error.
        assert!(join_columns(&[a.clone(), b.clone()], 2, &[part(2), part(2)]).is_err());
        assert!(join_columns(&[a], 2, &[part(1), part(2)]).is_err());
    }

    #[test]
    fn paths_cannot_leave_the_model_root() {
        let root = model_dir("paths");
        let store = ShardStore::new(root.clone(), 1 << 20, 1);
        let x = Matrix {
            rows: 1,
            cols: 2,
            data: vec![1.0, 1.0],
        };
        for bad in [
            "",
            "..",
            "../m",
            "m/..",
            "/etc",
            "m/../..",
            "C:\\x",
            "\\\\host\\share",
        ] {
            assert!(
                store
                    .compute(bad, "h.0.mlp.c_fc.weight", (0, 1), false, &x)
                    .is_err(),
                "{bad:?}"
            );
        }
        assert!(
            store
                .compute("absent", "h.0.mlp.c_fc.weight", (0, 1), false, &x)
                .is_err()
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_least_recently_used_slice_makes_room() {
        let root = model_dir("cap");
        // One row of 2 f64 is 16 bytes; a 24-byte cap holds one slice at a time.
        let store = ShardStore::new(root.clone(), 24, 1);
        let x = Matrix {
            rows: 1,
            cols: 2,
            data: vec![1.0, 1.0],
        };
        assert!(
            store
                .compute("m", "h.0.mlp.c_fc.weight", (0, 1), false, &x)
                .is_ok()
        );
        // The next slice evicts the first instead of failing, and the first loads again on demand.
        for range in [(1, 2), (0, 1), (1, 2)] {
            assert!(
                store
                    .compute("m", "h.0.mlp.c_fc.weight", range, false, &x)
                    .is_ok(),
                "{range:?}"
            );
        }
        assert_eq!(store.cache.lock().unwrap().0.len(), 1);
        // A slice that cannot fit even alone is refused.
        let tiny = ShardStore::new(root.clone(), 8, 1);
        assert!(
            tiny.compute("m", "h.0.mlp.c_fc.weight", (0, 1), false, &x)
                .is_err()
        );
        std::fs::remove_dir_all(root).ok();
    }
}
