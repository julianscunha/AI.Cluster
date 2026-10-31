//! Rollout de release (F19): o operador define a versão desejada e o controller a anuncia aos nós
//! pelo heartbeat. Um canário recebe primeiro; os demais só depois que ele reporta a versão nova.
//! Se o canário não chega lá no prazo, o rollout pausa (nada mais é anunciado).

use std::path::PathBuf;
use std::sync::Mutex;

use ai_cluster_protocol::cluster::Release;

/// Tempo que o canário tem para reportar a versão nova antes de o rollout pausar.
pub const CANARY_TIMEOUT_MS: u64 = 600_000;

#[derive(Debug, Default)]
struct State {
    desired: Option<Release>,
    canary: Option<String>,
    canary_since_ms: u64,
    paused: bool,
    /// Nós liberados depois do canário que ainda não chegaram à versão (nó → quando foi liberado).
    wave: std::collections::HashMap<String, u64>,
}

#[derive(Debug, Default)]
pub struct Rollout {
    state: Mutex<State>,
    /// Arquivo de uma linha com a release desejada (sobrevive a reinício do controller).
    path: Option<PathBuf>,
    /// Quantos nós atualizam ao mesmo tempo depois do canário (0 = todos).
    wave_limit: usize,
}

/// Palavra segura: letras, dígitos e os caracteres de `extra`, sem vazio e até 200 caracteres.
fn palavra(text: &str, extra: &str) -> bool {
    !text.is_empty()
        && text.len() <= 200
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// Ed25519 tem 64 bytes: 88 caracteres em base64. Quem confere a assinatura é o nó.
fn assinatura_ok(text: &str) -> bool {
    text.len() == 88
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c))
}

/// Valida o que vai ser repassado a todos os nós e, depois, a um comando do worker. Uma release
/// leva a assinatura de AI Cluster OS, a de Linux comum, ou as duas: cada nó confere a sua (ADR-025).
pub fn validate(release: &Release) -> Result<(), String> {
    if !palavra(&release.version, "._-+") {
        return Err("versão inválida".into());
    }
    if release.signature.is_empty() && release.linux_signature.is_empty() {
        return Err(
            "a release precisa de assinatura (AI Cluster OS, Linux comum ou as duas)".into(),
        );
    }
    if !release.signature.is_empty() {
        if release.sha256.len() != 64 || !release.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("sha256 deve ter 64 hexadecimais".into());
        }
        if release.image_sha256.len() != 64
            || !release.image_sha256.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err("image_sha256 deve ter 64 hexadecimais".into());
        }
        if !assinatura_ok(&release.signature) {
            return Err("signature deve ser base64 de 64 bytes (Ed25519)".into());
        }
    }
    if !release.linux_signature.is_empty() && !assinatura_ok(&release.linux_signature) {
        return Err("linux_signature deve ser base64 de 64 bytes (Ed25519)".into());
    }
    // Sem userinfo nem query: o URL vai para o log de auditoria e para todos os nós (e o worker Linux
    // busca a lista de hashes nele).
    let rest = release
        .url
        .strip_prefix("https://")
        .or_else(|| release.url.strip_prefix("http://"))
        .ok_or("url deve ser http(s)")?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains('@') || !palavra(rest, "./_-:%+") {
        return Err("url inválido (sem usuário, query ou caracteres especiais)".into());
    }
    Ok(())
}

/// Campo vazio no arquivo de estado: `-` (um espaço some na leitura por palavras).
fn campo(text: &str) -> &str {
    if text.is_empty() { "-" } else { text }
}

