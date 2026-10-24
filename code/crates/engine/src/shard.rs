//! Weight slices that live somewhere else (Fase J5).
//!
//! A projection `[out, in]` is cut by output rows into slices; each slice sits resident on one
//! node and answers `x * slice^T`. The engine only sees the [`Executor`] trait, so the model code
//! is the same whether the slices are local files ([`LocalExecutor`]) or workers behind the
//! controller. [`ShardStore`] is what a node runs: it loads a slice from its own disk on first
//! use and keeps it, so weights never travel.

use crate::safetensors::read_rows;
use ai_cluster_common::matrix::Matrix;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Largest slice and largest output one computation may hold in memory.
const MAX_SLICE_BYTES: usize = 256 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

/// Name of the weights file inside a model directory. Fixed, never taken from a caller.
const MODEL_FILE: &str = "model.safetensors";

/// Rows `row_start..row_end` of `tensor`, to be computed by `node`.
#[derive(Clone, Debug, PartialEq)]
pub struct Slice {
    pub node: String,
    pub tensor: String,
    pub row_start: u32,
    pub row_end: u32,
}

/// Runs `x * slice^T` for each slice, returning one `[x.rows, slice rows]` matrix per slice in
/// the same order.
pub trait Executor: Send + Sync {
    fn matmul(&self, x: &Matrix, slices: &[Slice]) -> Result<Vec<Matrix>, String>;
}

type Key = (String, String, u32, u32);

/// Slices resident on this node, loaded lazily from `root/<subdir>/model.safetensors`.
pub struct ShardStore {
    root: PathBuf,
    cap_bytes: usize,
    threads: usize,
    /// Loaded slices (already transposed to `[in, rows]`) and the bytes they hold.
    cache: Mutex<(HashMap<Key, Arc<Matrix>>, usize)>,
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
    ) -> Result<Arc<Matrix>, String> {
        // "m", "m/" and "m//" are one directory: the key uses the components, not the raw string,
        // so spelling variants cannot fill the cache with copies of the same slice.
        let normal: Vec<_> = Path::new(subdir).components().collect();
        let subdir = normal
            .iter()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let key = (subdir.clone(), tensor.to_string(), row_start, row_end);
        let lookup = |cache: &(HashMap<Key, Arc<Matrix>>, usize)| cache.0.get(&key).cloned();
        let lock = || {
            self.cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        };
        if let Some(found) = lookup(&lock()) {
            return Ok(found);
        }
        let path = self.model_path(&subdir)?;
        let _load = self
            .loading
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Whoever held the gate may have loaded exactly this slice.
        if let Some(found) = lookup(&lock()) {
            return Ok(found);
        }
        let room = self.cap_bytes.saturating_sub(lock().1).min(MAX_SLICE_BYTES);
        let rows = read_rows(&path, tensor, row_start as usize, row_end as usize, room)?;
        let loaded = Arc::new(transposed(&rows));
        drop(rows);
        let mut cache = lock();
        let bytes = loaded.data.len() * 8;
        if cache.1 + bytes > self.cap_bytes {
            return Err("memória de fatias esgotada neste nó".into());
        }
        cache.1 += bytes;
        cache.0.insert(key, loaded.clone());
        Ok(loaded)
    }

    /// `x * slice^T` for rows `row_start..row_end` of `tensor`.
    pub fn compute(
        &self,
        subdir: &str,
        tensor: &str,
        row_start: u32,
        row_end: u32,
        x: &Matrix,
    ) -> Result<Matrix, String> {
        let weights = self.slice(subdir, tensor, row_start, row_end)?;
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
                self.store
                    .compute(&self.subdir, &s.tensor, s.row_start, s.row_end, x)
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
        let header = r#"{"w":{"dtype":"F32","shape":[3,2],"data_offsets":[0,24]}}"#;
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
        let y = store.compute("m", "w", 1, 3, &x).unwrap();
        assert_eq!((y.rows, y.cols, y.data), (1, 2, vec![7.0, 11.0]));
        // Wrong input width is an error, not a panic.
        let wrong = Matrix {
            rows: 1,
            cols: 3,
            data: vec![1.0; 3],
        };
        assert!(store.compute("m", "w", 1, 3, &wrong).is_err());
        std::fs::remove_dir_all(root).ok();
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
            assert!(store.compute(bad, "w", 0, 1, &x).is_err(), "{bad:?}");
        }
        assert!(store.compute("absent", "w", 0, 1, &x).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_memory_cap_is_enforced() {
        let root = model_dir("cap");
        // One row of 2 f64 is 16 bytes; a 24-byte cap holds one slice and refuses the next.
        let store = ShardStore::new(root.clone(), 24, 1);
        let x = Matrix {
            rows: 1,
            cols: 2,
            data: vec![1.0, 1.0],
        };
        assert!(store.compute("m", "w", 0, 1, &x).is_ok());
        assert!(store.compute("m", "w", 1, 2, &x).is_err());
        // The resident slice still answers.
        assert!(store.compute("m", "w", 0, 1, &x).is_ok());
        std::fs::remove_dir_all(root).ok();
    }
}
