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
    AnnounceRequest, AnnounceResponse, InstallDisk, PendingInstall,
};
use ai_cluster_protocol::metadata::same_token;
use sha2::{Digest, Sha256};
use tonic::{Request, Response, Status};

/// Aparelhos na lista ao mesmo tempo; acima disso quem chega é ignorado até alguém sair.
pub const MAX_PENDING: usize = 32;
/// Cota de um IP: um aparelho com defeito (ou malicioso) não ocupa a lista toda.
pub const MAX_PER_HOST: usize = 4;
/// Sem notícias por este tempo a entrada some (15 min).
pub const TTL_MS: u64 = 15 * 60_000;
/// De quanto em quanto tempo o aparelho volta a perguntar.
pub const RETRY_SECS: u32 = 5;
const MAX_DISKS: usize = 16;

#[derive(Clone, Debug)]
struct Candidate {
    info: PendingInstall,
    host: Option<IpAddr>,
    secret_hash: [u8; 32],
    /// Token já gerado e ainda não entregue.
    token: Option<String>,
}

#[derive(Debug, Default)]
pub struct Enrollments {
    waiting: Mutex<HashMap<String, Candidate>>,
}

fn hash(secret: &[u8]) -> [u8; 32] {
    Sha256::digest(secret).into()
}

/// Seis grupos de 4 hex (`a1b2-c3d4-…`): curto para conferir a olho, 48 bits do hash do segredo.
fn fingerprint_of(secret_hash: &[u8; 32]) -> String {
    let hex: String = secret_hash[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    hex.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("-")
}

fn new_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("sem fonte de entropia para o token de instalação");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Texto que vem do aparelho: sem controle (falsificaria linhas no terminal do operador) e curto.
fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
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
            })
            .collect();
        let secret_hash = hash(&request.secret);
        let fingerprint = fingerprint_of(&secret_hash);
        let mut waiting = self.lock();
        waiting.retain(|_, c| now.saturating_sub(c.info.last_seen_unix_ms) <= TTL_MS);

        if let Some(known) = waiting.get_mut(&fingerprint) {
            // Quem repete o segredo é o mesmo aparelho. Compara de novo em tempo constante: a
            // chave do mapa já é derivada do segredo, mas o token só sai para o dono dele.
            if same_token(&known.secret_hash, &secret_hash) {
                known.info.last_seen_unix_ms = now;
                known.host = host;
                return Ok(match known.token.take() {
                    Some(install_token) => AnnounceResponse {
                        fingerprint,
                        approved: true,
                        install_token,
                        disk: known.info.approved_disk.clone(),
                        retry_after_secs: 0,
                    },
                    None => AnnounceResponse {
                        fingerprint,
                        approved: !known.info.approved_disk.is_empty(),
                        retry_after_secs: RETRY_SECS,
                        ..Default::default()
                    },
                });
            }
        }

        // Aparelho novo. Um reboot gera outro segredo para o mesmo MAC: a entrada velha sai. A já
        // aprovada fica (vence em 15 min): senão quem copia o MAC derrubaria a aprovação.
        if waiting
            .values()
            .any(|c| c.info.mac == mac && !c.info.approved_disk.is_empty())
        {
            return Err(Status::already_exists("este MAC já foi aprovado"));
        }
        waiting.retain(|_, c| c.info.mac != mac);
        if waiting.len() >= MAX_PENDING {
            return Err(Status::resource_exhausted("lista de aprovação cheia"));
        }
        if let Some(host) = host
            && waiting.values().filter(|c| c.host == Some(host)).count() >= MAX_PER_HOST
        {
            return Err(Status::resource_exhausted(
                "aparelhos demais neste endereço",
            ));
        }
        waiting.insert(
            fingerprint.clone(),
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
                host,
                secret_hash,
                token: None,
            },
        );
        Ok(AnnounceResponse {
            fingerprint,
            retry_after_secs: RETRY_SECS,
            ..Default::default()
        })
    }

    pub fn list(&self, now: u64) -> Vec<PendingInstall> {
        let mut list: Vec<PendingInstall> = self
            .lock()
            .values()
            .filter(|c| now.saturating_sub(c.info.last_seen_unix_ms) <= TTL_MS)
            .map(|c| c.info.clone())
            .collect();
        list.sort_by_key(|a| a.first_seen_unix_ms);
        list
    }

    /// Aprova a instalação apagando `disk`, que precisa ser um dos que o aparelho informou.
    /// `Ok(false)`: a impressão digital não está (mais) na lista.
    pub fn approve(&self, fingerprint: &str, disk: &str, now: u64) -> Result<bool, Status> {
        let mut waiting = self.lock();
        let Some(candidate) = waiting
            .get_mut(&fingerprint.trim().to_ascii_lowercase())
            .filter(|c| now.saturating_sub(c.info.last_seen_unix_ms) <= TTL_MS)
        else {
            return Ok(false);
        };
        if !candidate.info.disks.iter().any(|d| d.name == disk) {
            return Err(Status::invalid_argument(
                "o disco não é um dos que o aparelho informou",
            ));
        }
        candidate.info.approved_disk = disk.to_string();
        candidate.token.get_or_insert_with(new_token);
        Ok(true)
    }

    pub fn reject(&self, fingerprint: &str) -> bool {
        self.lock()
            .remove(&fingerprint.trim().to_ascii_lowercase())
            .is_some()
    }
}

