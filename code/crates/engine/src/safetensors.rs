//! Minimal safetensors reader: an 8-byte little-endian header length, a JSON header mapping each
//! tensor to its dtype, shape and byte range, then the raw data. Only `F32` is read; anything
//! else is an error rather than a silent reinterpretation.

use ai_cluster_common::matrix::Matrix;
use std::collections::HashMap;
use std::path::Path;

struct Entry {
    shape: Vec<usize>,
    start: usize,
    end: usize,
}

pub struct Tensors {
    bytes: Vec<u8>,
    /// Offset of the data section in `bytes`.
    base: usize,
    index: HashMap<String, Entry>,
}

impl Tensors {
    pub fn open(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let prefix: [u8; 8] = bytes
            .get(..8)
            .and_then(|slice| slice.try_into().ok())
            .ok_or("arquivo menor que o cabeçalho")?;
        let header_len = usize::try_from(u64::from_le_bytes(prefix)).map_err(|e| e.to_string())?;
        let base = 8usize
            .checked_add(header_len)
            .filter(|end| *end <= bytes.len())
            .ok_or("tamanho de cabeçalho inválido")?;
        let header: serde_json::Value =
            serde_json::from_slice(&bytes[8..base]).map_err(|err| format!("cabeçalho: {err}"))?;
        let header = header.as_object().ok_or("cabeçalho não é um objeto")?;
        let data_len = bytes.len() - base;

        let mut index = HashMap::new();
        for (name, meta) in header {
            if name == "__metadata__" {
                continue;
            }
            let dtype = meta["dtype"].as_str().unwrap_or_default();
            if dtype != "F32" {
                return Err(format!("{name}: dtype {dtype} não suportado (só F32)"));
            }
            let shape: Vec<usize> = meta["shape"]
                .as_array()
                .ok_or_else(|| format!("{name}: sem shape"))?
                .iter()
                .map(|dim| dim.as_u64().map(|d| d as usize))
                .collect::<Option<_>>()
                .ok_or_else(|| format!("{name}: shape inválido"))?;
            let offsets = meta["data_offsets"].as_array().map(Vec::as_slice);
            let (Some(start), Some(end)) = (
                offsets.and_then(|o| o.first()).and_then(|v| v.as_u64()),
                offsets.and_then(|o| o.get(1)).and_then(|v| v.as_u64()),
            ) else {
                return Err(format!("{name}: data_offsets inválido"));
            };
            let (start, end) = (start as usize, end as usize);
            let elements = shape.iter().try_fold(1usize, |a, d| a.checked_mul(*d));
            if start > end
                || end > data_len
                || elements.and_then(|n| n.checked_mul(4)) != Some(end - start)
            {
                return Err(format!("{name}: intervalo de dados incoerente com o shape"));
            }
            index.insert(name.clone(), Entry { shape, start, end });
        }
        Ok(Self { bytes, base, index })
    }

    pub fn shape(&self, name: &str) -> Option<&[usize]> {
        self.index.get(name).map(|entry| entry.shape.as_slice())
    }

    fn floats(&self, name: &str) -> Result<(&Entry, impl Iterator<Item = f64> + '_), String> {
        let entry = self
            .index
            .get(name)
            .ok_or_else(|| format!("tensor ausente: {name}"))?;
        let raw = &self.bytes[self.base + entry.start..self.base + entry.end];
        let values = raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b) as f64);
        Ok((entry, values))
    }

    /// A 2-D tensor as a row-major matrix.
    pub fn matrix(&self, name: &str) -> Result<Matrix, String> {
        let (entry, values) = self.floats(name)?;
        let [rows, cols] = entry.shape[..] else {
            return Err(format!(
                "{name}: esperava 2 dimensões, tem {}",
                entry.shape.len()
            ));
        };
        let rows = u32::try_from(rows).map_err(|e| e.to_string())?;
        let cols = u32::try_from(cols).map_err(|e| e.to_string())?;
        Ok(Matrix {
            rows,
            cols,
            data: values.collect(),
        })
    }

    /// A 1-D tensor.
    pub fn vector(&self, name: &str) -> Result<Vec<f64>, String> {
        let (entry, values) = self.floats(name)?;
        if entry.shape.len() != 1 {
            return Err(format!(
                "{name}: esperava 1 dimensão, tem {}",
                entry.shape.len()
            ));
        }
        Ok(values.collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(header: &str, data: &[f32]) -> Vec<u8> {
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend(header.as_bytes());
        for value in data {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn reads_a_matrix_and_a_vector() {
        let header = r#"{"__metadata__":{"format":"pt"},"w":{"dtype":"F32","shape":[2,2],"data_offsets":[0,16]},"b":{"dtype":"F32","shape":[2],"data_offsets":[16,24]}}"#;
        let tensors = Tensors::from_bytes(file(header, &[1.0, 2.0, 3.0, 4.0, 0.5, -0.5])).unwrap();
        let w = tensors.matrix("w").unwrap();
        assert_eq!((w.rows, w.cols, w.data), (2, 2, vec![1.0, 2.0, 3.0, 4.0]));
        assert_eq!(tensors.vector("b").unwrap(), vec![0.5, -0.5]);
        assert_eq!(tensors.shape("w"), Some(&[2usize, 2][..]));
        assert!(tensors.matrix("b").is_err(), "um vetor não é uma matriz");
        assert!(tensors.vector("ghost").is_err());
    }

    #[test]
    fn rejects_malformed_files() {
        assert!(Tensors::from_bytes(vec![1, 2, 3]).is_err());
        // Header length past the end of the file.
        assert!(Tensors::from_bytes(100u64.to_le_bytes().to_vec()).is_err());
        let wrong_dtype = r#"{"w":{"dtype":"F16","shape":[1],"data_offsets":[0,2]}}"#;
        assert!(Tensors::from_bytes(file(wrong_dtype, &[0.0])).is_err());
        let lying_shape = r#"{"w":{"dtype":"F32","shape":[3],"data_offsets":[0,8]}}"#;
        assert!(Tensors::from_bytes(file(lying_shape, &[0.0, 0.0])).is_err());
        let past_end = r#"{"w":{"dtype":"F32","shape":[4],"data_offsets":[0,16]}}"#;
        assert!(Tensors::from_bytes(file(past_end, &[0.0])).is_err());
    }
}
