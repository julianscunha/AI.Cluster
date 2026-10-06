use std::{fs::{self, OpenOptions}, hint::black_box, io::{Read, Write}, time::Instant};

#[derive(Debug, Clone, Default)]
pub struct BenchmarkSnapshot {
    pub cpu_ops_per_sec: f64,
    pub memory_mb_per_sec: f64,
    pub storage_write_mb_per_sec: f64,
}

pub fn run() -> BenchmarkSnapshot {
    BenchmarkSnapshot {
        cpu_ops_per_sec: benchmark_cpu(),
        memory_mb_per_sec: benchmark_memory(),
        storage_write_mb_per_sec: benchmark_storage(),
    }
}

fn benchmark_cpu() -> f64 {
    const ITERATIONS: u64 = 10_000_000;
    let start = Instant::now();
    let mut value = 0u64;

    for i in 0..ITERATIONS {
        value = value
            .wrapping_mul(1_664_525)
            .wrapping_add(i ^ 0x9e37_79b9);
    }

    black_box(value);
    ITERATIONS as f64 / start.elapsed().as_secs_f64().max(f64::MIN_POSITIVE)
}

fn benchmark_memory() -> f64 {
    const SIZE: usize = 64 * 1024 * 1024;
    let mut buffer = vec![0u8; SIZE];
    let start = Instant::now();

    for (index, byte) in buffer.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(31);
    }

    let checksum = buffer.iter().fold(0u64, |acc, value| acc + u64::from(*value));
    black_box(checksum);

    (SIZE as f64 / (1024.0 * 1024.0)) / start.elapsed().as_secs_f64().max(f64::MIN_POSITIVE)
}

fn benchmark_storage() -> f64 {
    const SIZE: usize = 16 * 1024 * 1024;
    let path = std::env::temp_dir().join(format!("real-ai-cluster-bench-{}", std::process::id()));
    let data = vec![0xA5u8; 1024 * 1024];

    let start = Instant::now();
    let result = (|| -> std::io::Result<f64> {
        let mut file = OpenOptions::new().create(true).truncate(true).write(true).open(&path)?;

        for _ in 0..(SIZE / data.len()) {
            file.write_all(&data)?;
        }

        file.sync_all()?;
        Ok((SIZE as f64 / (1024.0 * 1024.0)) / start.elapsed().as_secs_f64().max(f64::MIN_POSITIVE))
    })();

    let _ = fs::remove_file(&path);
    result.unwrap_or_default()
}

pub fn benchmark_file_read(path: &std::path::Path) -> std::io::Result<u64> {
    let mut file = fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn benchmark_produz_valores_nao_negativos() {
        let result = run();
        assert!(result.cpu_ops_per_sec > 0.0);
        assert!(result.memory_mb_per_sec > 0.0);
        assert!(result.storage_write_mb_per_sec >= 0.0);
    }
}
