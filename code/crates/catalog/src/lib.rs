//! Catálogo de modelos (ADR-022, E1): lê os cabeçalhos de GGUF e safetensors de uma pasta e diz, sem
//! carregar pesos, o que cada modelo é e se o motor atual o roda. Não depende do motor.

pub mod check;

use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Tetos de leitura: um cabeçalho malformado ou hostil não pode pedir memória sem limite.
const MAX_STRING: u64 = 1 << 20;
const MAX_KV: u64 = 1 << 20;
const MAX_TENSORS: u64 = 1 << 22;
const MAX_JSON_HEADER: u64 = 16 << 20;
const MAX_ENTRIES: usize = 4096;
const MAX_DIR_FILES: usize = 1024;
/// Teto de bytes lidos de um cabeçalho GGUF e de um config.json.
const MAX_GGUF_HEADER: u64 = 256 << 20;
const MAX_CONFIG: u64 = 1 << 20;
/// Partes de um GGUF dividido que o catálogo aceita somar.
const MAX_PARTS: u32 = 1024;

/// Um modelo encontrado na pasta.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelEntry {
    /// Nome para escolher o modelo: a pasta, ou `pasta/arquivo.gguf`.
    pub id: String,
    /// `gguf` ou `safetensors`.
    pub format: String,
    pub architecture: String,
    pub parameters: u64,
    /// Precisão dos pesos: `F32`, `BF16`, `Q4_K_M`...
    pub precision: String,
    /// Bytes em disco, somando as partes.
    pub bytes: u64,
    /// Contexto máximo em tokens; 0 = o cabeçalho não diz.
    pub context: u64,
    pub layers: u32,
    /// Largura do modelo (embedding), cabeças de atenção e cabeças de chave/valor; 0 = não consta.
    /// Servem à estimativa do cache de contexto (KV).
    pub hidden: u32,
    pub heads: u32,
    pub kv_heads: u32,
    /// O motor atual roda este modelo.
    pub supported: bool,
    /// Quando `supported` é falso, por quê.
    pub reason: String,
}

/// O que o motor instalado sabe rodar. Hoje o motor próprio lê safetensors de GPT-2 e Llama; o
/// llama.cpp (GGUF) entra quando a E-1 o aprovar.
#[derive(Debug, Clone, Copy, Default)]
pub struct Runtime {
    pub gguf: bool,
}

/// Varre `dir` (um nível) e devolve os modelos encontrados, em ordem de id. Entradas ilegíveis ou
/// malformadas entram com `supported = false` e o motivo, em vez de sumirem.
pub fn scan(dir: &Path, runtime: Runtime) -> std::io::Result<Vec<ModelEntry>> {
    let mut found = Vec::new();
    // Atalhos (symlinks) são ignorados: o catálogo só lê o que está dentro da pasta.
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .take(MAX_ENTRIES)
        .map(|entry| entry.path())
        .collect();
    names.sort();
    for path in names {
        let name = file_name(&path);
        if is_directory(&path) {
            found.extend(scan_dir(&path, &name, runtime));
        } else if is_first_gguf(&path) {
            found.push(gguf_entry(&path, name, runtime));
        }
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(found)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_kind(path: &Path) -> Option<std::fs::FileType> {
    std::fs::symlink_metadata(path).ok().map(|m| m.file_type())
}

fn is_regular(path: &Path) -> bool {
    file_kind(path).is_some_and(|t| t.is_file())
}

fn is_directory(path: &Path) -> bool {
    file_kind(path).is_some_and(|t| t.is_dir())
}

fn is_safetensors(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "safetensors")
}

fn scan_dir(dir: &Path, name: &str, runtime: Runtime) -> Vec<ModelEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .take(MAX_DIR_FILES)
        .map(|e| e.path())
        .filter(|p| is_regular(p))
        .collect();
    files.sort();
    let ggufs: Vec<&PathBuf> = files.iter().filter(|p| is_first_gguf(p)).collect();
    if !ggufs.is_empty() {
        return ggufs
            .into_iter()
            .map(|p| gguf_entry(p, format!("{name}/{}", file_name(p)), runtime))
            .collect();
    }
    if files.iter().any(|p| is_safetensors(p)) {
        return vec![safetensors_entry(dir, &files, name.to_string())];
    }
    Vec::new()
}

