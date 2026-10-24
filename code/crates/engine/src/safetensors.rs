//! Minimal safetensors reader: an 8-byte little-endian header length, a JSON header mapping each
//! tensor to its dtype, shape and byte range, then the raw data. `F32` and `BF16` are read (the
//! latter widens exactly: it is the top half of an f32); anything else is an error rather than a
//! silent reinterpretation.

use ai_cluster_common::matrix::Matrix;
use std::collections::HashMap;
use std::path::Path;

struct Entry {
    bf16: bool,
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
        let index = parse_header(&bytes[8..base], bytes.len() - base)?;
        Ok(Self { bytes, base, index })
    }

    pub fn shape(&self, name: &str) -> Option<&[usize]> {
        self.index.get(name).map(|entry| entry.shape.as_slice())
    }

    fn floats(&self, name: &str) -> Result<(&Entry, Vec<f64>), String> {
        let entry = self
            .index
            .get(name)
            .ok_or_else(|| format!("tensor ausente: {name}"))?;
        let raw = &self.bytes[self.base + entry.start..self.base + entry.end];
        Ok((entry, decode(raw, entry.bf16)))
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
            data: values,
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
        Ok(values)
    }
}

/// The tensor index of a safetensors header, with every byte range checked against the shape and
/// against the `data_len` bytes that follow the header.
fn parse_header(header: &[u8], data_len: usize) -> Result<HashMap<String, Entry>, String> {
    let header: serde_json::Value =
        serde_json::from_slice(header).map_err(|err| format!("cabeçalho: {err}"))?;
    let header = header.as_object().ok_or("cabeçalho não é um objeto")?;
    let mut index = HashMap::new();
    for (name, meta) in header {
        if name == "__metadata__" {
            continue;
        }
        let dtype = meta["dtype"].as_str().unwrap_or_default();
        if dtype != "F32" && dtype != "BF16" {
            return Err(format!(
                "{name}: dtype {dtype} não suportado (só F32 e BF16)"
            ));
        }
        let bf16 = dtype == "BF16";
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
            || elements.and_then(|n| n.checked_mul(if bf16 { 2 } else { 4 })) != Some(end - start)
        {
            return Err(format!("{name}: intervalo de dados incoerente com o shape"));
        }
        index.insert(
            name.clone(),
            Entry {
                bf16,
                shape,
                start,
                end,
            },
        );
    }
    Ok(index)
}

fn decode(raw: &[u8], bf16: bool) -> Vec<f64> {
    if bf16 {
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|b| f32::from_bits(u32::from(u16::from_le_bytes(*b)) << 16) as f64)
            .collect()
    } else {
        raw.as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b) as f64)
            .collect()
    }
}

/// Largest header `read_band` accepts; real ones are a few hundred KB.
const MAX_HEADER_BYTES: u64 = 16 * 1024 * 1024;

