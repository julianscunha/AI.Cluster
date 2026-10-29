//! O `ggml-rpc-server` do llama.cpp neste nó (ADR-022). O worker o roda como processo filho,
//! supervisionado, escutando só em 127.0.0.1, e é o túnel mTLS (`ai-cluster-tunnel`) a única porta
//! aberta para a rede: o `rpc-server` não tem senha nem criptografia e executa o que o cliente manda.
//!
//! Ligado por `AI_CLUSTER_RPC_SERVER_BIN`. Sem certificado do worker e CA dos clientes o túnel não
//! sobe e o nó não anuncia RPC: a falha é fechada, nunca em texto puro.

use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

/// A configuração do `rpc-server` deste nó, lida do ambiente uma única vez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcConfig {
    pub bin: String,
    /// Porta do `rpc-server` em 127.0.0.1.
    pub local_port: u16,
    /// Endereço do túnel (o que a rede enxerga).
    pub listen: String,
    pub cert: String,
    pub key: String,
    pub client_ca: String,
    /// Nome no SAN que o certificado do cliente precisa ter (`aic-gateway`): o de um worker não vale.
    pub client_role: String,
    pub threads: usize,
    /// `-c` do `rpc-server`: cache dos tensores em disco. No cartão SD ocupa o tamanho da fatia.
    pub cache: bool,
    /// `LLAMA_CACHE` do `rpc-server`: onde o cache mora (`AI_CLUSTER_RPC_CACHE_DIR`). Sem ele, o
    /// llama.cpp usa o diretório do usuário, que num contêiner some junto com ele.
    pub cache_dir: Option<String>,
}

impl RpcConfig {
    /// A porta do túnel, a que o controller entrega ao gateway.
    pub fn port(&self) -> u32 {
        self.listen
            .rsplit(':')
            .next()
            .and_then(|port| port.parse().ok())
            .unwrap_or(0)
    }

    /// Argumentos do `rpc-server`: loopback apenas, e as threads do nó.
    pub fn server_args(&self) -> Vec<String> {
        let mut args = vec![
            "-H".to_string(),
            "127.0.0.1".to_string(),
            "-p".to_string(),
            self.local_port.to_string(),
            "-t".to_string(),
            self.threads.max(1).to_string(),
        ];
        if self.cache {
            args.push("-c".to_string());
        }
        args
    }
}

/// Lê o ambiente. `None` quando o nó não roda `rpc-server`; `Some(Err)` quando pediu e a configuração
/// é insegura ou incompleta (o nó segue sem RPC).
pub fn from_env(threads: usize) -> Option<Result<RpcConfig, String>> {
    let var = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let bin = var("AI_CLUSTER_RPC_SERVER_BIN")?;
    let (Some(cert), Some(key), Some(client_ca)) = (
        var("AI_CLUSTER_RPC_TLS_CERT"),
        var("AI_CLUSTER_RPC_TLS_KEY"),
        var("AI_CLUSTER_RPC_TLS_CLIENT_CA"),
    ) else {
        return Some(Err(
            "AI_CLUSTER_RPC_SERVER_BIN pede também AI_CLUSTER_RPC_TLS_CERT, _KEY e _CLIENT_CA: sem o túnel mTLS o rpc-server não é exposto"
                .into(),
        ));
    };
    let local_port = match var("AI_CLUSTER_RPC_LOCAL_PORT") {
        Some(text) => match text.parse::<u16>() {
            Ok(port) if port > 0 => port,
            _ => return Some(Err(format!("AI_CLUSTER_RPC_LOCAL_PORT inválida: {text}"))),
        },
        None => 50062,
    };
    let listen = var("AI_CLUSTER_RPC_LISTEN").unwrap_or_else(|| "0.0.0.0:50061".into());
    if listen
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .is_none()
    {
        return Some(Err(format!("AI_CLUSTER_RPC_LISTEN inválido: {listen}")));
    }
    let cache_dir = var("AI_CLUSTER_RPC_CACHE_DIR");
    // Explícito vale; sem a variável o cache liga sozinho se o diretório tem o dobro da RAM livre.
    // shortcut: a regra é grosseira (2 × RAM total), refine se houver medida de ganho por disco.
    let cache = match var("AI_CLUSTER_RPC_CACHE").as_deref() {
        Some("1" | "true") => true,
        Some(_) => false,
        None => cache_dir.as_deref().is_some_and(|dir| {
            let ram = memory_total_bytes();
            ram > 0 && free_bytes(dir) >= ram.saturating_mul(2)
        }),
    };
    let client_role = var("AI_CLUSTER_RPC_CLIENT_NAME").unwrap_or_else(|| "aic-gateway".into());
    Some(Ok(RpcConfig {
        bin,
        local_port,
        listen,
        cert,
        key,
        client_ca,
        client_role,
        threads,
        cache,
        cache_dir,
    }))
}

