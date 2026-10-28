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
    pub threads: usize,
    /// `-c` do `rpc-server`: cache dos tensores em disco. No cartão SD ocupa o tamanho da fatia.
    pub cache: bool,
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
    let cache = matches!(var("AI_CLUSTER_RPC_CACHE").as_deref(), Some("1" | "true"));
    Some(Ok(RpcConfig {
        bin,
        local_port,
        listen,
        cert,
        key,
        client_ca,
        threads,
        cache,
    }))
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
    let tls = ai_cluster_tunnel::server_config(&config.cert, &config.key, &config.client_ca)?;
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

/// Roda o `rpc-server`; se sair, espera um pouco (cada vez mais) e o sobe de novo.
async fn supervise(config: &RpcConfig) {
    let mut wait = Duration::from_secs(1);
    loop {
        let started = std::time::Instant::now();
        let child = tokio::process::Command::new(&config.bin)
            .args(config.server_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn();
        match child {
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

        unsafe { std::env::set_var("AI_CLUSTER_RPC_LISTEN", "sem-porta") };
        assert!(from_env(4).unwrap().is_err());
        unsafe {
            std::env::set_var("AI_CLUSTER_RPC_LISTEN", "0.0.0.0:6000");
            std::env::set_var("AI_CLUSTER_RPC_LOCAL_PORT", "0");
        }
        assert!(from_env(4).unwrap().is_err());
        clear();
    }
}
