//! Aparelhos que aguardam aprovação para instalar o AI Cluster OS (ADR-023).
//!
//! A etapa de espera se apresenta sem credencial. Por isso tudo aqui é desconfiado: a lista tem
//! teto, cada IP tem cota, as entradas vencem, os textos são limpos e a impressão digital deriva de
//! um segredo que só o aparelho conhece. O token de instalação só sai depois do operador aprovar, e
//! uma única vez, para quem repete o segredo.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard};

use ai_cluster_protocol::cluster::enrollment_server::Enrollment;
use ai_cluster_protocol::cluster::{
    AnnounceRequest, AnnounceResponse, ClaimInstallRequest, ClaimInstallResponse, InstallDisk,
    PendingInstall,
};
use sha2::{Digest, Sha256};
use tonic::{Request, Response, Status};

use crate::registry::Registry;

/// Aparelhos na lista ao mesmo tempo; acima disso quem chega é ignorado até alguém sair.
pub const MAX_PENDING: usize = 32;
/// Cota de um IP: um aparelho com defeito (ou malicioso) não ocupa a lista toda.
pub const MAX_PER_HOST: usize = 4;
/// Sem notícias por este tempo a entrada some (15 min).
pub const TTL_MS: u64 = 15 * 60_000;
/// De quanto em quanto tempo o aparelho volta a perguntar.
pub const RETRY_SECS: u32 = 5;
const MAX_DISKS: usize = 16;
/// Quanto tempo o id fica reservado para o aparelho que consumiu o token (2 min). Curto de propósito:
/// quem tem o join token pode registrar o id antes; a credencial de registro amarrada ao aparelho não
/// é viável sem mudar o worker (limite documentado no ADR-023).
pub const PREAPPROVAL_MS: u64 = 2 * 60_000;
/// Teto da janela em que a porta fica aberta.
pub const MAX_OPEN_MINUTES: u32 = 120;

#[derive(Clone, Debug)]
struct Candidate {
    info: PendingInstall,
    /// A cota é por balde: o IPv4 inteiro, ou o /64 de um IPv6 (quem tem uma sub-rede dessas tem
    /// bilhões de endereços). Endereço desconhecido cai num balde só, em vez de escapar da cota.
    bucket: Option<IpAddr>,
    /// Token de instalação, dado ao dono do segredo depois da aprovação.
    token: Option<String>,
    /// O token foi consumido pelo `ClaimInstall` (uso único): a entrada fica só para recusar o reuso.
    claimed: bool,
}

#[derive(Debug, Default)]
pub struct Enrollments {
    waiting: Mutex<HashMap<String, Candidate>>,
    /// Até quando a porta de espera aceita aparelhos (ms desde a época; 0 = fechada). Fechada é o
    /// normal: o operador abre por uma janela e ela se fecha sozinha, mesmo que ele esqueça.
    open_until: std::sync::atomic::AtomicU64,
}

fn hash(secret: &[u8]) -> [u8; 32] {
    Sha256::digest(secret).into()
}

