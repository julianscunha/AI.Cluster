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

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use rustls::client::verify_server_name;
use rustls::crypto::{CryptoProvider, ring};
use rustls::pki_types::UnixTime;
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::server::{ParsedCertificate, WebPkiClientVerifier};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf, copy_bidirectional};
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

/// Aceita só certificados de cliente da CA **e** com o papel combinado no SAN (`aic-gateway`). Com uma
/// CA só, o certificado de um worker (SAN `aic-worker`) deixa de servir de cliente: um worker
/// comprometido não abre o `rpc-server` de outro.
#[derive(Debug)]
struct RoleVerifier {
    inner: Arc<dyn ClientCertVerifier>,
    role: ServerName<'static>,
}

impl ClientCertVerifier for RoleVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.inner.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        let verified = self
            .inner
            .verify_client_cert(end_entity, intermediates, now)?;
        let parsed = ParsedCertificate::try_from(end_entity)?;
        verify_server_name(&parsed, &self.role)?;
        Ok(verified)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// A configuração do túnel servidor: certificado do worker, CA que assina os clientes e o papel
/// (`client_role`, um nome no SAN) que o certificado do cliente precisa ter. Sem certificado de
/// cliente válido e com o papel, o handshake falha.
pub fn server_config(
    cert: &str,
    key: &str,
    client_ca: &str,
    client_role: &str,
) -> Result<Arc<ServerConfig>, String> {
    let role = ServerName::try_from(client_role.to_string())
        .map_err(|e| format!("papel do cliente {client_role}: {e}"))?;
    let chain = certs(cert)?;
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("chave {key}: {e}"))?;
    let verifier =
        WebPkiClientVerifier::builder_with_provider(Arc::new(roots(client_ca)?), provider())
            .build()
            .map_err(|e| format!("CA de clientes {client_ca}: {e}"))?;
    let verifier: Arc<dyn ClientCertVerifier> = Arc::new(RoleVerifier {
        inner: verifier,
        role,
    });
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

/// Tráfego do servidor do túnel, para o log: bytes que chegaram do gateway (`IN`) e que voltaram a ele
/// (`OUT`), e as sessões abertas agora. O `rpc-server` do llama.cpp só escreve no log em blocos, então é
/// por aqui que se vê, com horário, que o gateway está falando com este worker.
static BYTES_IN: AtomicU64 = AtomicU64::new(0);
static BYTES_OUT: AtomicU64 = AtomicU64::new(0);
static SESSIONS: AtomicUsize = AtomicUsize::new(0);