/// Rows `start..end` of a 2-D tensor (or, with `by_columns`, columns `start..end` of every row),
/// read straight from the file: the header and those
/// rows' bytes only, never the rest of the (possibly huge) file. `max_bytes` bounds the widened
/// f64 result, checked before anything is allocated.
pub fn read_band(
    path: &Path,
    name: &str,
    start: usize,
    end: usize,
    by_columns: bool,
    max_bytes: usize,
) -> Result<Matrix, String> {
    use std::io::{Read, Seek, SeekFrom};
    // No path in the message: it travels back to the caller, who must not learn the disk layout.
    let context = |err: std::io::Error| format!("leitura do modelo: {err}");
    let mut file = std::fs::File::open(path).map_err(context)?;
    let file_len = file.metadata().map_err(context)?.len();
    let mut prefix = [0u8; 8];
    file.read_exact(&mut prefix).map_err(context)?;
    let header_len = u64::from_le_bytes(prefix);
    if header_len > MAX_HEADER_BYTES || header_len + 8 > file_len {
        return Err("tamanho de cabeçalho inválido".into());
    }
    let mut header = vec![0u8; header_len as usize];
    file.read_exact(&mut header).map_err(context)?;
    let base = 8 + header_len;
    let index = parse_header(&header, (file_len - base) as usize)?;
    let entry = index
        .get(name)
        .ok_or_else(|| format!("tensor ausente: {name}"))?;
    let [rows, cols] = entry.shape[..] else {
        return Err(format!(
            "{name}: esperava 2 dimensões, tem {}",
            entry.shape.len()
        ));
    };
    let extent = if by_columns { cols } else { rows };
    if start >= end || end > extent {
        return Err(format!("{name}: faixa {start}..{end} fora de 0..{extent}"));
    }
    let count = end - start;
    let (out_rows, out_cols) = if by_columns {
        (rows, count)
    } else {
        (count, cols)
    };
    let element = if entry.bf16 { 2 } else { 4 };
    // Checked against the f64 size the caller will hold, which is the larger of the two.
    if out_rows
        .checked_mul(out_cols)
        .and_then(|n| n.checked_mul(8))
        .is_none_or(|n| n > max_bytes)
    {
        return Err(format!("{name}: fatia acima de {max_bytes} bytes"));
    }
    let origin = base + entry.start as u64;
    let mut raw = vec![0u8; out_rows * out_cols * element];
    if by_columns {
        // One short read per row; done once per slice, when it becomes resident.
        for (r, line) in raw.chunks_exact_mut(count * element).enumerate() {
            let offset = origin + ((r * cols + start) * element) as u64;
            file.seek(SeekFrom::Start(offset)).map_err(context)?;
            file.read_exact(line).map_err(context)?;
        }
    } else {
        file.seek(SeekFrom::Start(origin + (start * cols * element) as u64))
            .map_err(context)?;
        file.read_exact(&mut raw).map_err(context)?;
    }
    Ok(Matrix {
        rows: u32::try_from(out_rows).map_err(|e| e.to_string())?,
        cols: u32::try_from(out_cols).map_err(|e| e.to_string())?,
        data: decode(&raw, entry.bf16),
    })
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
    fn read_band_reads_only_the_requested_rows_or_columns() {
        let header = r#"{"w":{"dtype":"F32","shape":[3,2],"data_offsets":[0,24]}}"#;
        let path =
            std::env::temp_dir().join(format!("aic-rows-{}.safetensors", std::process::id()));
        std::fs::write(&path, file(header, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])).unwrap();
        let mid = read_band(&path, "w", 1, 3, false, 1024).unwrap();
        assert_eq!(
            (mid.rows, mid.cols, mid.data),
            (2, 2, vec![3.0, 4.0, 5.0, 6.0])
        );
        assert!(
            read_band(&path, "w", 2, 2, false, 1024).is_err(),
            "fatia vazia"
        );
        assert!(
            read_band(&path, "w", 0, 4, false, 1024).is_err(),
            "além das linhas"
        );
        assert!(
            read_band(&path, "w", 0, 3, false, 16).is_err(),
            "acima do teto"
        );
        assert!(read_band(&path, "ghost", 0, 1, false, 1024).is_err());
        // Column 1 of [[1,2],[3,4],[5,6]] is [2,4,6], laid out as a 3x1 matrix.
        let col = read_band(&path, "w", 1, 2, true, 1024).unwrap();
        assert_eq!((col.rows, col.cols, col.data), (3, 1, vec![2.0, 4.0, 6.0]));
        assert!(
            read_band(&path, "w", 0, 3, true, 1024).is_err(),
            "além das colunas"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn widens_bf16_exactly() {
        // 1.0 is 0x3F80 and -2.5 is 0xC020 in bfloat16.
        let header = r#"{"w":{"dtype":"BF16","shape":[2],"data_offsets":[0,4]}}"#;
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend(header.as_bytes());
        bytes.extend([0x80, 0x3F, 0x20, 0xC0]);
        let tensors = Tensors::from_bytes(bytes).unwrap();
        assert_eq!(tensors.vector("w").unwrap(), vec![1.0, -2.5]);
    }

    #[test]
    fn rejects_malformed_files() {
        assert!(Tensors::from_bytes(vec![1, 2, 3]).is_err());
        // Header length past the end of the file.
        assert!(Tensors::from_bytes(100u64.to_le_bytes().to_vec()).is_err());
        let wrong_dtype = r#"{"w":{"dtype":"F64","shape":[1],"data_offsets":[0,2]}}"#;
        assert!(Tensors::from_bytes(file(wrong_dtype, &[0.0])).is_err());
        let lying_shape = r#"{"w":{"dtype":"F32","shape":[3],"data_offsets":[0,8]}}"#;
        assert!(Tensors::from_bytes(file(lying_shape, &[0.0, 0.0])).is_err());
        let past_end = r#"{"w":{"dtype":"F32","shape":[4],"data_offsets":[0,16]}}"#;
        assert!(Tensors::from_bytes(file(past_end, &[0.0])).is_err());
    }
}