/// `modelo.gguf` ou a parte 1 de `modelo-00001-of-0000N.gguf`; as outras partes não viram entrada.
fn is_first_gguf(path: &Path) -> bool {
    let name = file_name(path);
    if !is_regular(path) || !name.ends_with(".gguf") {
        return false;
    }
    match split_part(&name) {
        Some((index, _)) => index == 1,
        None => true,
    }
}

/// `(parte, total)` de um nome `...-00002-of-00005.gguf`.
fn split_part(name: &str) -> Option<(u32, u32)> {
    let stem = name.strip_suffix(".gguf")?;
    let (head, total) = stem.rsplit_once("-of-")?;
    let (_, index) = head.rsplit_once('-')?;
    Some((index.parse().ok()?, total.parse().ok()?))
}

fn failed(id: String, format: &str, bytes: u64, reason: String) -> ModelEntry {
    ModelEntry {
        id,
        format: format.into(),
        bytes,
        supported: false,
        reason,
        ..ModelEntry::default()
    }
}

// ---------------------------------------------------------------- GGUF

fn gguf_entry(path: &Path, id: String, runtime: Runtime) -> ModelEntry {
    let bytes = match gguf_total_bytes(path) {
        Ok(bytes) => bytes,
        Err(error) => return failed(id, "gguf", 0, error),
    };
    match read_gguf(path) {
        Ok(info) => {
            let (supported, reason) = if runtime.gguf {
                (true, String::new())
            } else {
                (
                    false,
                    "GGUF exige o motor llama.cpp, que ainda não está ligado (ADR-022)".into(),
                )
            };
            ModelEntry {
                id,
                format: "gguf".into(),
                architecture: info.architecture,
                parameters: info.parameters,
                precision: info.precision,
                bytes,
                context: info.context,
                layers: info.layers,
                hidden: info.hidden,
                heads: info.heads,
                kv_heads: info.kv_heads,
                supported,
                reason,
            }
        }
        Err(error) => failed(
            id,
            "gguf",
            bytes,
            format!("cabeçalho GGUF inválido: {error}"),
        ),
    }
}

/// Soma as partes de um GGUF dividido; um arquivo único devolve o próprio tamanho. Parte ausente ou
/// total absurdo é erro: somar menos bytes faria o veredito dizer "cabe" quando não cabe.
fn gguf_total_bytes(path: &Path) -> Result<u64, String> {
    let size = |p: &Path| {
        std::fs::metadata(p)
            .map(|m| m.len())
            .map_err(|e| e.to_string())
    };
    let name = file_name(path);
    let (Some((_, total)), Some(parent)) = (split_part(&name), path.parent()) else {
        return size(path);
    };
    if total == 0 || total > MAX_PARTS {
        return Err(format!("GGUF dividido em {total} partes: fora do limite"));
    }
    let Some((stem, _)) = name
        .rsplit_once("-of-")
        .and_then(|(head, _)| head.rsplit_once('-'))
    else {
        return size(path);
    };
    let mut sum: u64 = 0;
    for part in 1..=total {
        let file = parent.join(format!("{stem}-{part:05}-of-{total:05}.gguf"));
        if !is_regular(&file) {
            return Err(format!("falta a parte {part} de {total} do GGUF"));
        }
        sum = sum.saturating_add(size(&file)?);
    }
    Ok(sum)
}

struct GgufInfo {
    architecture: String,
    parameters: u64,
    precision: String,
    context: u64,
    layers: u32,
    hidden: u32,
    heads: u32,
    kv_heads: u32,
}

