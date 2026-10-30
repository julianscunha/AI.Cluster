//! Servidor HTTP do gateway, com TLS opcional: a chave de API e os prompts deixam de cruzar a rede em claro.
//! Ligado por `AI_CLUSTER_GATEWAY_TLS_CERT` e `AI_CLUSTER_GATEWAY_TLS_KEY` (PEM). O handshake é do
//! rustls; depois dele a conexão é do axum, igual à de texto puro.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use hyper_util::rt::{TokioIo, TokioTimer};
use rustls::ServerConfig;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::TlsAcceptor;
use tower_service::Service;

/// Quanto o handshake pode levar: um cliente que abre a conexão e cala não segura recurso.
const HANDSHAKE: Duration = Duration::from_secs(10);

/// Conexões abertas ao mesmo tempo (com ou sem TLS).
const MAX_CONNECTIONS: usize = 256;

/// A configuração do servidor a partir do certificado (cadeia) e da chave, em PEM.
pub fn load_config(cert: &str, key: &str) -> Result<Arc<ServerConfig>, String> {
    let chain = CertificateDer::pem_file_iter(cert)
        .map_err(|e| format!("certificado {cert}: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("certificado {cert}: {e}"))?;
    if chain.is_empty() {
        return Err(format!("certificado {cert}: nenhum certificado no arquivo"));
    }
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("chave {key}: {e}"))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|e| format!("certificado e chave não combinam: {e}"))?;
    // O axum (http1) fala HTTP/1.1.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// Aceita conexões TLS, faz o handshake e entrega cada uma ao `app`. Não retorna.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Router,
    config: Arc<ServerConfig>,
) -> std::io::Result<()> {
    accept_loop(listener, app, Some(TlsAcceptor::from(config))).await
}

/// Como [`serve`], sem TLS: as mesmas defesas (teto de conexões abertas e prazo para o cabeçalho
/// completo) em vez do `axum::serve`, que não tem nenhuma das duas.
pub async fn serve_plain(listener: tokio::net::TcpListener, app: Router) -> std::io::Result<()> {
    accept_loop(listener, app, None).await
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    app: Router,
    acceptor: Option<TlsAcceptor>,
) -> std::io::Result<()> {
    // Teto de conexões abertas: acima dele o accept espera, em vez de gastar um descritor por cliente.
    let open = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (tcp, _) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                // Falta de descritores e afins: espera um pouco em vez de girar sem parar.
                eprintln!("aceitar conexão falhou: {error}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Ok(permit) = open.clone().acquire_owned().await else {
            return Ok(());
        };
        let (acceptor, app) = (acceptor.clone(), app.clone());
        tokio::spawn(async move {
            let _permit = permit;
            match acceptor {
                Some(acceptor) => {
                    let Ok(Ok(stream)) =
                        tokio::time::timeout(HANDSHAKE, acceptor.accept(tcp)).await
                    else {
                        return;
                    };
                    connection(stream, app).await;
                }
                None => connection(tcp, app).await,
            }
        });
    }
}

async fn connection<S>(stream: S, app: Router)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = hyper::service::service_fn(move |request| {
        let mut app = app.clone();
        async move { app.call(request).await }
    });
    // Sem cabeçalho completo em 30 s a conexão cai: quem conecta e cala não a segura.
    let _ = hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(30))
        .serve_connection(TokioIo::new(stream), service)
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn o_servidor_sem_tls_responde_http() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route("/", axum::routing::get(|| async { "ok" }));
        tokio::spawn(serve_plain(listener, app));
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(
                b"GET / HTTP/1.1
Host: x
Connection: close

",
            )
            .await
            .unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).await.unwrap();
        assert!(
            out.starts_with("HTTP/1.1 200") && out.ends_with("ok"),
            "{out}"
        );
    }

    #[test]
    fn arquivos_ausentes_ou_sem_certificado_sao_recusados() {
        assert!(load_config("/nao/existe.pem", "/nao/existe.key").is_err());
        let dir = std::env::temp_dir().join(format!("aic-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let vazio = dir.join("vazio.pem");
        std::fs::write(&vazio, "isto não é PEM\n").unwrap();
        let erro = load_config(vazio.to_str().unwrap(), vazio.to_str().unwrap()).unwrap_err();
        assert!(
            erro.contains("nenhum certificado") || erro.contains("certificado"),
            "{erro}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
