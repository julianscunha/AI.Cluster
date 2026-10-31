//! Renovação do certificado do túnel RPC deste nó (ADR-026). Perto de vencer, o nó manda ao controller um
//! CSR feito com a chave que já tem, autenticado pela credencial (não pelo token de join). O certificado novo
//! só entra depois de carregar com a chave, e a troca é atômica: `worker.crt.new` no diretório dos renovados,
//! o atual vira `worker.crt.prev`, e o `rename` põe o novo. Sem renovação, o túnel segue com o certificado do
//! convite (que nunca é sobrescrito).

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use ai_cluster_protocol::cluster::RenewNodeCertificateRequest;
use ai_cluster_protocol::metadata::MetadataClient;
use rcgen::{CertificateParams, DnType, KeyPair};

/// Intervalo entre duas olhadas no prazo do certificado em uso.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Intervalo quando nenhum certificado carrega ainda (o túnel não subiu).
pub const RETRY_EVERY: Duration = Duration::from_secs(60);

/// Renova quando faltar menos que isto para o certificado vencer (a emissão dá 825 dias).
const RENEW_BEFORE_SECS: i64 = 30 * 24 * 3600;

/// O CSR do nó: a chave dele, com o nome do nó. O controller põe o SAN por conta própria.
pub fn csr_pem(key_pem: &str, node_id: &str) -> Result<String, String> {
    let key = KeyPair::from_pem(key_pem).map_err(|e| format!("chave do nó: {e}"))?;
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, node_id);
    params
        .serialize_request(&key)
        .and_then(|csr| csr.pem())
        .map_err(|e| format!("CSR: {e}"))
}

/// Quando o certificado vence (segundos desde 1970), ou `None` se o PEM não se lê.
pub fn not_after(cert_pem: &str) -> Option<i64> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes()).ok()?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents).ok()?;
    Some(cert.validity().not_after.timestamp())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Vence em menos de 30 dias, ou não se lê (e o arquivo sumiu ou está cortado): é hora de renovar.
pub fn due(path: &str) -> bool {
    match std::fs::read_to_string(path)
        .ok()
        .and_then(|pem| not_after(&pem))
    {
        Some(end) => end - now() <= RENEW_BEFORE_SECS,
        None => true,
    }
}

/// Certificados em ordem de preferência: o renovado e o anterior renovado, depois o do convite (que nunca
/// é sobrescrito). O `bool` diz se o caminho é de renovado.
pub fn candidates(cert: &str, renewed_dir: Option<&str>) -> Vec<(String, bool)> {
    let mut list = Vec::new();
    if let Some(dir) = renewed_dir {
        list.push((format!("{dir}/worker.crt"), true));
        list.push((format!("{dir}/worker.crt.prev"), true));
    }
    list.push((cert.to_string(), false));
    list
}

/// Pede ao controller um certificado novo para a chave deste nó. Só devolve o PEM se ele confere com a
/// chave, ainda vale e é do nó; quem grava é `install`.
pub async fn fetch(
    client: &mut MetadataClient,
    node_id: &str,
    key_path: &str,
) -> Result<String, String> {
    let key_pem = std::fs::read_to_string(key_path).map_err(|e| format!("{key_path}: {e}"))?;
    let csr_pem = csr_pem(&key_pem, node_id)?;
    let fresh = client
        .renew_node_certificate(RenewNodeCertificateRequest {
            node_id: node_id.to_string(),
            csr_pem,
        })
        .await
        .map_err(|status| format!("controller recusou: {}", status.message()))?
        .into_inner()
        .cert_pem;
    let end = not_after(&fresh)
        .filter(|end| *end > now())
        .ok_or("certificado recebido não se lê ou já venceu")?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(fresh.as_bytes()).map_err(|e| e.to_string())?;
    let (_, cert) =
        x509_parser::parse_x509_certificate(&pem.contents).map_err(|e| e.to_string())?;
    let key_public = KeyPair::from_pem(&key_pem)
        .map_err(|e| format!("chave do nó: {e}"))?
        .public_key_der();
    if cert.public_key().raw != key_public.as_slice() {
        return Err("certificado recebido não é desta chave".into());
    }
    println!("certificado do túnel RPC recebido; vale até {end}");
    Ok(fresh)
}

