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
}

#[derive(Debug, Default)]
pub struct Rollout {
    state: Mutex<State>,
    /// Arquivo de uma linha com a release desejada (sobrevive a reinício do controller).
    path: Option<PathBuf>,
}

/// Valida o que vai ser repassado a todos os nós e, depois, a um comando do worker.
pub fn validate(release: &Release) -> Result<(), String> {
    let word = |text: &str, extra: &str| {
        !text.is_empty()
            && text.len() <= 200
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
    };
    if !word(&release.version, "._-+") {
        return Err("versão inválida".into());
    }
    if release.sha256.len() != 64 || !release.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("sha256 deve ter 64 hexadecimais".into());
    }
    if release.image_sha256.len() != 64
        || !release.image_sha256.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err("image_sha256 deve ter 64 hexadecimais".into());
    }
    // Ed25519 tem 64 bytes: 88 caracteres em base64. Quem confere a assinatura é o nó.
    if release.signature.len() != 88
        || !release
            .signature
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c))
    {
        return Err("signature deve ser base64 de 64 bytes (Ed25519)".into());
    }
    // Sem userinfo nem query: o URL vai para o log de auditoria e para todos os nós.
    let rest = release
        .url
        .strip_prefix("https://")
        .or_else(|| release.url.strip_prefix("http://"))
        .ok_or("url deve ser http(s)")?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains('@') || !word(rest, "./_-:%+") {
        return Err("url inválido (sem usuário, query ou caracteres especiais)".into());
    }
    Ok(())
}

impl Rollout {
    pub fn new(path: Option<PathBuf>) -> Self {
        let desired = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| {
                let mut parts = text.split_whitespace();
                let release = Release {
                    version: parts.next()?.to_string(),
                    url: parts.next()?.to_string(),
                    sha256: parts.next()?.to_string(),
                    image_sha256: parts.next()?.to_string(),
                    signature: parts.next()?.to_string(),
                };
                validate(&release).ok().map(|()| release)
            });
        Self {
            state: Mutex::new(State {
                desired,
                ..State::default()
            }),
            path,
        }
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
                        "{} {} {} {} {}\n",
                        r.version, r.url, r.sha256, r.image_sha256, r.signature
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
                let done = canary_version(&canary).is_some_and(|(v, m)| {
                    v == desired.version && (m.is_empty() || m == desired.image_sha256)
                });
                if done {
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
        }
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
        let a_novo = |n: &str| (n == "a").then(|| ("2".to_string(), String::new()));
        // medida errada: o canário não concluiu
        let a_errado = |n: &str| (n == "a").then(|| ("2".to_string(), "c".repeat(64)));
        assert!(rollout.announce(true, "b", "1", a_errado, 2).0.is_none());
        assert!(rollout.announce(true, "b", "1", a_novo, 2).0.is_some());
        // quem já está na versão não recebe nada
        assert!(rollout.announce(true, "a", "2", a_novo, 3).0.is_none());
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