/// O hash inteiro em hex: é a chave da lista, então dois segredos nunca dividem uma entrada.
fn key_of(secret_hash: &[u8; 32]) -> String {
    secret_hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// O que se mostra e se digita: três grupos de 4 hex (`a1b2-c3d4-e5f6`, 48 bits do hash). Curto para
/// conferir a olho; uma colisão com outra entrada é recusada.
fn fingerprint_of(key: &str) -> String {
    key.as_bytes()[..12]
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("-")
}

fn bucket_of(host: Option<IpAddr>) -> Option<IpAddr> {
    match host? {
        IpAddr::V6(v6) if v6.to_ipv4_mapped().is_none() => {
            let mut octets = v6.octets();
            octets[8..].fill(0);
            Some(IpAddr::V6(octets.into()))
        }
        other => Some(other),
    }
}

fn normalize(fingerprint: &str) -> String {
    fingerprint.trim().to_ascii_lowercase()
}

fn new_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("sem fonte de entropia para o token de instalação");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Texto que vem do aparelho: só ASCII imprimível e curto. Controle falsificaria linhas no terminal
/// do operador, e os invisíveis do Unicode (inversão de direção, separadores) esconderiam trechos.
fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| c.is_ascii() && !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

fn valid_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Nome de worker dado no claim: o mesmo conjunto de caracteres que o id tem no registro e nos logs.
fn valid_node_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn valid_disk_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

impl Enrollments {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Candidate>> {
        self.waiting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// O aparelho se apresenta (ou repete a pergunta). `Err` é um motivo curto para o aparelho.
    pub fn announce(
        &self,
        request: AnnounceRequest,
        host: Option<IpAddr>,
        now: u64,
    ) -> Result<AnnounceResponse, Status> {
        self.announce_if(request, host, now, true)
    }

    /// Como `announce`, mas com `allow_new` falso um aparelho que ainda não está na lista é recusado:
    /// a janela fecha só as entradas novas; quem já está na lista continua recebendo a resposta.
    pub fn announce_if(
        &self,
        request: AnnounceRequest,
        host: Option<IpAddr>,
        now: u64,
        allow_new: bool,
    ) -> Result<AnnounceResponse, Status> {
        if !(16..=64).contains(&request.secret.len()) {
            return Err(Status::invalid_argument(
                "segredo deve ter de 16 a 64 bytes",
            ));
        }
        let mac = request.mac.to_ascii_lowercase();
        if !valid_mac(&mac) {
            return Err(Status::invalid_argument("MAC inválido"));
        }
        let disks: Vec<InstallDisk> = request
            .disks
            .iter()
            .filter(|disk| valid_disk_name(&disk.name))
            .take(MAX_DISKS)
            .map(|disk| InstallDisk {
                name: disk.name.clone(),
                size_bytes: disk.size_bytes,
                model: clean(&disk.model, 40),
                removable: disk.removable,
            })
            .collect();
        let secret_hash = hash(&request.secret);
        let key = key_of(&secret_hash);
        let fingerprint = fingerprint_of(&key);
        let bucket = bucket_of(host);
        let mut waiting = self.lock();
        waiting.retain(|_, c| now.saturating_sub(c.info.last_seen_unix_ms) <= TTL_MS);

        if let Some(known) = waiting.get_mut(&key) {
            // A chave é o hash inteiro: quem chega aqui repetiu o segredo, é o mesmo aparelho.
            known.info.last_seen_unix_ms = now;
            known.bucket = bucket;
            if known.claimed {
                return Err(Status::failed_precondition(
                    "este aparelho já consumiu o token de instalação",
                ));
            }
            // O token fica disponível ao dono do segredo até a entrada vencer: se a resposta se
            // perder, a próxima pergunta o traz de novo. Quem o consumir o invalida (etapa futura).
            return Ok(AnnounceResponse {
                fingerprint,
                approved: !known.info.approved_disk.is_empty(),
                install_token: known.token.clone().unwrap_or_default(),
                disk: known.info.approved_disk.clone(),
                retry_after_secs: if known.token.is_some() { 0 } else { RETRY_SECS },
            });
        }

        if !allow_new {
            return Err(Status::unavailable(
                "instalação fechada: o operador abre com `aic cluster install open`",
            ));
        }
        // Dois hashes com o mesmo prefixo mostrado seriam indistinguíveis na tela do operador.
        if waiting.values().any(|c| c.info.fingerprint == fingerprint) {
            return Err(Status::already_exists("impressão digital em uso; reinicie"));
        }
        // Um MAC já aprovado não perde a vaga para quem o copia (vence em 15 min).
        if waiting
            .values()
            .any(|c| c.info.mac == mac && !c.info.approved_disk.is_empty())
        {
            return Err(Status::already_exists("este MAC já foi aprovado"));
        }
        // Reboot do mesmo aparelho: outro segredo, mesmo MAC e mesmo balde. A entrada velha sai. De
        // outro endereço ninguém despeja a entrada alheia só por saber o MAC, que é público na rede.
        waiting.retain(|_, c| !(c.info.mac == mac && c.bucket == bucket));
        if waiting.len() >= MAX_PENDING {
            return Err(Status::resource_exhausted("lista de aprovação cheia"));
        }
        if waiting.values().filter(|c| c.bucket == bucket).count() >= MAX_PER_HOST {
            return Err(Status::resource_exhausted(
                "aparelhos demais neste endereço",
            ));
        }
        waiting.insert(
            key,
            Candidate {
                info: PendingInstall {
                    fingerprint: fingerprint.clone(),
                    mac,
                    host: host.map(|h| h.to_string()).unwrap_or_default(),
                    arch: clean(&request.arch, 16),
                    firmware: clean(&request.firmware, 16),
                    cpu_model: clean(&request.cpu_model, 64),
                    cpu_cores: request.cpu_cores,
                    ram_bytes: request.ram_bytes,
                    disks,
                    first_seen_unix_ms: now,
                    last_seen_unix_ms: now,
                    approved_disk: String::new(),
                },
                bucket,
                token: None,
                claimed: false,
            },
        );
        Ok(AnnounceResponse {
            fingerprint,
            retry_after_secs: RETRY_SECS,
            ..Default::default()
        })
    }

    /// Abre a porta por `minutes` (0 fecha) e devolve até quando; o teto é `MAX_OPEN_MINUTES`.
    pub fn open_for(&self, minutes: u32, now: u64) -> u64 {
        let until = if minutes == 0 {
            0
        } else {
            now + u64::from(minutes.min(MAX_OPEN_MINUTES)) * 60_000
        };
        self.open_until
            .store(until, std::sync::atomic::Ordering::Relaxed);
        until
    }

    pub fn is_open(&self, now: u64) -> bool {
        now < self.open_until.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn list(&self, now: u64) -> Vec<PendingInstall> {
        let mut list: Vec<PendingInstall> = self
            .lock()
            .values()
            .filter(|c| !c.claimed && now.saturating_sub(c.info.last_seen_unix_ms) <= TTL_MS)
            .map(|c| c.info.clone())
            .collect();
        list.sort_by_key(|a| a.first_seen_unix_ms);
        list
    }

    /// Aprova a instalação apagando `disk`, que precisa ser um dos que o aparelho informou. Disco
    /// removível (pendrive, USB) só com `force_removable`: pode ser o pendrive do próprio operador.
    /// `Ok(false)`: a impressão digital não está (mais) na lista.
    pub fn approve(
        &self,
        fingerprint: &str,
        disk: &str,
        force_removable: bool,
        now: u64,
    ) -> Result<bool, Status> {
        let mut waiting = self.lock();
        let wanted = normalize(fingerprint);
        let Some(candidate) = waiting
            .values_mut()
            .find(|c| c.info.fingerprint == wanted)
            .filter(|c| !c.claimed && now.saturating_sub(c.info.last_seen_unix_ms) <= TTL_MS)
        else {
            return Ok(false);
        };
        let Some(found) = candidate.info.disks.iter().find(|d| d.name == disk) else {
            return Err(Status::invalid_argument(
                "o disco não é um dos que o aparelho informou",
            ));
        };
        if found.removable && !force_removable {
            return Err(Status::failed_precondition(format!(
                "o disco {disk} é removível (pendrive ou USB): só apaga com --force-removable"
            )));
        }
        if !candidate.info.approved_disk.is_empty() && candidate.info.approved_disk != disk {
            return Err(Status::already_exists(format!(
                "já aprovado com o disco {}; recuse e aprove de novo",
                candidate.info.approved_disk
            )));
        }
        candidate.info.approved_disk = disk.to_string();
        candidate.token.get_or_insert_with(new_token);
        Ok(true)
    }

    /// Consome o token de instalação (uso único) e devolve o id do worker, já pré-aprovado. `reserve`
    /// pré-aprova o id no registro: se recusar (id com dono), o token não é gasto. Tudo sob o lock desta
    /// lista, então dois claims com o mesmo token não passam juntos.
    pub fn claim(
        &self,
        token: &str,
        node_name: &str,
        now: u64,
        reserve: impl FnOnce(&str) -> bool,
    ) -> Result<String, Status> {
        if !node_name.is_empty() && !valid_node_name(node_name) {
            return Err(Status::invalid_argument(
                "nome de worker: até 64 letras, números, ponto, traço ou sublinhado",
            ));
        }
        let mut waiting = self.lock();
        let Some(candidate) = waiting
            .values_mut()
            .find(|c| c.token.as_deref() == Some(token))
        else {
            return Err(Status::not_found(
                "token de instalação inválido, expirado ou já usado",
            ));
        };
        if candidate.claimed {
            return Err(Status::failed_precondition("token de instalação já usado"));
        }
        if now.saturating_sub(candidate.info.last_seen_unix_ms) > TTL_MS {
            return Err(Status::failed_precondition(
                "token de instalação expirado: o aparelho precisa se apresentar de novo",
            ));
        }
        // Sem nome, o id sai do MAC (`x86-` + os três últimos octetos, sem dois-pontos).
        let node_id = if node_name.is_empty() {
            let tail: String = candidate.info.mac.split(':').skip(3).collect();
            format!("x86-{tail}")
        } else {
            node_name.to_string()
        };
        if !reserve(&node_id) {
            return Err(Status::already_exists(
                "id de worker já em uso; dê outro nome ao claim",
            ));
        }
        candidate.claimed = true;
        Ok(node_id)
    }

    pub fn reject(&self, fingerprint: &str) -> bool {
        let mut waiting = self.lock();
        let wanted = normalize(fingerprint);
        let before = waiting.len();
        waiting.retain(|_, c| c.info.fingerprint != wanted);
        waiting.len() != before
    }
}

/// O que o `ClaimInstall` entrega e em que condição: o join token só sai por uma porta TLS, ou em claro
/// se o operador disser `AI_CLUSTER_ALLOW_PLAIN_CLAIM=1`.
#[derive(Debug, Clone)]
pub struct ClaimSetup {
    /// URL do controller que o worker usa (`AI_CLUSTER_PUBLIC_URL`). Sem ela o claim é recusado.
    pub public_url: Option<String>,
    pub join_token: String,
    /// A porta de espera tem TLS (certificado do controller).
    pub tls: bool,
    /// `AI_CLUSTER_ALLOW_PLAIN_CLAIM=1`: aceita o claim sem TLS, com aviso no log a cada uso.
    pub plain_allowed: bool,
}

/// O lado sem credencial: `Announce` e `ClaimInstall`, na porta própria.
#[derive(Debug)]
pub struct EnrollmentService {
    enrollments: Arc<Enrollments>,
    registry: Arc<Registry>,
    setup: ClaimSetup,
}

impl EnrollmentService {
    pub fn new(enrollments: Arc<Enrollments>, registry: Arc<Registry>, setup: ClaimSetup) -> Self {
        Self {
            enrollments,
            registry,
            setup,
        }
    }
}

#[tonic::async_trait]
impl Enrollment for EnrollmentService {
    async fn announce(
        &self,
        request: Request<AnnounceRequest>,
    ) -> Result<Response<AnnounceResponse>, Status> {
        let host = request.remote_addr().map(|addr| addr.ip().to_canonical());
        let now = crate::registry::unix_ms();
        let open = self.enrollments.is_open(now);
        let response = self
            .enrollments
            .announce_if(request.into_inner(), host, now, open)?;
        Ok(Response::new(response))
    }

    async fn claim_install(
        &self,
        request: Request<ClaimInstallRequest>,
    ) -> Result<Response<ClaimInstallResponse>, Status> {
        let request = request.into_inner();
        // Tudo isto antes de gastar o token: recusado aqui, o token continua valendo.
        if !self.setup.tls {
            if !self.setup.plain_allowed {
                return Err(Status::failed_precondition(
                    "porta de espera sem TLS: o claim entregaria o join token em claro (use TLS ou AI_CLUSTER_ALLOW_PLAIN_CLAIM=1)",
                ));
            }
            println!(
                "AVISO: ClaimInstall sem TLS (AI_CLUSTER_ALLOW_PLAIN_CLAIM=1): o join token sai em claro"
            );
        }
        let Some(url) = self.setup.public_url.clone() else {
            return Err(Status::failed_precondition(
                "controller sem AI_CLUSTER_PUBLIC_URL: o aparelho não sabe onde se ligar",
            ));
        };
        let now = crate::registry::unix_ms();
        let node_id =
            self.enrollments
                .claim(request.token.trim(), request.node_name.trim(), now, |id| {
                    self.registry.pre_approve(id, now, now + PREAPPROVAL_MS)
                })?;
        Ok(Response::new(ClaimInstallResponse {
            controller_url: url,
            join_token: self.setup.join_token.clone(),
            node_id,
        }))
    }
}

/// Conexões abertas ao mesmo tempo na porta anônima. Passando disso a nova é fechada na hora: o
/// descritor de arquivo é do processo todo, e esta porta não pode tirá-los do plano de controle.
pub const MAX_CONNECTIONS: usize = 64;
/// Vida máxima de uma conexão: quem conecta e cala, ou fica lendo devagar, não segura a vaga.
const CONNECTION_LIFETIME: std::time::Duration = std::time::Duration::from_secs(120);

/// Uma conexão que segura uma vaga do teto e morre depois de `CONNECTION_LIFETIME`.
pub struct Limited<IO> {
    io: IO,
    deadline: std::pin::Pin<Box<tokio::time::Sleep>>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

fn expired() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::TimedOut, "conexão passou do prazo")
}

impl<IO: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Limited<IO> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        // Consultar o prazo aqui também registra o waker: a leitura acorda quando ele vence.
        if std::future::Future::poll(self.deadline.as_mut(), cx).is_ready() {
            return std::task::Poll::Ready(Err(expired()));
        }
        std::pin::Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl<IO: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Limited<IO> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.io).poll_write(cx, data)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

impl<IO: tonic::transport::server::Connected> tonic::transport::server::Connected for Limited<IO> {
    type ConnectInfo = IO::ConnectInfo;

    fn connect_info(&self) -> Self::ConnectInfo {
        self.io.connect_info()
    }
}

/// As conexões do `listener`, no máximo `MAX_CONNECTIONS` vivas, cada uma com prazo de vida.
pub fn limited_incoming(
    listener: tokio::net::TcpListener,
) -> impl tokio_stream::Stream<Item = std::io::Result<Limited<tokio::net::TcpStream>>> {
    use tokio_stream::StreamExt;
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    tokio_stream::wrappers::TcpListenerStream::new(listener)
        // Erro de accept (ex.: EMFILE) não pode virar laço quente: espera antes de tentar de novo.
        .then(|accepted| async move {
            if accepted.is_err() {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            accepted
        })
        .filter_map(move |accepted| match accepted {
            Ok(io) => slots.clone().try_acquire_owned().ok().map(|permit| {
                Ok(Limited {
                    io,
                    deadline: Box::pin(tokio::time::sleep(CONNECTION_LIFETIME)),
                    _permit: permit,
                })
            }),
            Err(error) => Some(Err(error)),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(name: &str) -> InstallDisk {
        InstallDisk {
            name: name.into(),
            size_bytes: 16_000_000_000,
            model: "SD".into(),
            removable: false,
        }
    }

    #[test]
    fn disco_removivel_so_e_apagado_com_force_removable() {
        let e = Enrollments::default();
        let mut pen = hello(1, MAC);
        pen.disks[0].removable = true;
        pen.disks.push(disk("sda"));
        let fp = e.announce(pen, None, 0).unwrap().fingerprint;
        let recusa = e.approve(&fp, "mmcblk0", false, 1).unwrap_err();
        assert_eq!(recusa.code(), tonic::Code::FailedPrecondition);
        assert!(recusa.message().contains("--force-removable"));
        // O interno continua aprovável sem a permissão.
        assert!(e.approve(&fp, "sda", false, 2).unwrap());
        let mut so_pen = hello(2, "aa:bb:cc:dd:ee:02");
        so_pen.disks[0].removable = true;
        let fp2 = e.announce(so_pen, None, 3).unwrap().fingerprint;
        assert!(e.approve(&fp2, "mmcblk0", true, 4).unwrap());
    }

    fn hello(secret: u8, mac: &str) -> AnnounceRequest {
        AnnounceRequest {
            secret: vec![secret; 32],
            mac: mac.into(),
            arch: "armv7".into(),
            firmware: "uboot".into(),
            cpu_model: "Cortex-A7".into(),
            cpu_cores: 4,
            ram_bytes: 1 << 30,
            disks: vec![disk("mmcblk0")],
        }
    }

    const MAC: &str = "aa:bb:cc:dd:ee:01";

    #[test]
    fn o_aparelho_aparece_pendente_e_so_recebe_o_token_depois_da_aprovacao() {
        let e = Enrollments::default();
        let first = e.announce(hello(1, MAC), None, 1_000).unwrap();
        assert!(!first.approved && first.install_token.is_empty());
        assert_eq!(e.list(1_000).len(), 1);
        assert_eq!(e.list(1_000)[0].fingerprint, first.fingerprint);

        assert!(
            e.approve(&first.fingerprint, "mmcblk0", false, 2_000)
                .unwrap()
        );
        // Outro segredo com o mesmo MAC (um intruso) não leva o token nem derruba a aprovação.
        let intruder = e.announce(hello(2, MAC), None, 3_000);
        assert_eq!(intruder.unwrap_err().code(), tonic::Code::AlreadyExists);
        let owner = e.announce(hello(1, MAC), None, 4_000).unwrap();
        assert!(owner.approved && owner.install_token.len() == 32);
    }

    #[test]
    fn o_dono_do_segredo_recebe_o_token_e_pode_pedir_de_novo() {
        let e = Enrollments::default();
        let fp = e.announce(hello(1, MAC), None, 0).unwrap().fingerprint;
        assert!(e.approve(&fp, "mmcblk0", false, 1).unwrap());
        let got = e.announce(hello(1, MAC), None, 2).unwrap();
        assert!(got.approved && got.disk == "mmcblk0");
        assert_eq!(got.install_token.len(), 32);
        // Resposta perdida: o dono do segredo recebe o mesmo token de novo.
        let again = e.announce(hello(1, MAC), None, 3).unwrap();
        assert_eq!(again.install_token, got.install_token);
        // Outro segredo nunca o recebe.
        assert!(
            e.announce(hello(2, "aa:bb:cc:dd:ee:02"), None, 4)
                .unwrap()
                .install_token
                .is_empty()
        );
    }

    #[test]
    fn o_disco_precisa_ser_um_dos_que_o_aparelho_informou() {
        let e = Enrollments::default();
        let fp = e.announce(hello(1, MAC), None, 0).unwrap().fingerprint;
        assert!(e.approve(&fp, "sda", false, 1).is_err());
        assert!(!e.approve("0000-0000-0000", "mmcblk0", false, 1).unwrap());
    }

    #[test]
    fn aprovar_de_novo_com_outro_disco_e_recusado_e_com_o_mesmo_e_idempotente() {
        let e = Enrollments::default();
        let mut two = hello(1, MAC);
        two.disks.push(disk("sda"));
        let fp = e.announce(two, None, 0).unwrap().fingerprint;
        assert!(e.approve(&fp, "mmcblk0", false, 1).unwrap());
        let token = e.announce(hello(1, MAC), None, 2).unwrap().install_token;
        let err = e.approve(&fp, "sda", false, 3).unwrap_err();
        assert_eq!(err.code(), tonic::Code::AlreadyExists);
        assert!(err.message().contains("já aprovado com o disco mmcblk0"));
        assert!(e.approve(&fp, "mmcblk0", false, 4).unwrap());
        assert_eq!(
            e.announce(hello(1, MAC), None, 5).unwrap().install_token,
            token
        );
    }

    #[test]
    fn com_a_porta_fechada_o_aprovado_recebe_o_token_e_o_novo_e_recusado() {
        let e = Enrollments::default();
        let fp = e.announce(hello(1, MAC), None, 0).unwrap().fingerprint;
        assert!(e.approve(&fp, "mmcblk0", false, 1).unwrap());
        // Operador aprovou e fechou (ou a janela venceu): o aparelho aprovado ainda recebe o token.
        let got = e.announce_if(hello(1, MAC), None, 2, false).unwrap();
        assert!(got.approved && got.install_token.len() == 32);
        // Aparelho novo, com a porta fechada, não entra na lista.
        let novo = e.announce_if(hello(2, "aa:bb:cc:dd:ee:02"), None, 3, false);
        assert_eq!(novo.unwrap_err().code(), tonic::Code::Unavailable);
        assert_eq!(e.list(3).len(), 1);
    }

    #[test]
    fn a_lista_tem_teto_cota_por_ip_e_validade() {
        let e = Enrollments::default();
        let host: IpAddr = "10.0.0.9".parse().unwrap();
        for i in 0..MAX_PER_HOST as u8 {
            e.announce(hello(i, &format!("aa:bb:cc:dd:ee:{i:02x}")), Some(host), 0)
                .unwrap();
        }
        let blocked = e.announce(hello(99, "aa:bb:cc:dd:ee:99"), Some(host), 0);
        assert_eq!(blocked.unwrap_err().code(), tonic::Code::ResourceExhausted);

        // Sem endereço conhecido todos dividem um balde só: não escapam da cota.
        let e = Enrollments::default();
        for i in 0..MAX_PER_HOST as u8 {
            e.announce(hello(i, &format!("aa:bb:cc:dd:ee:{i:02x}")), None, 0)
                .unwrap();
        }
        assert!(e.announce(hello(50, "aa:bb:cc:dd:ee:50"), None, 0).is_err());

        // Um /64 inteiro é um balde só.
        let e = Enrollments::default();
        for i in 0..MAX_PER_HOST as u8 {
            let host: IpAddr = format!("2001:db8:0:1::{:x}", i + 1).parse().unwrap();
            e.announce(hello(i, &format!("aa:bb:cc:dd:ee:{i:02x}")), Some(host), 0)
                .unwrap();
        }
        let sibling: IpAddr = "2001:db8:0:1::ffff".parse().unwrap();
        assert!(
            e.announce(hello(60, "aa:bb:cc:dd:ee:60"), Some(sibling), 0)
                .is_err()
        );

        // Lista cheia de IPs diferentes: recusa, e a vaga volta quando as entradas vencem.
        let e = Enrollments::default();
        for i in 0..MAX_PENDING as u8 {
            let host: IpAddr = format!("10.0.0.{}", i + 1).parse().unwrap();
            e.announce(hello(i, &format!("aa:bb:cc:dd:ee:{i:02x}")), Some(host), 0)
                .unwrap();
        }
        let other: IpAddr = "10.0.1.1".parse().unwrap();
        assert!(
            e.announce(hello(200, "aa:bb:cc:dd:ff:00"), Some(other), 1)
                .is_err()
        );
        assert!(
            e.announce(hello(200, "aa:bb:cc:dd:ff:00"), Some(other), TTL_MS + 1)
                .is_ok()
        );
        assert_eq!(e.list(TTL_MS + 1).len(), 1);
    }

    #[test]
    fn outro_endereco_nao_despeja_a_entrada_so_por_saber_o_mac() {
        let e = Enrollments::default();
        let (a, b): (IpAddr, IpAddr) = ("10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap());
        e.announce(hello(1, MAC), Some(a), 0).unwrap();
        e.announce(hello(2, MAC), Some(b), 1).unwrap();
        assert_eq!(e.list(1).len(), 2, "o intruso não apaga a entrada do dono");
        // O reboot do dono, do mesmo endereço, troca a entrada dele.
        e.announce(hello(3, MAC), Some(a), 2).unwrap();
        assert_eq!(e.list(2).len(), 2);
    }

    #[test]
    fn o_reboot_do_mesmo_mac_troca_a_entrada_velha() {
        let e = Enrollments::default();
        e.announce(hello(1, MAC), None, 0).unwrap();
        e.announce(hello(2, MAC), None, 1).unwrap();
        assert_eq!(e.list(1).len(), 1);
    }

    #[test]
    fn entradas_invalidas_sao_recusadas_e_o_texto_vem_limpo() {
        let e = Enrollments::default();
        assert!(e.announce(hello(1, "nada"), None, 0).is_err());
        let mut short = hello(1, MAC);
        short.secret = vec![1; 4];
        assert!(e.announce(short, None, 0).is_err());
        let mut dirty = hello(1, MAC);
        dirty.cpu_model = "A\u{1b}[31mB\nC\u{202e}D".into();
        dirty.disks.push(disk("../etc"));
        e.announce(dirty, None, 0).unwrap();
        let listed = e.list(0);
        assert_eq!(listed[0].cpu_model, "A[31mBCD");
        assert_eq!(
            listed[0].disks.len(),
            1,
            "nome de disco inválido é descartado"
        );
    }

    #[test]
    fn recusar_tira_da_lista() {
        let e = Enrollments::default();
        let fp = e.announce(hello(1, MAC), None, 0).unwrap().fingerprint;
        assert!(e.reject(&fp));
        assert!(!e.reject(&fp));
        assert!(e.list(0).is_empty());
    }

    #[test]
    fn a_janela_abre_por_minutos_tem_teto_e_fecha_sozinha() {
        let e = Enrollments::default();
        assert!(!e.is_open(1_000), "fechada é o normal");
        let until = e.open_for(5, 1_000);
        assert_eq!(until, 1_000 + 5 * 60_000);
        assert!(e.is_open(until - 1) && !e.is_open(until));
        assert_eq!(e.open_for(10_000, 0), u64::from(MAX_OPEN_MINUTES) * 60_000);
        e.open_for(0, 2_000);
        assert!(!e.is_open(2_001));
    }

    #[tokio::test]
    async fn a_porta_anonima_atende_o_announce_e_enxerga_o_endereco_do_cliente() {
        use ai_cluster_protocol::cluster::enrollment_client::EnrollmentClient;
        use ai_cluster_protocol::cluster::enrollment_server::EnrollmentServer;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let enrollments = Arc::new(Enrollments::default());
        enrollments.open_for(5, crate::registry::unix_ms());
        let service = EnrollmentServer::new(EnrollmentService::new(
            enrollments.clone(),
            Arc::new(Registry::new()),
            ClaimSetup {
                public_url: None,
                join_token: String::new(),
                tls: false,
                plain_allowed: false,
            },
        ));
        tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(service)
                .serve_with_incoming(limited_incoming(listener)),
        );
        let mut client = EnrollmentClient::connect(format!("http://{addr}"))
            .await
            .unwrap();
        // Fechada (o normal): o aparelho recebe "indisponível" e nada entra na lista.
        let closed = Arc::new(Enrollments::default());
        assert!(!closed.is_open(crate::registry::unix_ms()));
        let reply = client.announce(hello(1, MAC)).await.unwrap().into_inner();
        assert!(!reply.approved);
        let listed = enrollments.list(crate::registry::unix_ms());
        assert_eq!(listed[0].fingerprint, reply.fingerprint);
        assert_eq!(listed[0].host, "127.0.0.1");
    }

    /// Anuncia, aprova e devolve o token de instalação que o aparelho receberia.
    fn aprovado(e: &Enrollments, secret: u8, mac: &str, now: u64) -> String {
        let fp = e
            .announce(hello(secret, mac), None, now)
            .unwrap()
            .fingerprint;
        assert!(e.approve(&fp, "mmcblk0", false, now).unwrap());
        e.announce(hello(secret, mac), None, now)
            .unwrap()
            .install_token
    }

    #[test]
    fn o_token_de_instalacao_vale_uma_vez() {
        let e = Enrollments::default();
        let token = aprovado(&e, 1, MAC, 1_000);
        let mut reserved = Vec::new();
        let node = e
            .claim(&token, "", 2_000, |id| {
                reserved.push(id.to_string());
                true
            })
            .unwrap();
        assert_eq!(node, "x86-ddee01");
        assert_eq!(reserved, ["x86-ddee01"]);
        let again = e.claim(&token, "", 3_000, |_| true).unwrap_err();
        assert_eq!(again.code(), tonic::Code::FailedPrecondition);
        // O aparelho que já consumiu não recebe o token de novo nem volta à lista.
        let back = e.announce(hello(1, MAC), None, 4_000).unwrap_err();
        assert_eq!(back.code(), tonic::Code::FailedPrecondition);
        assert!(e.list(4_000).is_empty());
    }

    #[test]
    fn token_vencido_ou_inventado_nao_reserva_nem_gasta_nada() {
        let e = Enrollments::default();
        let token = aprovado(&e, 1, MAC, 1_000);
        let late = 1_000 + TTL_MS + 1;
        let expired = e
            .claim(&token, "", late, |_| {
                panic!("não reserva com token vencido")
            })
            .unwrap_err();
        assert_eq!(expired.code(), tonic::Code::FailedPrecondition);
        let made_up = e.claim("00", "", 2_000, |_| true).unwrap_err();
        assert_eq!(made_up.code(), tonic::Code::NotFound);
    }

    #[test]
    fn id_recusado_pela_reserva_nao_gasta_o_token_e_nome_invalido_nem_chega_la() {
        let e = Enrollments::default();
        let token = aprovado(&e, 1, MAC, 1_000);
        let taken = e.claim(&token, "lab-1", 2_000, |_| false).unwrap_err();
        assert_eq!(taken.code(), tonic::Code::AlreadyExists);
        let bad = e.claim(&token, "../x", 2_000, |_| true).unwrap_err();
        assert_eq!(bad.code(), tonic::Code::InvalidArgument);
        // Com outro nome o mesmo token ainda serve.
        assert_eq!(
            e.claim(&token, "lab-2", 3_000, |id| id == "lab-2").unwrap(),
            "lab-2"
        );
    }

    #[tokio::test]
    async fn claim_install_entrega_url_join_e_id_pre_aprovado_sem_url_recusa() {
        let enrollments = Arc::new(Enrollments::default());
        let registry = Arc::new(Registry::requiring_approval());
        let now = crate::registry::unix_ms();
        let token = aprovado(&enrollments, 1, MAC, now);
        let claim = |url: Option<&str>| {
            EnrollmentService::new(
                enrollments.clone(),
                registry.clone(),
                ClaimSetup {
                    public_url: url.map(String::from),
                    join_token: "join-secreto".into(),
                    tls: true,
                    plain_allowed: false,
                },
            )
        };
        let request = || {
            Request::new(ClaimInstallRequest {
                token: token.clone(),
                node_name: String::new(),
            })
        };
        // Sem URL o claim é recusado antes de gastar o token.
        let refused = claim(None).claim_install(request()).await.unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        let got = claim(Some("https://ctl:50051"))
            .claim_install(request())
            .await
            .unwrap()
            .into_inner();
        assert_eq!(got.controller_url, "https://ctl:50051");
        assert_eq!(got.join_token, "join-secreto");
        assert_eq!(got.node_id, "x86-ddee01");
        let again = claim(Some("https://ctl:50051"))
            .claim_install(request())
            .await
            .unwrap_err();
        assert_eq!(again.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn claim_sem_tls_so_sai_com_a_permissao_e_recusa_sem_gastar_o_token() {
        let enrollments = Arc::new(Enrollments::default());
        let registry = Arc::new(Registry::requiring_approval());
        let now = crate::registry::unix_ms();
        let plain = aprovado(&enrollments, 1, MAC, now);
        let tls = aprovado(&enrollments, 2, "aa:bb:cc:dd:ee:02", now);
        let setup = |tls: bool, plain_allowed: bool| ClaimSetup {
            public_url: Some("https://ctl:50051".into()),
            join_token: "join-secreto".into(),
            tls,
            plain_allowed,
        };
        let service = |setup: ClaimSetup| {
            EnrollmentService::new(enrollments.clone(), registry.clone(), setup)
        };
        let request = |token: &str| {
            Request::new(ClaimInstallRequest {
                token: token.into(),
                node_name: String::new(),
            })
        };
        // Porta sem TLS e sem a permissão: recusa, e o token continua valendo.
        let refused = service(setup(false, false))
            .claim_install(request(&plain))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert!(refused.message().contains("em claro"));
        // Com a permissão explícita, o mesmo token sai (com aviso no log).
        let ok = service(setup(false, true))
            .claim_install(request(&plain))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(ok.join_token, "join-secreto");
        // Com TLS não precisa de permissão.
        let ok_tls = service(setup(true, false))
            .claim_install(request(&tls))
            .await
            .unwrap();
        assert_eq!(ok_tls.into_inner().node_id, "x86-ddee02");
    }
}