fn read_gguf(path: &Path) -> Result<GgufInfo, String> {
    let mut r = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if &magic != b"GGUF" {
        return Err("falta a marca GGUF".into());
    }
    let version = u32le(&mut r)?;
    if !(2..=3).contains(&version) {
        return Err(format!("versão {version} não suportada"));
    }
    let tensors = u64le(&mut r)?;
    let kvs = u64le(&mut r)?;
    if tensors > MAX_TENSORS || kvs > MAX_KV {
        return Err("contagens fora do limite".into());
    }
    let mut numbers: HashMap<String, u64> = HashMap::new();
    let mut texts: HashMap<String, String> = HashMap::new();
    for _ in 0..kvs {
        within_budget(&mut r)?;
        let key = gguf_string(&mut r)?;
        let kind = u32le(&mut r)?;
        match kind {
            8 => {
                let text = gguf_string(&mut r)?;
                if key.starts_with("general.") {
                    texts.insert(key, text);
                }
            }
            9 => skip_array(&mut r)?,
            _ => {
                let value = scalar(&mut r, kind)?;
                if wanted_number(&key) {
                    numbers.insert(key, value);
                }
            }
        }
    }
    let mut parameters: u64 = 0;
    for _ in 0..tensors {
        within_budget(&mut r)?;
        let _name = gguf_string(&mut r)?;
        let dims = u32le(&mut r)?;
        if dims > 8 {
            return Err("tensor com dimensões demais".into());
        }
        let mut count: u64 = 1;
        for _ in 0..dims {
            count = count.saturating_mul(u64le(&mut r)?);
        }
        let _kind = u32le(&mut r)?;
        let _offset = u64le(&mut r)?;
        parameters = parameters.saturating_add(count);
    }
    let architecture = texts
        .get("general.architecture")
        .cloned()
        .unwrap_or_default();
    let per_arch = |suffix: &str| numbers.get(&format!("{architecture}.{suffix}")).copied();
    let precision = numbers
        .get("general.file_type")
        .map(|&t| file_type(t))
        .unwrap_or_else(|| "?".into());
    let small = |suffix: &str| per_arch(suffix).unwrap_or(0).min(u64::from(u32::MAX)) as u32;
    Ok(GgufInfo {
        context: per_arch("context_length").unwrap_or(0),
        hidden: small("embedding_length"),
        heads: small("attention.head_count"),
        kv_heads: small("attention.head_count_kv"),
        layers: per_arch("block_count")
            .unwrap_or(0)
            .min(u64::from(u32::MAX)) as u32,
        architecture,
        parameters,
        precision,
    })
}

/// Só estas chaves numéricas importam; guardar todas deixaria a memória crescer com o arquivo.
fn wanted_number(key: &str) -> bool {
    key == "general.file_type"
        || [
            ".context_length",
            ".block_count",
            ".embedding_length",
            ".attention.head_count",
            ".attention.head_count_kv",
        ]
        .iter()
        .any(|suffix| key.ends_with(suffix))
}

/// Falha se o cabeçalho já consumiu mais bytes do que o teto.
fn within_budget<R: Seek>(r: &mut R) -> Result<(), String> {
    match r.stream_position() {
        Ok(position) if position <= MAX_GGUF_HEADER => Ok(()),
        Ok(_) => Err("cabeçalho maior que o teto".into()),
        Err(error) => Err(error.to_string()),
    }
}

fn file_type(code: u64) -> String {
    match code {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        32 => "BF16",
        other => return format!("tipo {other}"),
    }
    .into()
}

fn u32le<R: Read>(r: &mut R) -> Result<u32, String> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(b))
}

fn u64le<R: Read>(r: &mut R) -> Result<u64, String> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u64::from_le_bytes(b))
}

