//! O resumo do aparelho, lido de /proc e /sys. Campo que não se consegue ler fica vazio ou zero:
//! o operador vê o que faltou em vez de a espera falhar.

use std::fs;

use ai_cluster_protocol::cluster::{AnnounceRequest, InstallDisk};

pub fn collect() -> AnnounceRequest {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let (cpu_model, cpu_cores) = parse_cpu(&cpuinfo);
    AnnounceRequest {
        mac: first_mac(),
        arch: std::env::consts::ARCH.to_string(),
        firmware: if std::path::Path::new("/sys/firmware/efi").exists() {
            "uefi"
        } else {
            "uboot"
        }
        .into(),
        cpu_model,
        cpu_cores,
        ram_bytes: parse_ram(&meminfo),
        disks: disks(),
        secret: Vec::new(),
    }
}

pub fn parse_cpu(cpuinfo: &str) -> (String, u32) {
    let mut model = String::new();
    let mut cores = 0u32;
    for line in cpuinfo.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "processor" => cores += 1,
            "model name" | "Hardware" | "Model" if model.is_empty() => {
                model = value.trim().to_string()
            }
            _ => {}
        }
    }
    (model, cores)
}

pub fn parse_ram(meminfo: &str) -> u64 {
    meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| {
            rest.trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map_or(0, |kib| kib * 1024)
}

/// O MAC da primeira interface (física antes das virtuais, por ordem de nome), em minúsculas; vazio
/// se não houver. Máquina virtual tem interface física emulada; só contêiner cai no segundo grupo.
fn first_mac() -> String {
    let mut names: Vec<(bool, String)> = fs::read_dir("/sys/class/net")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "lo")
        .map(|n| {
            (
                !std::path::Path::new(&format!("/sys/class/net/{n}/device")).exists(),
                n,
            )
        })
        .collect();
    names.sort();
    names
        .iter()
        .find_map(|(_, n)| fs::read_to_string(format!("/sys/class/net/{n}/address")).ok())
        .map(|mac| mac.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Discos de verdade: sem loop, ram, zram, dispositivo óptico nem mapeado.
fn disks() -> Vec<InstallDisk> {
    let mut found: Vec<InstallDisk> = fs::read_dir("/sys/block")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if ["loop", "ram", "zram", "sr", "dm-", "md", "nbd"]
                .iter()
                .any(|p| name.starts_with(p))
            {
                return None;
            }
            let sectors: u64 = fs::read_to_string(entry.path().join("size"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            let model = fs::read_to_string(entry.path().join("device/model")).unwrap_or_default();
            Some(InstallDisk {
                name,
                size_bytes: sectors * 512,
                model: model.trim().to_string(),
            })
        })
        .filter(|d| d.size_bytes > 0)
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_cpu_e_ram_do_proc() {
        let cpu =
            "processor\t: 0\nmodel name\t: Intel i5\nprocessor\t: 1\nmodel name\t: Intel i5\n";
        assert_eq!(parse_cpu(cpu), ("Intel i5".to_string(), 2));
        let arm = "processor\t: 0\nprocessor\t: 1\nHardware\t: Allwinner sun8i Family\n";
        assert_eq!(parse_cpu(arm), ("Allwinner sun8i Family".to_string(), 2));
        assert_eq!(
            parse_ram("MemTotal:        1024 kB\nMemFree: 1 kB\n"),
            1024 * 1024
        );
        assert_eq!(parse_ram(""), 0);
    }
}
