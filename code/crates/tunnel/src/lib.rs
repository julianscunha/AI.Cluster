//! Túnel TCP sobre mTLS para o `rpc-server` do llama.cpp (ADR-022). Esse servidor não tem senha nem
//! criptografia e executa o que o cliente manda: ele escuta só em 127.0.0.1 e este túnel é a única
//! porta que o worker abre para a rede.
//!
//! - **Servidor** (`serve`, no worker): aceita TLS com certificado de cliente assinado pela CA e
//!   repassa os bytes ao `rpc-server` local.
//! - **Cliente** (`forward`, no gateway): escuta em 127.0.0.1 e repassa cada conexão ao worker por TLS,
//!   apresentando o certificado do gateway.
//!
//! Preferência por ChaCha20-Poly1305: o Cortex-A7 não tem instruções de AES e gastaria a CPU da
//! inferência na criptografia. `TCP_NODELAY` nas duas pontas: o RPC troca muitas mensagens pequenas.

use std::sync::Arc;
use std::time::Duration;

use rustls::crypto::{CryptoProvider, ring};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

/// Quanto o handshake e a conexão ao destino podem levar.
const SETUP: Duration = Duration::from_secs(10);
/// Conexões abertas ao mesmo tempo em cada ponta (depois do handshake).
const MAX_CONNECTIONS: usize = 64;
/// Handshakes em andamento: quem abre TCP e cala só ocupa uma destas vagas, por no máximo `SETUP`,
/// e nunca uma das conexões abertas.
const MAX_HANDSHAKES: usize = 32;

/// ChaCha20-Poly1305 primeiro; AES só como reserva para clientes que não falem ChaCha.
fn provider() -> Arc<CryptoProvider> {
    let mut provider = ring::default_provider();
    provider.cipher_suites = vec![
        ring::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
        ring::cipher_suite::TLS13_AES_256_GCM_SHA384,
        ring::cipher_suite::TLS13_AES_128_GCM_SHA256,
    ];
    Arc::new(provider)
}

fn certs(path: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let list = CertificateDer::pem_file_iter(path)
        .map_err(|e| format!("certificado {path}: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("certificado {path}: {e}"))?;
    if list.is_empty() {
        return Err(format!("certificado {path}: nenhum certificado no arquivo"));
    }
    Ok(list)
}

fn roots(ca: &str) -> Result<RootCertStore, String> {
    let mut store = RootCertStore::empty();
    for cert in certs(ca)? {
        store.add(cert).map_err(|e| format!("CA {ca}: {e}"))?;
    }
    Ok(store)
}

/// A configuração do túnel servidor: certificado do worker e CA que assina os clientes. Sem
/// certificado de cliente válido o handshake falha.
pub fn server_config(cert: &str, key: &str, client_ca: &str) -> Result<Arc<ServerConfig>, String> {
    let chain = certs(cert)?;
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("chave {key}: {e}"))?;
    let verifier =
        WebPkiClientVerifier::builder_with_provider(Arc::new(roots(client_ca)?), provider())
            .build()
            .map_err(|e| format!("CA de clientes {client_ca}: {e}"))?;
    let mut config = ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .with_client_cert_verifier(verifier)
        .with_single_cert(chain, key)
        .map_err(|e| format!("certificado e chave não combinam: {e}"))?;
    // O servidor escolhe a cifra: o Pi impõe o ChaCha mesmo diante de um cliente com AES-NI.
    config.ignore_client_order = true;
    Ok(Arc::new(config))
}

/// A configuração do túnel cliente: CA que assina os workers e o certificado do gateway.
pub fn client_config(ca: &str, cert: &str, key: &str) -> Result<Arc<ClientConfig>, String> {
    let chain = certs(cert)?;
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("chave {key}: {e}"))?;
    let config = ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots(ca)?)
        .with_client_auth_cert(chain, key)
        .map_err(|e| format!("certificado de cliente: {e}"))?;
    Ok(Arc::new(config))
}