fn gguf_string<R: Read>(r: &mut R) -> Result<String, String> {
    let len = u64le(r)?;
    if len > MAX_STRING {
        return Err("texto longo demais".into());
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Tamanho em bytes de um escalar GGUF; `None` para texto e matriz.
fn scalar_size(kind: u32) -> Option<u64> {
    match kind {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

fn scalar<R: Read>(r: &mut R, kind: u32) -> Result<u64, String> {
    let size = scalar_size(kind).ok_or_else(|| format!("tipo {kind} desconhecido"))?;
    let mut b = [0u8; 8];
    r.read_exact(&mut b[..size as usize])
        .map_err(|e| e.to_string())?;
    Ok(u64::from_le_bytes(b))
}

fn skip_array<R: Read + Seek>(r: &mut R) -> Result<(), String> {
    let kind = u32le(r)?;
    let len = u64le(r)?;
    if len > MAX_KV * 64 {
        return Err("matriz grande demais".into());
    }
    if let Some(size) = scalar_size(kind) {
        let skip = len
            .checked_mul(size)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or("matriz grande demais")?;
        r.seek(SeekFrom::Current(skip)).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if kind != 8 {
        return Err(format!("matriz de tipo {kind} não suportada"));
    }
    for _ in 0..len {
        let n = u64le(r)?;
        if n > MAX_STRING {
            return Err("texto longo demais".into());
        }
        r.seek(SeekFrom::Current(n as i64))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------- safetensors

fn safetensors_entry(dir: &Path, files: &[PathBuf], id: String) -> ModelEntry {
    let weights: Vec<&PathBuf> = files.iter().filter(|p| is_safetensors(p)).collect();
    let bytes: u64 = weights
        .iter()
        .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();
    let mut parameters = 0u64;
    let mut by_dtype = HashMap::<String, u64>::new();
    for file in weights {
        match safetensors_header(file) {
            Ok(header) => {
                for (dtype, count) in header {
                    parameters = parameters.saturating_add(count);
                    let slot = by_dtype.entry(dtype).or_default();
                    *slot = slot.saturating_add(count);
                }
            }
            Err(error) => {
                return failed(
                    id,
                    "safetensors",
                    bytes,
                    format!(
                        "cabeçalho safetensors inválido em {}: {error}",
                        file_name(file)
                    ),
                );
            }
        }
    }
    let precision = by_dtype
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(dtype, _)| dtype)
        .unwrap_or_else(|| "?".into());
    let config: Value = read_small(&dir.join("config.json"))
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let model_type = config["model_type"].as_str().unwrap_or("").to_string();
    let number = |keys: &[&str]| keys.iter().find_map(|k| config[*k].as_u64()).unwrap_or(0);
    let small = |value: u64| value.min(u64::from(u32::MAX)) as u32;
    let (supported, reason) = match model_type.as_str() {
        "gpt2" | "llama" => (true, String::new()),
        "" => (false, "falta config.json com model_type".into()),
        other => (
            false,
            format!("arquitetura {other} ainda não tem receita no motor (ADR-022, E3)"),
        ),
    };
    ModelEntry {
        id,
        format: "safetensors".into(),
        architecture: model_type,
        parameters,
        precision,
        bytes,
        context: number(&["max_position_embeddings", "n_positions"]),
        layers: number(&["num_hidden_layers", "n_layer"]).min(u64::from(u32::MAX)) as u32,
        hidden: small(number(&["hidden_size", "n_embd"])),
        heads: small(number(&["num_attention_heads", "n_head"])),
        kv_heads: small(number(&[
            "num_key_value_heads",
            "num_attention_heads",
            "n_head",
        ])),
        supported,
        reason,
    }
}

/// Texto de um arquivo regular de até `MAX_CONFIG` bytes; maior, ilegível ou atalho vira `None`.
fn read_small(path: &Path) -> Option<String> {
    if !is_regular(path) {
        return None;
    }
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_CONFIG + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_CONFIG).then_some(text)
}

/// Parâmetros por tipo de dado de um arquivo safetensors, lidos só do cabeçalho.
fn safetensors_header(path: &Path) -> Result<Vec<(String, u64)>, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let file_len = file.metadata().map_err(|e| e.to_string())?.len();
    let len = u64le(&mut file)?;
    if len == 0 || len > MAX_JSON_HEADER || len > file_len.saturating_sub(8) {
        return Err("tamanho de cabeçalho fora do limite".into());
    }
    let mut raw = vec![0u8; len as usize];
    file.read_exact(&mut raw).map_err(|e| e.to_string())?;
    let json: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let object = json.as_object().ok_or("cabeçalho não é um objeto")?;
    let mut counts = HashMap::<String, u64>::new();
    for (name, tensor) in object {
        if name == "__metadata__" {
            continue;
        }
        let dtype = tensor["dtype"].as_str().ok_or("tensor sem dtype")?;
        let shape = tensor["shape"].as_array().ok_or("tensor sem shape")?;
        let elements = shape
            .iter()
            .map(|d| d.as_u64().ok_or("dimensão inválida"))
            .try_fold(1u64, |acc, d| d.map(|d| acc.saturating_mul(d)))?;
        let slot = counts.entry(dtype.to_string()).or_default();
        *slot = slot.saturating_add(elements);
    }
    Ok(counts.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aic-catalog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn gguf_string_bytes(text: &str) -> Vec<u8> {
        let mut out = (text.len() as u64).to_le_bytes().to_vec();
        out.extend(text.as_bytes());
        out
    }

    /// Um GGUF mínimo: arquitetura, contexto, camadas, tipo de arquivo, uma matriz de textos (que o
    /// leitor deve pular) e dois tensores.
    fn tiny_gguf() -> Vec<u8> {
        let mut out = b"GGUF".to_vec();
        out.extend(3u32.to_le_bytes());
        out.extend(2u64.to_le_bytes()); // tensores
        out.extend(5u64.to_le_bytes()); // pares
        let mut kv = |key: &str, kind: u32, value: Vec<u8>| {
            out.extend(gguf_string_bytes(key));
            out.extend(kind.to_le_bytes());
            out.extend(value);
        };
        kv("general.architecture", 8, gguf_string_bytes("llama"));
        kv("llama.context_length", 4, 2048u32.to_le_bytes().to_vec());
        kv("llama.block_count", 4, 12u32.to_le_bytes().to_vec());
        kv("general.file_type", 4, 15u32.to_le_bytes().to_vec());
        let mut array = 8u32.to_le_bytes().to_vec(); // elementos: texto
        array.extend(2u64.to_le_bytes());
        array.extend(gguf_string_bytes("ab"));
        array.extend(gguf_string_bytes("cde"));
        kv("tokenizer.ggml.tokens", 9, array);
        for (name, dims) in [("a", [4u64, 8]), ("b", [2, 3])] {
            out.extend(gguf_string_bytes(name));
            out.extend(2u32.to_le_bytes());
            for d in dims {
                out.extend(d.to_le_bytes());
            }
            out.extend(0u32.to_le_bytes());
            out.extend(0u64.to_le_bytes());
        }
        out
    }

    #[test]
    fn reads_a_gguf_header_without_loading_weights() {
        let dir = temp("gguf");
        std::fs::write(dir.join("tiny-Q4_K_M.gguf"), tiny_gguf()).unwrap();
        let found = scan(&dir, Runtime::default()).unwrap();
        assert_eq!(found.len(), 1);
        let entry = &found[0];
        assert_eq!(entry.id, "tiny-Q4_K_M.gguf");
        assert_eq!(entry.architecture, "llama");
        assert_eq!(entry.parameters, 4 * 8 + 2 * 3);
        assert_eq!(entry.precision, "Q4_K_M");
        assert_eq!((entry.context, entry.layers), (2048, 12));
        assert!(!entry.supported && entry.reason.contains("llama.cpp"));
        let ready = scan(&dir, Runtime { gguf: true }).unwrap();
        assert!(ready[0].supported);
    }

    #[test]
    fn a_broken_gguf_is_listed_with_the_reason() {
        let dir = temp("broken");
        std::fs::write(dir.join("bad.gguf"), b"not a model").unwrap();
        let found = scan(&dir, Runtime::default()).unwrap();
        assert!(!found[0].supported);
        assert!(found[0].reason.contains("GGUF"), "{}", found[0].reason);
    }

    #[test]
    fn a_truncated_gguf_does_not_panic() {
        let dir = temp("cut");
        let full = tiny_gguf();
        for cut in [3, 10, 30, full.len() - 5] {
            std::fs::write(dir.join("cut.gguf"), &full[..cut]).unwrap();
            let found = scan(&dir, Runtime::default()).unwrap();
            assert!(!found[0].supported);
        }
    }

    #[test]
    fn only_the_first_part_of_a_split_gguf_is_an_entry() {
        let dir = temp("split");
        std::fs::write(dir.join("big-00001-of-00002.gguf"), tiny_gguf()).unwrap();
        std::fs::write(dir.join("big-00002-of-00002.gguf"), vec![0u8; 100]).unwrap();
        let found = scan(&dir, Runtime::default()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].bytes, tiny_gguf().len() as u64 + 100);
    }

    fn write_safetensors(dir: &Path, header: &str) {
        let mut file = File::create(dir.join("model.safetensors")).unwrap();
        file.write_all(&(header.len() as u64).to_le_bytes())
            .unwrap();
        file.write_all(header.as_bytes()).unwrap();
    }

    #[test]
    fn reads_a_safetensors_dir_and_checks_the_architecture() {
        let dir = temp("st");
        let model = dir.join("tiny-llama");
        std::fs::create_dir_all(&model).unwrap();
        write_safetensors(
            &model,
            r#"{"__metadata__":{"format":"pt"},"w":{"dtype":"BF16","shape":[10,20],"data_offsets":[0,400]},"b":{"dtype":"F32","shape":[5],"data_offsets":[400,420]}}"#,
        );
        std::fs::write(
            model.join("config.json"),
            r#"{"model_type":"llama","num_hidden_layers":4,"max_position_embeddings":512}"#,
        )
        .unwrap();
        let other = dir.join("qwen");
        std::fs::create_dir_all(&other).unwrap();
        write_safetensors(
            &other,
            r#"{"w":{"dtype":"F16","shape":[2],"data_offsets":[0,4]}}"#,
        );
        std::fs::write(other.join("config.json"), r#"{"model_type":"qwen2"}"#).unwrap();

        let found = scan(&dir, Runtime::default()).unwrap();
        assert_eq!(found.len(), 2);
        let llama = found.iter().find(|m| m.id == "tiny-llama").unwrap();
        assert!(llama.supported);
        assert_eq!(
            (
                llama.parameters,
                llama.precision.as_str(),
                llama.layers,
                llama.context
            ),
            (205, "BF16", 4, 512)
        );
        let qwen = found.iter().find(|m| m.id == "qwen").unwrap();
        assert!(!qwen.supported && qwen.reason.contains("qwen2"));
    }

    #[test]
    fn a_huge_safetensors_header_is_refused() {
        let dir = temp("huge");
        let model = dir.join("m");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("model.safetensors"), u64::MAX.to_le_bytes()).unwrap();
        let found = scan(&dir, Runtime::default()).unwrap();
        assert!(!found[0].supported);
    }

    #[test]
    fn a_split_gguf_with_a_missing_part_or_a_huge_total_is_refused() {
        let dir = temp("parts");
        std::fs::write(dir.join("a-00001-of-00003.gguf"), tiny_gguf()).unwrap();
        std::fs::write(dir.join("a-00002-of-00003.gguf"), b"x").unwrap();
        let found = scan(&dir, Runtime { gguf: true }).unwrap();
        assert!(
            !found[0].supported && found[0].reason.contains("falta a parte 3"),
            "{}",
            found[0].reason
        );
        std::fs::write(dir.join("b-00001-of-4294967295.gguf"), tiny_gguf()).unwrap();
        let found = scan(&dir, Runtime { gguf: true }).unwrap();
        let huge = found.iter().find(|m| m.id.starts_with("b-")).unwrap();
        assert!(!huge.supported && huge.reason.contains("fora do limite"));
    }

    #[test]
    fn a_safetensors_header_longer_than_the_file_is_refused() {
        let dir = temp("short");
        let model = dir.join("m");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("model.safetensors"), 1_000_000u64.to_le_bytes()).unwrap();
        let found = scan(&dir, Runtime::default()).unwrap();
        assert!(!found[0].supported);
    }

    #[test]
    fn two_enormous_tensors_do_not_overflow_the_parameter_count() {
        let dir = temp("overflow");
        let model = dir.join("m");
        std::fs::create_dir_all(&model).unwrap();
        write_safetensors(
            &model,
            r#"{"a":{"dtype":"F32","shape":[4294967296,4294967296],"data_offsets":[0,0]},"b":{"dtype":"F32","shape":[4294967296,4294967296],"data_offsets":[0,0]}}"#,
        );
        let found = scan(&dir, Runtime::default()).unwrap();
        assert_eq!(found[0].parameters, u64::MAX);
    }

    #[test]
    fn an_oversized_config_json_is_ignored() {
        let dir = temp("bigcfg");
        let model = dir.join("m");
        std::fs::create_dir_all(&model).unwrap();
        write_safetensors(
            &model,
            r#"{"w":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#,
        );
        let padding = " ".repeat(2 << 20);
        std::fs::write(
            model.join("config.json"),
            format!(r#"{{"model_type":"gpt2"{padding}}}"#),
        )
        .unwrap();
        let found = scan(&dir, Runtime::default()).unwrap();
        assert!(!found[0].supported && found[0].reason.contains("model_type"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let dir = temp("links");
        let outside = temp("outside");
        std::fs::write(outside.join("x.gguf"), tiny_gguf()).unwrap();
        std::os::unix::fs::symlink(outside.join("x.gguf"), dir.join("link.gguf")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("pasta")).unwrap();
        assert!(scan(&dir, Runtime::default()).unwrap().is_empty());
    }

    #[test]
    fn a_missing_directory_is_an_error_not_an_empty_catalog() {
        assert!(scan(Path::new("/nonexistent-aic-models"), Runtime::default()).is_err());
    }
}
