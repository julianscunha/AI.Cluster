//! Model manifest (F12, N1/N2): the name, the version and the SHA-256 of every file of a model
//! directory, in `manifest.json`. It is what lets the gateway and the workers say they are
//! running the same weights, and refuse weights that changed on disk.

use std::io::Read;
use std::path::Path;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The file a model directory keeps its manifest in.
pub const FILE: &str = "manifest.json";

/// The weights every model directory is loaded from; a manifest that does not cover them
/// vouches for nothing.
const WEIGHTS: &str = "model.safetensors";

/// A manifest bigger than this, or listing more files, is not one this code wrote.
const MAX_MANIFEST_BYTES: u64 = 1 << 20;
const MAX_FILES: usize = 1024;

/// Letters, digits and `._+-`, 1 to 64 of them: safe in a header value and in a log line.
fn plain(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub files: Vec<Entry>,
}

fn hash_file(path: &Path) -> Result<(u64, String), String> {
    let mut file = std::fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    let digest = hasher.finalize();
    Ok((
        total,
        digest.iter().map(|byte| format!("{byte:02x}")).collect(),
    ))
}

impl Manifest {
    /// Hashes every regular file directly inside `dir` (a model directory is flat), except the
    /// manifest itself. Sorted by name, so the same weights always give the same manifest.
    pub fn build(dir: &Path, name: &str, version: &str) -> Result<Self, String> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))? {
            let entry = entry.map_err(|err| err.to_string())?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if file_name != FILE && entry.path().is_file() {
                names.push(file_name);
            }
        }
        names.sort();
        let mut files = Vec::new();
        for path in names {
            let (bytes, sha256) = hash_file(&dir.join(&path))?;
            files.push(Entry {
                path,
                bytes,
                sha256,
            });
        }
        if !files.iter().any(|file| file.path == WEIGHTS) {
            return Err(format!("{}: sem {WEIGHTS} para o manifest", dir.display()));
        }
        if !plain(name) || !plain(version) {
            return Err("`name` e `version` aceitam letras, dígitos e ._+-".to_string());
        }
        Ok(Self {
            name: name.to_string(),
            version: version.to_string(),
            files,
        })
    }

    pub fn to_json(&self) -> String {
        let files: Vec<Value> = self
            .files
            .iter()
            .map(|file| json!({ "path": file.path, "bytes": file.bytes, "sha256": file.sha256 }))
            .collect();
        let mut text = serde_json::to_string_pretty(&json!({
            "name": self.name,
            "version": self.version,
            "files": files,
        }))
        .expect("a manifest is always valid JSON");
        text.push('\n');
        text
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|err| format!("manifest: {err}"))?;
        let field = |name: &str| {
            value
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("manifest: falta `{name}`"))
        };
        let mut files = Vec::new();
        for file in value
            .get("files")
            .and_then(Value::as_array)
            .ok_or("manifest: falta `files`")?
        {
            let text = |name: &str| {
                file.get(name)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| format!("manifest: arquivo sem `{name}`"))
            };
            let path = text("path")?;
            // A path that leaves the directory would let a manifest point at someone else's file.
            if !plain(&path) || path == ".." || path == "." {
                return Err(format!("manifest: caminho inválido `{path}`"));
            }
            if files.iter().any(|known: &Entry| known.path == path) {
                return Err(format!("manifest: `{path}` repetido"));
            }
            if files.len() >= MAX_FILES {
                return Err("manifest: arquivos demais".to_string());
            }
            let sha256 = text("sha256")?;
            if sha256.len() != 64 || !sha256.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
                return Err(format!("manifest: sha256 inválido em `{path}`"));
            }
            files.push(Entry {
                path,
                bytes: file
                    .get("bytes")
                    .and_then(Value::as_u64)
                    .ok_or("manifest: arquivo sem `bytes`")?,
                sha256,
            });
        }
        if !files.iter().any(|file| file.path == WEIGHTS) {
            return Err(format!("manifest: não cobre {WEIGHTS}"));
        }
        let (name, version) = (field("name")?, field("version")?);
        if !plain(&name) || !plain(&version) {
            return Err("manifest: `name` e `version` aceitam letras, dígitos e ._+-".to_string());
        }
        Ok(Self {
            name,
            version,
            files,
        })
    }

    /// The manifest of `dir`, or `None` when it has none.
    pub fn load(dir: &Path) -> Result<Option<Self>, String> {
        let path = dir.join(FILE);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.len() > MAX_MANIFEST_BYTES => {
                return Err(format!(
                    "{}: maior que {MAX_MANIFEST_BYTES} bytes",
                    path.display()
                ));
            }
            _ => {}
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::from_json(&text).map(Some),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(format!("{}: {err}", dir.join(FILE).display())),
        }
    }

    /// Checks size and hash of every listed file against what is on disk now.
    pub fn verify(&self, dir: &Path) -> Result<(), String> {
        for entry in &self.files {
            let (bytes, sha256) = hash_file(&dir.join(&entry.path))?;
            if bytes != entry.bytes || sha256 != entry.sha256 {
                let what = if bytes != entry.bytes {
                    format!("tamanho {bytes}, o manifest diz {}", entry.bytes)
                } else {
                    "conteúdo diferente (hash)".to_string()
                };
                return Err(format!(
                    "{}: {} não confere com o manifest {} {}: {what}",
                    dir.display(),
                    entry.path,
                    self.name,
                    self.version,
                ));
            }
        }
        Ok(())
    }
}