impl Rollout {
    pub fn new(path: Option<PathBuf>) -> Self {
        let desired = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| {
                let mut parts = text
                    .split_whitespace()
                    .map(|p| if p == "-" { "" } else { p });
                let release = Release {
                    version: parts.next()?.to_string(),
                    url: parts.next()?.to_string(),
                    sha256: parts.next()?.to_string(),
                    image_sha256: parts.next()?.to_string(),
                    signature: parts.next()?.to_string(),
                    // Arquivo gravado antes da assinatura Linux tem só os cinco campos acima.
                    linux_signature: parts.next().unwrap_or_default().to_string(),
                };
                validate(&release).ok().map(|()| release)
            });
        Self {
            state: Mutex::new(State {
                desired,
                ..State::default()
            }),
            path,
            wave_limit: 0,
        }
    }

    /// Limita a quantos nós atualizam ao mesmo tempo depois do canário (`AI_CLUSTER_ROLLOUT_WAVE`).
    pub fn with_wave(mut self, limit: usize) -> Self {
        self.wave_limit = limit;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Define (ou, com `None`, cancela) a release desejada e reinicia o canário.
    pub fn set(&self, release: Option<Release>) {
        if let Some(path) = &self.path {
            let line = release
                .as_ref()
                .map(|r| {
                    format!(
                        "{} {} {} {} {} {}\n",
                        r.version,
                        r.url,
                        campo(&r.sha256),
                        campo(&r.image_sha256),
                        campo(&r.signature),
                        campo(&r.linux_signature)
                    )
                })
                .unwrap_or_default();
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, line).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
        *self.lock() = State {
            desired: release,
            ..State::default()
        };
    }

    pub fn desired(&self) -> Option<Release> {
        self.lock().desired.clone()
    }

    /// O que anunciar ao nó `node` que roda `version`. `canary_version` diz o que o canário
    /// atual roda (o registro sabe). Devolve também um evento quando o rollout pausa ou conclui.
    pub fn announce(
        &self,
        can_update: bool,
        node: &str,
        version: &str,
        canary_version: impl Fn(&str) -> Option<(String, String)>,
        now_ms: u64,
    ) -> (Option<Release>, Option<String>) {
        let mut state = self.lock();
        let Some(desired) = state.desired.clone() else {
            return (None, None);
        };
        // um nó sem comando de atualização não entra no rollout nem vira canário
        if version == desired.version {
            state.wave.remove(node);
        }
        if !can_update || version == desired.version || state.paused {
            return (None, None);
        }
        match state.canary.clone() {
            None => {
                state.canary = Some(node.to_string());
                state.canary_since_ms = now_ms;
                (Some(desired), Some(format!("rollout canário={node}")))
            }
            Some(canary) if canary == node => {
                if now_ms.saturating_sub(state.canary_since_ms) > CANARY_TIMEOUT_MS {
                    state.paused = true;
                    return (
                        None,
                        Some(format!(
                            "rollout pausado: canário {node} não chegou a {}",
                            desired.version
                        )),
                    );
                }
                (Some(desired), None)
            }
            Some(canary) => {
                // o canário concluiu quando roda a versão e, se mede o slot, a medida bate
                let done = canary_version(&canary)
                    .is_some_and(|(v, m)| v == desired.version && m == desired.image_sha256);
                if done {
                    // onda: quem não chegou à versão em 10 min deixa de ocupar vaga
                    state
                        .wave
                        .retain(|_, since| now_ms.saturating_sub(*since) <= CANARY_TIMEOUT_MS);
                    if self.wave_limit > 0
                        && !state.wave.contains_key(node)
                        && state.wave.len() >= self.wave_limit
                    {
                        return (None, None);
                    }
                    state.wave.entry(node.to_string()).or_insert(now_ms);
                    (Some(desired), None)
                } else {
                    (None, None)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str) -> Release {
        Release {
            version: version.into(),
            url: "http://192.168.1.25:8080/img/rootfs.ext4.gz".into(),
            sha256: "a".repeat(64),
            image_sha256: "b".repeat(64),
            signature: format!("{}==", "A".repeat(86)),
            linux_signature: String::new(),
        }
    }

    #[test]
    fn aceita_assinatura_so_linux_ou_so_os_ou_as_duas() {
        let assinada = format!("{}==", "A".repeat(86));
        // só Linux: a URL é obrigatória e os campos de AI Cluster OS ficam vazios
        let mut so_linux = release("1.2.0");
        so_linux.signature = String::new();
        so_linux.sha256 = String::new();
        so_linux.image_sha256 = String::new();
        so_linux.linux_signature = assinada.clone();
        assert!(validate(&so_linux).is_ok());
        so_linux.url = String::new();
        assert!(
            validate(&so_linux).is_err(),
            "sem URL o worker não acha o SHA256SUMS"
        );
        // sem assinatura nenhuma, nunca
        let mut nua = release("1.2.0");
        nua.signature = String::new();
        assert!(validate(&nua).is_err());
        // assinatura Linux mal formada
        let mut ruim = release("1.2.0");
        ruim.linux_signature = "curta".into();
        assert!(validate(&ruim).is_err());
    }

    #[test]
    fn valida_o_que_vai_para_os_nos() {
        assert!(validate(&release("1.2.0")).is_ok());
        let mut r = release("1.2.0");
        r.url = "http://u:p@host/x".into();
        assert!(validate(&r).is_err());
        r.url = "http://host/x?token=1".into();
        assert!(validate(&r).is_err());
        r.url = "ftp://host/x".into();
        assert!(validate(&r).is_err());
        let mut r = release("1.2.0");
        r.sha256 = "zz".into();
        assert!(validate(&r).is_err());
        assert!(validate(&release("1; reboot")).is_err());
        let mut r = release("1.2.0");
        r.signature = "curta".into();
        assert!(validate(&r).is_err());
        r.signature = String::new();
        assert!(validate(&r).is_err());
    }

    #[test]
    fn canario_primeiro_depois_os_demais() {
        let rollout = Rollout::new(None);
        rollout.set(Some(release("2")));
        let none = |_: &str| None;
        // nó que não sabe atualizar não vira canário
        assert!(rollout.announce(false, "x", "1", none, 0).0.is_none());
        // o primeiro nó vira canário
        assert!(rollout.announce(true, "a", "1", none, 0).0.is_some());
        // outro nó espera enquanto o canário não reporta a versão nova
        assert!(rollout.announce(true, "b", "1", none, 1).0.is_none());
        // o canário reporta "2": libera os demais
        let a_novo = |n: &str| (n == "a").then(|| ("2".to_string(), "b".repeat(64)));
        // medida errada: o canário não concluiu
        let a_errado = |n: &str| (n == "a").then(|| ("2".to_string(), "c".repeat(64)));
        assert!(rollout.announce(true, "b", "1", a_errado, 2).0.is_none());
        assert!(rollout.announce(true, "b", "1", a_novo, 2).0.is_some());
        // quem já está na versão não recebe nada
        assert!(rollout.announce(true, "a", "2", a_novo, 3).0.is_none());
    }

    #[test]
    fn onda_limita_quantos_atualizam_juntos() {
        let rollout = Rollout::new(None).with_wave(1);
        rollout.set(Some(release("2")));
        let none = |_: &str| None;
        let a_novo = |n: &str| (n == "a").then(|| ("2".to_string(), "b".repeat(64)));
        assert!(rollout.announce(true, "a", "1", none, 0).0.is_some());
        assert!(rollout.announce(true, "b", "1", a_novo, 1).0.is_some());
        // vaga ocupada por b; c espera, b continua sendo anunciado
        assert!(rollout.announce(true, "c", "1", a_novo, 2).0.is_none());
        assert!(rollout.announce(true, "b", "1", a_novo, 3).0.is_some());
        // b chegou à versão: libera a vaga
        assert!(rollout.announce(true, "b", "2", a_novo, 4).0.is_none());
        assert!(rollout.announce(true, "c", "1", a_novo, 5).0.is_some());
        // b que nunca voltasse perderia a vaga depois do prazo
        assert!(rollout.announce(true, "d", "1", a_novo, 6).0.is_none());
        assert!(
            rollout
                .announce(true, "d", "1", a_novo, CANARY_TIMEOUT_MS + 6)
                .0
                .is_some()
        );
    }

    #[test]
    fn canario_que_nao_chega_pausa() {
        let rollout = Rollout::new(None);
        rollout.set(Some(release("2")));
        let none = |_: &str| None;
        assert!(rollout.announce(true, "a", "1", none, 0).0.is_some());
        let (r, evento) = rollout.announce(true, "a", "1", none, CANARY_TIMEOUT_MS + 1);
        assert!(r.is_none() && evento.is_some());
        assert!(
            rollout
                .announce(true, "b", "1", none, CANARY_TIMEOUT_MS + 2)
                .0
                .is_none()
        );
        // novo set limpa a pausa
        rollout.set(Some(release("3")));
        assert!(rollout.announce(true, "b", "1", none, 0).0.is_some());
    }

    #[test]
    fn persiste_a_release() {
        let path = std::env::temp_dir().join(format!("aic-rollout-{}", std::process::id()));
        Rollout::new(Some(path.clone())).set(Some(release("2")));
        assert_eq!(
            Rollout::new(Some(path.clone())).desired().unwrap().version,
            "2"
        );
        let _ = std::fs::remove_file(path);
    }
}