/// Conta uma sessão aberta; a contagem volta quando o guarda sai de escopo, mesmo se a tarefa for
/// abortada no meio (sem isso a conta ficaria errada para sempre).
struct SessionGuard(&'static AtomicUsize);

impl SessionGuard {
    fn open(counter: &'static AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// Hora do dia em UTC (`HH:MM:SS`), para as linhas de log.
fn clock() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600 % 24,
        secs / 60 % 60,
        secs % 60
    )
}

fn kib(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / (1u64 << 20) as f64)
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

/// Conta os bytes que passam: o que se lê vai para `read`, o que se escreve para `written`.
struct Counted<T> {
    inner: T,
    read: &'static AtomicU64,
    written: &'static AtomicU64,
}

impl<T: AsyncRead + Unpin> AsyncRead for Counted<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &poll {
            self.read
                .fetch_add((buf.filled().len() - before) as u64, Ordering::Relaxed);
        }
        poll
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Counted<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let poll = Pin::new(&mut self.inner).poll_write(cx, data);
        if let Poll::Ready(Ok(n)) = &poll {
            self.written.fetch_add(*n as u64, Ordering::Relaxed);
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// A cada `REPORT_EVERY`, uma linha se houve tráfego: nada de ruído com o modelo parado.
async fn report_traffic() {
    let mut last = (0u64, 0u64);
    loop {
        tokio::time::sleep(REPORT_EVERY).await;
        let now = (
            BYTES_IN.load(Ordering::Relaxed),
            BYTES_OUT.load(Ordering::Relaxed),
        );
        if now != last {
            eprintln!(
                "túnel {}: gateway enviou {} e recebeu {} em {}s (sessões abertas: {})",
                clock(),
                kib(now.0 - last.0),
                kib(now.1 - last.1),
                REPORT_EVERY.as_secs(),
                SESSIONS.load(Ordering::Relaxed)
            );
            last = now;
        }
    }
}

/// Configuração TLS do servidor do túnel. Trocar o certificado é montar outra e enviá-la pelo `watch`.
pub type Tls = Arc<ServerConfig>;

/// Servidor: aceita TLS em `listener` e repassa cada conexão a `upstream` (o `rpc-server` local).
/// A configuração TLS vem de um `watch`: cada conexão nova usa a que estiver lá no momento, então um
/// certificado renovado (ADR-026) vale sem derrubar o túnel. Não retorna.
pub async fn serve(
    listener: TcpListener,
    upstream: String,
    config: tokio::sync::watch::Receiver<Arc<ServerConfig>>,
) -> std::io::Result<()> {
    let open = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let handshakes = Arc::new(tokio::sync::Semaphore::new(MAX_HANDSHAKES));
    tokio::spawn(report_traffic());
    loop {
        let (tcp, peer) = match listener.accept().await {
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
        let acceptor = TlsAcceptor::from(config.borrow().clone());
        let (upstream, open) = (upstream.clone(), open.clone());
        tokio::spawn(async move {
            let Ok(Ok(mut tls)) = tokio::time::timeout(SETUP, acceptor.accept(tcp)).await else {
                return;
            };
            // Só quem tem certificado válido ocupa uma das conexões abertas.
            drop(handshake);
            let Ok(_permit) = open.try_acquire_owned() else {
                return;
            };
            let local = match tokio::time::timeout(SETUP, TcpStream::connect(&upstream)).await {
                Ok(Ok(local)) => local,
                Ok(Err(error)) => {
                    eprintln!(
                        "túnel {}: gateway {} não alcançou o rpc-server: {error}",
                        clock(),
                        peer.ip()
                    );
                    return;
                }
                Err(_) => {
                    eprintln!(
                        "túnel {}: gateway {} não alcançou o rpc-server em {}s",
                        clock(),
                        peer.ip(),
                        SETUP.as_secs()
                    );
                    return;
                }
            };
            nodelay(&local);
            let mut local = Counted {
                inner: local,
                read: &BYTES_OUT,
                written: &BYTES_IN,
            };
            let started = std::time::Instant::now();
            let _session = SessionGuard::open(&SESSIONS);
            eprintln!("túnel {}: gateway {} conectou", clock(), peer.ip());
            let outcome = copy_bidirectional(&mut tls, &mut local).await;
            let secs = started.elapsed().as_secs();
            match outcome {
                Ok((to_rpc, to_gateway)) => eprintln!(
                    "túnel {}: gateway {} desconectou depois de {secs}s ({} de ida, {} de volta)",
                    clock(),
                    peer.ip(),
                    kib(to_rpc),
                    kib(to_gateway)
                ),
                Err(error) => eprintln!(
                    "túnel {}: gateway {} caiu depois de {secs}s: {error}",
                    clock(),
                    peer.ip()
                ),
            }
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

    fn watch_config(config: Arc<ServerConfig>) -> tokio::sync::watch::Receiver<Arc<ServerConfig>> {
        tokio::sync::watch::channel(config).1
    }

    async fn start_server(pki: &Pki, upstream: std::net::SocketAddr) -> std::net::SocketAddr {
        let config = server_config(
            &pki.p("worker.crt"),
            &pki.p("worker.key"),
            &pki.p("ca.crt"),
            "aic-gateway",
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve(listener, upstream.to_string(), watch_config(config)));
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
        let (before_in, before_out) = (
            BYTES_IN.load(Ordering::Relaxed),
            BYTES_OUT.load(Ordering::Relaxed),
        );
        assert_eq!(roundtrip(client).await.as_deref(), Some(&b"ola rpc"[..]));
        // O eco passou 7 bytes para dentro e 7 de volta: o log do worker vê o tráfego.
        assert!(BYTES_IN.load(Ordering::Relaxed) >= before_in + 7);
        assert!(BYTES_OUT.load(Ordering::Relaxed) >= before_out + 7);
        assert_eq!(clock().len(), 8);
        assert_eq!(kib(2048), "2.0 KiB");
    }

    #[tokio::test]
    async fn a_renewed_server_certificate_is_used_by_the_next_connection() {
        let pki = pki("reload");
        let good = server_config(
            &pki.p("worker.crt"),
            &pki.p("worker.key"),
            &pki.p("ca.crt"),
            "aic-gateway",
        )
        .unwrap();
        // Um certificado de outra CA: o gateway (que confia só na CA do cluster) tem de recusá-lo.
        let bad = server_config(
            &pki.p("intruso.crt"),
            &pki.p("intruso.key"),
            &pki.p("ca.crt"),
            "aic-gateway",
        )
        .unwrap();
        let (renew, current) = tokio::sync::watch::channel(good);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = listener.local_addr().unwrap();
        tokio::spawn(serve(listener, echo().await.to_string(), current));
        let client = start_client(&pki, server, "gateway").await;
        assert_eq!(
            roundtrip(client).await.as_deref(),
            Some(&b"ola rpc"[..]),
            "antes da troca, o certificado bom passa"
        );
        renew.send(bad).unwrap();
        assert_eq!(
            roundtrip(client).await,
            None,
            "depois da troca, o novo é o usado"
        );
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
    #[tokio::test]
    async fn a_worker_certificate_is_not_accepted_as_a_client() {
        let pki = pki("role");
        let server = start_server(&pki, echo().await).await;
        // Mesma CA, certificado válido, mas o papel é de worker (SAN aic-worker): recusado.
        let as_worker = start_client(&pki, server, "worker").await;
        assert_eq!(roundtrip(as_worker).await, None);
        // O do gateway segue valendo.
        let as_gateway = start_client(&pki, server, "gateway").await;
        assert_eq!(
            roundtrip(as_gateway).await.as_deref(),
            Some(&b"ola rpc"[..])
        );
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    #[test]
    fn o_guarda_devolve_a_conta_ao_sair_de_escopo() {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let guard = SessionGuard::open(&COUNT);
        assert_eq!(COUNT.load(Ordering::Relaxed), 1);
        drop(guard);
        assert_eq!(COUNT.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn a_tarefa_abortada_no_meio_tambem_devolve_a_conta() {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let task = tokio::spawn(async {
            let _session = SessionGuard::open(&COUNT);
            std::future::pending::<()>().await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(COUNT.load(Ordering::Relaxed), 1);
        task.abort();
        let _ = task.await;
        assert_eq!(COUNT.load(Ordering::Relaxed), 0);
    }
}