/// O endereço por onde o gateway chega a este nó: `AI_CLUSTER_RPC_ADVERTISE_HOST`, ou o IP local que
/// a rota até o controller usa (um socket UDP "conectado" não envia nada; só escolhe a interface).
/// Vazio se não der para saber: o controller então usa o endereço de onde o nó se registrou, que atrás
/// de um NAT (Docker, por exemplo) é o do NAT.
pub fn advertise_host(controller: &str) -> String {
    if let Ok(host) = std::env::var("AI_CLUSTER_RPC_ADVERTISE_HOST") {
        let host = host.trim().to_string();
        if !host.is_empty() {
            return host;
        }
    }
    local_ip_toward(controller).unwrap_or_default()
}

/// O IP local da interface que leva ao `endpoint` (`http://host:porta`), se não for loopback.
fn local_ip_toward(endpoint: &str) -> Option<String> {
    use std::net::ToSocketAddrs;
    let rest = endpoint.split("://").nth(1).unwrap_or(endpoint);
    let authority = rest.split('/').next()?;
    let target = authority.to_socket_addrs().ok()?.next()?;
    let bind = if target.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = std::net::UdpSocket::bind(bind).ok()?;
    socket.connect(target).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

static CONFIG: OnceLock<Option<RpcConfig>> = OnceLock::new();

/// A configuração válida deste nó, ou `None` (sem RPC). Registra o motivo de uma recusa uma vez.
pub fn config(threads: usize) -> Option<&'static RpcConfig> {
    CONFIG
        .get_or_init(|| match from_env(threads) {
            Some(Ok(config)) => Some(config),
            Some(Err(reason)) => {
                eprintln!("rpc-server desligado: {reason}");
                None
            }
            None => None,
        })
        .as_ref()
}

/// Sobe o túnel mTLS e mantém o `rpc-server` vivo. Só retorna se o túnel não puder subir.
pub async fn run(config: &'static RpcConfig) -> Result<(), String> {
    wait_for_valid_clock().await;
    let tls = ai_cluster_tunnel::server_config(
        &config.cert,
        &config.key,
        &config.client_ca,
        &config.client_role,
    )?;
    let listener = tokio::net::TcpListener::bind(&config.listen)
        .await
        .map_err(|e| format!("túnel RPC em {}: {e}", config.listen))?;
    println!(
        "túnel RPC (mTLS) em {} para o rpc-server em 127.0.0.1:{}",
        config.listen, config.local_port
    );
    let upstream = format!("127.0.0.1:{}", config.local_port);
    tokio::spawn(async move {
        if let Err(error) = ai_cluster_tunnel::serve(listener, upstream, tls).await {
            eprintln!("túnel RPC encerrou: {error}");
        }
    });
    supervise(config).await;
    Ok(())
}

/// Antes disto (2026-01-01) o relógio é o de uma placa sem RTC que ainda não sincronizou: o
/// certificado de um túnel válido hoje seria recusado como "ainda não válido".
const CLOCK_FLOOR_UNIX: u64 = 1_767_225_600;

/// Espera a hora do sistema passar do piso; avisa uma vez e depois a cada minuto.
async fn wait_for_valid_clock() {
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    };
    let mut waited = 0u32;
    while now() < CLOCK_FLOOR_UNIX {
        if waited.is_multiple_of(12) {
            eprintln!(
                "a hora do sistema ainda é anterior a 2026: o túnel RPC espera o relógio acertar (NTP)"
            );
        }
        waited += 1;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Espaço livre, em bytes, no sistema de arquivos de `dir` (0 se não existe ou fora do Linux).
#[cfg(target_os = "linux")]
fn free_bytes(dir: &str) -> u64 {
    let Ok(path) = std::ffi::CString::new(dir) else {
        return 0;
    };
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `path` é uma string C válida e `stat` tem o tamanho de uma `statvfs`.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return 0;
    }
    // SAFETY: a chamada preencheu a estrutura.
    let stat = unsafe { stat.assume_init() };
    // Em armv7 os campos são u32: o cast é necessário lá, redundante em x86_64.
    #[allow(clippy::unnecessary_cast)]
    let free = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
    free
}

#[cfg(not(target_os = "linux"))]
fn free_bytes(_dir: &str) -> u64 {
    0
}

/// RAM total da máquina em bytes (`/proc/meminfo`); 0 quando não dá para ler.
fn memory_total_bytes() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("MemTotal:"))
                .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
        })
        .map_or(0, |kib| kib.saturating_mul(1024))
}