fn nodelay(stream: &TcpStream) {
    let _ = stream.set_nodelay(true);
    // Um par que some sem FIN (queda de energia, cabo) não segura a vaga para sempre.
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(10));
    let _ = socket2::SockRef::from(stream).set_tcp_keepalive(&keepalive);
}

/// Servidor: aceita TLS em `listener` e repassa cada conexão a `upstream` (o `rpc-server` local).
/// Não retorna.
pub async fn serve(
    listener: TcpListener,
    upstream: String,
    config: Arc<ServerConfig>,
) -> std::io::Result<()> {
    let acceptor = TlsAcceptor::from(config);
    let open = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let handshakes = Arc::new(tokio::sync::Semaphore::new(MAX_HANDSHAKES));
    loop {
        let (tcp, _) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                eprintln!("túnel: aceitar conexão falhou: {error}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        // Sem vaga de handshake a conexão é descartada na hora: o accept nunca espera por um intruso.
        let Ok(handshake) = handshakes.clone().try_acquire_owned() else {
            continue;
        };
        nodelay(&tcp);
        let (acceptor, upstream, open) = (acceptor.clone(), upstream.clone(), open.clone());
        tokio::spawn(async move {
            let Ok(Ok(mut tls)) = tokio::time::timeout(SETUP, acceptor.accept(tcp)).await else {
                return;
            };
            // Só quem tem certificado válido ocupa uma das conexões abertas.
            drop(handshake);
            let Ok(_permit) = open.try_acquire_owned() else {
                return;
            };
            let Ok(Ok(mut local)) =
                tokio::time::timeout(SETUP, TcpStream::connect(&upstream)).await
            else {
                return;
            };
            nodelay(&local);
            let _ = copy_bidirectional(&mut tls, &mut local).await;
        });
    }
}

/// Cliente: escuta em `listener` (127.0.0.1) e repassa cada conexão a `remote` por TLS, verificando o
/// servidor pelo nome `server_name`. Não retorna.
pub async fn forward(
    listener: TcpListener,
    remote: String,
    server_name: String,
    config: Arc<ClientConfig>,
) -> std::io::Result<()> {
    let name = ServerName::try_from(server_name)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    let connector = TlsConnector::from(config);
    let open = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (mut local, _) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                eprintln!("túnel: aceitar conexão falhou: {error}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        // Acima do limite a conexão local é recusada, em vez de travar o accept.
        let Ok(permit) = open.clone().try_acquire_owned() else {
            continue;
        };
        nodelay(&local);
        let (connector, remote, name) = (connector.clone(), remote.clone(), name.clone());
        tokio::spawn(async move {
            let _permit = permit;
            let setup = async {
                let tcp = TcpStream::connect(&remote).await?;
                nodelay(&tcp);
                connector.connect(name, tcp).await
            };
            let Ok(Ok(mut tls)) = tokio::time::timeout(SETUP, setup).await else {
                return;
            };
            let _ = copy_bidirectional(&mut local, &mut tls).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{CertificateParams, DnType, IsCa, KeyPair};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Pki {
        dir: std::path::PathBuf,
    }

    fn write(dir: &std::path::Path, name: &str, text: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// Uma CA e certificados assinados por ela: `worker` (servidor) e `gateway` (cliente); mais uma
    /// segunda CA, de um intruso, com um certificado de cliente próprio.
    fn pki(tag: &str) -> Pki {
        let dir = std::env::temp_dir().join(format!("aic-tunnel-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let issue = |name: &str, ca: &(rcgen::Certificate, KeyPair), san: &str| {
            let key = KeyPair::generate().unwrap();
            let mut params = CertificateParams::new(vec![san.to_string()]).unwrap();
            params.distinguished_name.push(DnType::CommonName, name);
            let cert = params.signed_by(&key, &ca.0, &ca.1).unwrap();
            write(&dir, &format!("{name}.crt"), &cert.pem());
            write(&dir, &format!("{name}.key"), &key.serialize_pem());
        };
        let make_ca = |name: &str| {
            let key = KeyPair::generate().unwrap();
            let mut params = CertificateParams::new(vec![]).unwrap();
            params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            params.distinguished_name.push(DnType::CommonName, name);
            let cert = params.self_signed(&key).unwrap();
            write(&dir, &format!("{name}.crt"), &cert.pem());
            (cert, key)
        };
        let ca = make_ca("ca");
        let evil = make_ca("evil-ca");
        issue("worker", &ca, "aic-worker");
        issue("gateway", &ca, "aic-gateway");
        issue("intruso", &evil, "aic-gateway");
        Pki { dir }
    }

    impl Pki {
        fn p(&self, name: &str) -> String {
            self.dir.join(name).to_string_lossy().into_owned()
        }
    }

    /// Um servidor de eco que faz o papel do rpc-server.
    async fn echo() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = s.read(&mut buf).await {
                        if n == 0 || s.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        addr
    }

    async fn start_server(pki: &Pki, upstream: std::net::SocketAddr) -> std::net::SocketAddr {
        let config =
            server_config(&pki.p("worker.crt"), &pki.p("worker.key"), &pki.p("ca.crt")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve(listener, upstream.to_string(), config));
        addr
    }

    async fn start_client(
        pki: &Pki,
        remote: std::net::SocketAddr,
        who: &str,
    ) -> std::net::SocketAddr {
        let config = client_config(
            &pki.p("ca.crt"),
            &pki.p(&format!("{who}.crt")),
            &pki.p(&format!("{who}.key")),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(forward(
            listener,
            remote.to_string(),
            "aic-worker".into(),
            config,
        ));
        addr
    }

    async fn roundtrip(addr: std::net::SocketAddr) -> Option<Vec<u8>> {
        let mut s = TcpStream::connect(addr).await.ok()?;
        s.write_all(b"ola rpc").await.ok()?;
        let mut buf = vec![0u8; 7];
        tokio::time::timeout(Duration::from_secs(3), s.read_exact(&mut buf))
            .await
            .ok()?
            .ok()?;
        Some(buf)
    }

    #[tokio::test]
    async fn bytes_cross_the_mtls_tunnel_in_both_directions() {
        let pki = pki("ok");
        let server = start_server(&pki, echo().await).await;
        let client = start_client(&pki, server, "gateway").await;
        assert_eq!(roundtrip(client).await.as_deref(), Some(&b"ola rpc"[..]));
    }

    #[tokio::test]
    async fn a_client_certificate_from_another_ca_is_refused() {
        let pki = pki("evil");
        let server = start_server(&pki, echo().await).await;
        // O intruso confia na CA certa (para validar o servidor), mas o certificado dele é de outra.
        let client = start_client(&pki, server, "intruso").await;
        assert_eq!(roundtrip(client).await, None);
    }

    #[tokio::test]
    async fn plain_tcp_gets_no_access_to_the_upstream() {
        let pki = pki("plain");
        let server = start_server(&pki, echo().await).await;
        let mut s = TcpStream::connect(server).await.unwrap();
        s.write_all(b"ola rpc").await.unwrap();
        let mut buf = vec![0u8; 7];
        let got = tokio::time::timeout(Duration::from_secs(3), s.read_exact(&mut buf)).await;
        // Ou o servidor fecha, ou nada volta; o eco nunca chega.
        assert!(!matches!(got, Ok(Ok(_))) || buf != b"ola rpc");
    }

    #[tokio::test]
    async fn the_server_name_is_checked() {
        let pki = pki("name");
        let server = start_server(&pki, echo().await).await;
        let config = client_config(
            &pki.p("ca.crt"),
            &pki.p("gateway.crt"),
            &pki.p("gateway.key"),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(forward(
            listener,
            server.to_string(),
            "outro-nome".into(),
            config,
        ));
        assert_eq!(roundtrip(addr).await, None);
    }
}