/// The version of the model in `dir` after checking its weights, or `None` when it has no
/// manifest and one is not required. A manifest that does not match, or a missing one when
/// `required`, is an error.
pub fn verified_version(dir: &Path, required: bool) -> Result<Option<String>, String> {
    match Manifest::load(dir)? {
        Some(manifest) => {
            manifest.verify(dir)?;
            Ok(Some(manifest.version))
        }
        None if required => Err(format!("{}: sem {FILE} (exigido)", dir.display())),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aic-manifest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_manifest_round_trips_and_verifies_until_a_file_changes() {
        let dir = temp_dir("rt");
        std::fs::write(dir.join("model.safetensors"), b"weights").unwrap();
        std::fs::write(dir.join("config.json"), b"{}").unwrap();
        let manifest = Manifest::build(&dir, "tiny", "1.0.0").unwrap();
        assert_eq!(manifest.files.len(), 2);
        // Sorted by name: the same weights always give the same text.
        assert_eq!(manifest.files[0].path, "config.json");
        assert_eq!(Manifest::from_json(&manifest.to_json()).unwrap(), manifest);
        std::fs::write(dir.join(FILE), manifest.to_json()).unwrap();
        // The manifest does not list itself.
        assert_eq!(Manifest::build(&dir, "tiny", "1.0.0").unwrap(), manifest);

        assert_eq!(
            verified_version(&dir, true).unwrap().as_deref(),
            Some("1.0.0")
        );
        // One flipped byte is caught, and the error names the file.
        std::fs::write(dir.join("model.safetensors"), b"weightz").unwrap();
        let err = verified_version(&dir, false).unwrap_err();
        assert!(err.contains("model.safetensors"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_manifest_is_fine_unless_required() {
        let dir = temp_dir("none");
        std::fs::write(dir.join("model.safetensors"), b"w").unwrap();
        assert_eq!(verified_version(&dir, false).unwrap(), None);
        assert!(verified_version(&dir, true).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_cannot_point_outside_its_directory() {
        let hash = "0".repeat(64);
        let json = |path: &str| {
            format!(
                r#"{{"name":"x","version":"1","files":[{{"path":"{path}","bytes":1,"sha256":"{hash}"}},{{"path":"model.safetensors","bytes":1,"sha256":"{hash}"}}]}}"#
            )
        };
        assert!(Manifest::from_json(&json("config.json")).is_ok());
        for bad in ["../secret", "a/b", "a\\b", "..", "C:x", ""] {
            assert!(Manifest::from_json(&json(bad)).is_err(), "{bad}");
        }
        // The weights must be listed, the hash well formed and the version plain.
        assert!(Manifest::from_json(r#"{"name":"x","version":"1","files":[]}"#).is_err());
        let short = r#"{"name":"x","version":"1","files":[{"path":"model.safetensors","bytes":1,"sha256":"00"}]}"#;
        assert!(Manifest::from_json(short).is_err());
        let newline = json("config.json").replace(r#""version":"1""#, r#""version":"1\nx""#);
        assert!(Manifest::from_json(&newline).is_err());
        assert!(Manifest::from_json("não é json").is_err());
        assert!(Manifest::from_json(r#"{"name":"x"}"#).is_err());
    }

    #[test]
    fn known_hash() {
        let dir = temp_dir("hash");
        std::fs::write(dir.join("model.safetensors"), b"abc").unwrap();
        let manifest = Manifest::build(&dir, "n", "v").unwrap();
        assert_eq!(
            manifest.files[0].sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