/// A porta do `rpc-server` em loopback está livre? Outro processo nela receberia o tráfego do túnel.
fn port_is_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Faz o filho morrer junto com o pai (Linux): `kill_on_drop` não cobre SIGKILL nem falta de memória.
#[cfg(target_os = "linux")]
fn die_with_parent(command: &mut tokio::process::Command) {
    // SAFETY: só chama `prctl` entre o fork e o exec, sem alocar nem tocar em estado do processo.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn die_with_parent(_command: &mut tokio::process::Command) {}

/// Roda o `rpc-server`; se sair, espera um pouco (cada vez mais) e o sobe de novo.
async fn supervise(config: &RpcConfig) {
    let mut wait = Duration::from_secs(1);
    loop {
        let started = std::time::Instant::now();
        if !port_is_free(config.local_port) {
            eprintln!(
                "a porta {} (rpc-server) está ocupada por outro processo; tentando de novo",
                config.local_port
            );
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(Duration::from_secs(30));
            continue;
        }
        let mut command = tokio::process::Command::new(&config.bin);
        command
            .args(config.server_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if let Some(dir) = &config.cache_dir {
            command.env("LLAMA_CACHE", dir);
        }
        die_with_parent(&mut command);
        match command.spawn() {
            Ok(mut child) => match child.wait().await {
                Ok(status) => eprintln!("rpc-server saiu ({status}); reiniciando"),
                Err(error) => eprintln!("rpc-server: {error}; reiniciando"),
            },
            Err(error) => eprintln!("rpc-server não iniciou ({}): {error}", config.bin),
        }
        // Uma execução longa zera a espera; quedas seguidas a dobram até meio minuto.
        wait = if started.elapsed() > Duration::from_secs(60) {
            Duration::from_secs(1)
        } else {
            (wait * 2).min(Duration::from_secs(30))
        };
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // O ambiente é do processo inteiro: um teste só, em série, mexe nele.
    #[test]
    fn the_environment_decides_and_insecure_setups_are_refused() {
        let names = [
            "AI_CLUSTER_RPC_SERVER_BIN",
            "AI_CLUSTER_RPC_TLS_CERT",
            "AI_CLUSTER_RPC_TLS_KEY",
            "AI_CLUSTER_RPC_TLS_CLIENT_CA",
            "AI_CLUSTER_RPC_LOCAL_PORT",
            "AI_CLUSTER_RPC_LISTEN",
            "AI_CLUSTER_RPC_CACHE",
            "AI_CLUSTER_RPC_CACHE_DIR",
            "AI_CLUSTER_RPC_CLIENT_NAME",
        ];
        let clear = || {
            names
                .iter()
                .for_each(|n| unsafe { std::env::remove_var(n) })
        };
        clear();
        assert!(from_env(4).is_none(), "sem o binário, o nó não roda RPC");

        unsafe { std::env::set_var("AI_CLUSTER_RPC_SERVER_BIN", "/usr/bin/ggml-rpc-server") };
        let refused = from_env(4).unwrap().unwrap_err();
        assert!(refused.contains("mTLS"), "{refused}");

        unsafe {
            std::env::set_var("AI_CLUSTER_RPC_TLS_CERT", "/c.pem");
            std::env::set_var("AI_CLUSTER_RPC_TLS_KEY", "/k.pem");
            std::env::set_var("AI_CLUSTER_RPC_TLS_CLIENT_CA", "/ca.pem");
        }
        let config = from_env(4).unwrap().unwrap();
        assert_eq!((config.local_port, config.port()), (50062, 50061));
        assert_eq!(
            config.server_args(),
            ["-H", "127.0.0.1", "-p", "50062", "-t", "4"],
            "o rpc-server só escuta em loopback"
        );

        unsafe {
            std::env::set_var("AI_CLUSTER_RPC_LISTEN", "0.0.0.0:6000");
            std::env::set_var("AI_CLUSTER_RPC_CACHE", "1");
        }
        let config = from_env(0).unwrap().unwrap();
        assert_eq!(config.port(), 6000);
        assert_eq!(config.server_args().last().map(String::as_str), Some("-c"));
        assert_eq!(config.server_args()[5], "1", "pelo menos uma thread");

        // Com o diretório e sem a opção: liga só se houver o dobro da RAM livre (o /tmp do teste decide).
        unsafe {
            std::env::remove_var("AI_CLUSTER_RPC_CACHE");
            std::env::set_var("AI_CLUSTER_RPC_CACHE_DIR", "/nao/existe");
        }
        let config = from_env(4).unwrap().unwrap();
        assert!(!config.cache, "diretório inexistente não liga o cache");
        assert_eq!(config.cache_dir.as_deref(), Some("/nao/existe"));
        unsafe { std::env::set_var("AI_CLUSTER_RPC_CACHE", "0") };
        assert!(!from_env(4).unwrap().unwrap().cache, "0 desliga");
        #[cfg(target_os = "linux")]
        assert!(free_bytes("/tmp") > 0 && memory_total_bytes() > 0);

        unsafe { std::env::set_var("AI_CLUSTER_RPC_LISTEN", "sem-porta") };
        assert!(from_env(4).unwrap().is_err());
        unsafe {
            std::env::set_var("AI_CLUSTER_RPC_LISTEN", "0.0.0.0:6000");
            std::env::set_var("AI_CLUSTER_RPC_LOCAL_PORT", "0");
        }
        assert!(from_env(4).unwrap().is_err());
        clear();
    }
    #[test]
    fn the_local_ip_is_the_one_on_the_route_to_the_controller() {
        // Sem rede externa: para um alvo local, o IP é o da interface (ou nenhum, se só há loopback).
        let ip = local_ip_toward("http://127.0.0.1:50051");
        assert!(
            ip.is_none(),
            "o loopback não é um endereço que o gateway alcance: {ip:?}"
        );
        assert!(local_ip_toward("http://").is_none());
    }
}
