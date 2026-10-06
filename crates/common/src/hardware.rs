use std::{fs, path::Path};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CpuInfo {
    pub architecture: String,
    pub logical_cores: u32,
    pub model: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemoryInfo {
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageKind {
    Hdd,
    SataSsd,
    Nvme,
    Unknown,
}

impl Default for StorageKind {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StorageInfo {
    pub name: String,
    pub kind: StorageKind,
    pub size_bytes: u64,
    pub rotational: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HardwareSnapshot {
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub storage: Vec<StorageInfo>,
}

pub fn discover() -> HardwareSnapshot {
    HardwareSnapshot {
        cpu: discover_cpu(),
        memory: discover_memory(),
        storage: discover_storage(),
    }
}

fn discover_cpu() -> CpuInfo {
    let architecture = std::env::consts::ARCH.to_string();
    let cpuinfo = read_text("/proc/cpuinfo");

    let logical_cores = cpuinfo
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count()
        .max(1) as u32;

    let model = cpuinfo
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if matches!(key.trim(), "model name" | "Hardware" | "Processor") {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".into());

    CpuInfo {
        architecture,
        logical_cores,
        model,
    }
}

fn discover_memory() -> MemoryInfo {
    let total_kib = read_text("/proc/meminfo")
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.trim() == "MemTotal" {
                value.split_whitespace().next()?.parse::<u64>().ok()
            } else {
                None
            }
        })
        .unwrap_or_default();

    MemoryInfo {
        total_bytes: total_kib.saturating_mul(1024),
    }
}

fn discover_storage() -> Vec<StorageInfo> {
    let mut result = Vec::new();
    let dir = match fs::read_dir("/sys/block") {
        Ok(dir) => dir,
        Err(_) => return result,
    };

    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("dm-") {
            continue;
        }

        let base = entry.path();
        let rotational = read_text(base.join("queue/rotational"))
            .trim()
            .parse::<u8>()
            .unwrap_or(1)
            == 1;

        let size_sectors = read_text(base.join("size"))
            .trim()
            .parse::<u64>()
            .unwrap_or_default();

        let size_bytes = size_sectors.saturating_mul(512);
        let kind = classify_storage(&name, rotational);

        result.push(StorageInfo {
            name,
            kind,
            size_bytes,
            rotational,
        });
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

fn classify_storage(name: &str, rotational: bool) -> StorageKind {
    if rotational {
        StorageKind::Hdd
    } else if name.starts_with("nvme") {
        StorageKind::Nvme
    } else {
        StorageKind::SataSsd
    }
}

fn read_text(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{classify_storage, StorageKind};

    #[test]
    fn classifica_nvme() {
        assert_eq!(classify_storage("nvme0n1", false), StorageKind::Nvme);
    }

    #[test]
    fn classifica_hdd_rotacional() {
        assert_eq!(classify_storage("sda", true), StorageKind::Hdd);
    }

    #[test]
    fn classifica_ssd_sata() {
        assert_eq!(classify_storage("sda", false), StorageKind::SataSsd);
    }
}