/// O lado sem credencial: só `Announce`, na porta própria.
#[derive(Debug)]
pub struct EnrollmentService {
    enrollments: Arc<Enrollments>,
}

impl EnrollmentService {
    pub fn new(enrollments: Arc<Enrollments>) -> Self {
        Self { enrollments }
    }
}

#[tonic::async_trait]
impl Enrollment for EnrollmentService {
    async fn announce(
        &self,
        request: Request<AnnounceRequest>,
    ) -> Result<Response<AnnounceResponse>, Status> {
        let host = request.remote_addr().map(|addr| addr.ip().to_canonical());
        let response =
            self.enrollments
                .announce(request.into_inner(), host, crate::registry::unix_ms())?;
        Ok(Response::new(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(name: &str) -> InstallDisk {
        InstallDisk {
            name: name.into(),
            size_bytes: 16_000_000_000,
            model: "SD".into(),
        }
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

        assert!(e.approve(&first.fingerprint, "mmcblk0", 2_000).unwrap());
        // Outro segredo com o mesmo MAC (um intruso) não leva o token nem derruba a aprovação.
        let intruder = e.announce(hello(2, MAC), None, 3_000);
        assert_eq!(intruder.unwrap_err().code(), tonic::Code::AlreadyExists);
        let owner = e.announce(hello(1, MAC), None, 4_000).unwrap();
        assert!(owner.approved && owner.install_token.len() == 32);
    }

    #[test]
    fn o_dono_do_segredo_recebe_o_token_uma_vez() {
        let e = Enrollments::default();
        let fp = e.announce(hello(1, MAC), None, 0).unwrap().fingerprint;
        assert!(e.approve(&fp, "mmcblk0", 1).unwrap());
        let got = e.announce(hello(1, MAC), None, 2).unwrap();
        assert!(got.approved && got.disk == "mmcblk0");
        assert_eq!(got.install_token.len(), 32);
        let second = e.announce(hello(1, MAC), None, 3).unwrap();
        assert!(second.install_token.is_empty(), "o token sai uma vez só");
        assert!(second.approved);
    }

    #[test]
    fn o_disco_precisa_ser_um_dos_que_o_aparelho_informou() {
        let e = Enrollments::default();
        let fp = e.announce(hello(1, MAC), None, 0).unwrap().fingerprint;
        assert!(e.approve(&fp, "sda", 1).is_err());
        assert!(!e.approve("0000-0000-0000", "mmcblk0", 1).unwrap());
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

        let e = Enrollments::default();
        for i in 0..MAX_PENDING as u8 {
            e.announce(hello(i, &format!("aa:bb:cc:dd:ee:{i:02x}")), None, 0)
                .unwrap();
        }
        assert!(
            e.announce(hello(200, "aa:bb:cc:dd:ff:00"), None, 1)
                .is_err()
        );
        // Passado o prazo as entradas somem e a vaga volta.
        assert!(
            e.announce(hello(200, "aa:bb:cc:dd:ff:00"), None, TTL_MS + 1)
                .is_ok()
        );
        assert_eq!(e.list(TTL_MS + 1).len(), 1);
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
        dirty.cpu_model = "A\u{1b}[31mB\nC".into();
        dirty.disks.push(disk("../etc"));
        e.announce(dirty, None, 0).unwrap();
        let listed = e.list(0);
        assert_eq!(listed[0].cpu_model, "A[31mBC");
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
}