/// Grava `pem` como o certificado renovado de `dir` (ver o topo do módulo). `loads` diz se um caminho
/// carrega com a chave deste nó; o `.new` só vira `worker.crt` se carregar, e o anterior só vira `.prev`
/// se também carregava (um certificado quebrado não substitui um bom).
pub fn install(dir: &str, pem: &str, loads: impl Fn(&str) -> bool) -> Result<(), String> {
    let (current, previous, new) = (
        format!("{dir}/worker.crt"),
        format!("{dir}/worker.crt.prev"),
        format!("{dir}/worker.crt.new"),
    );
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&new)?;
        file.write_all(pem.as_bytes())?;
        file.sync_all()
    };
    write().map_err(|e| format!("não consegui gravar {new}: {e}"))?;
    if !loads(&new) {
        let _ = std::fs::remove_file(&new);
        return Err("o certificado novo não carrega com a chave deste nó".into());
    }
    if Path::new(&current).exists() && loads(&current) {
        std::fs::rename(&current, &previous).map_err(|e| format!("guardar o anterior: {e}"))?;
    }
    std::fs::rename(&new, &current).map_err(|e| format!("trocar o certificado: {e}"))?;
    // Sem sincronizar o diretório o rename pode se perder numa queda de energia.
    std::fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| format!("sincronizar {dir}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn self_signed(key: &KeyPair, days: i64) -> String {
        let mut params = CertificateParams::new(vec!["aic-worker-no-1".into()]).unwrap();
        let today = time::OffsetDateTime::now_utc();
        params.not_before = today;
        params.not_after = today + time::Duration::days(days);
        params.self_signed(key).unwrap().pem()
    }

    #[test]
    fn o_csr_leva_a_chave_do_no_e_o_prazo_se_le() {
        let key = KeyPair::generate().unwrap();
        let csr = csr_pem(&key.serialize_pem(), "no-1").unwrap();
        assert!(csr.starts_with("-----BEGIN CERTIFICATE REQUEST-----"));
        assert!(rcgen::CertificateSigningRequestParams::from_pem(&csr).is_ok());
        let soon = self_signed(&key, 10);
        assert!(not_after(&soon).unwrap() - now() < RENEW_BEFORE_SECS);
        assert!(not_after("lixo").is_none());
    }

    #[test]
    fn a_troca_guarda_o_anterior_e_nao_troca_por_certificado_quebrado() {
        let dir = std::env::temp_dir().join(format!("aic-cert-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().into_owned();
        let key = KeyPair::generate().unwrap();
        let old = self_signed(&key, 400);
        let new = self_signed(&key, 800);
        std::fs::write(dir.join("worker.crt"), &old).unwrap();
        let ok =
            |p: &str| std::fs::read_to_string(p).is_ok_and(|t| t.contains("BEGIN CERTIFICATE"));
        // certificado que não carrega: o atual fica, e o .new some
        assert!(install(&d, "lixo", |p| ok(p)).is_err());
        assert!(dir.join("worker.crt").exists() && !dir.join("worker.crt.new").exists());
        assert!(!dir.join("worker.crt.prev").exists());
        // certificado bom: o antigo vira .prev, o novo é o atual
        install(&d, &new, |p| ok(p)).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("worker.crt")).unwrap(),
            new
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("worker.crt.prev")).unwrap(),
            old
        );
        // um atual quebrado não vira .prev (o bom anterior fica)
        std::fs::write(dir.join("worker.crt"), "quebrado").unwrap();
        install(&d, &new, |p| ok(p)).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("worker.crt.prev")).unwrap(),
            old
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
