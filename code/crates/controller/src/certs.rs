//! Renovação do certificado do túnel RPC de cada worker (ADR-026). O controller guarda a CA do cluster
//! e emite, a pedido de um worker aprovado, um certificado novo com o SAN `aic-worker-<node_id>`. Sem convite
//! aberto: a prova é a credencial do worker, não o token de join.

use rcgen::{
    CertificateParams, CertificateSigningRequestParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SanType, SerialNumber,
};
use time::{Duration, OffsetDateTime};

/// Validade do certificado emitido (a mesma do convite).
pub const WORKER_CERT_DAYS: i64 = 825;

/// Variáveis que apontam para a CA: certificado e chave, sempre juntos.
pub const CA_CERT_ENV: &str = "AI_CLUSTER_RPC_CA_CERT";
pub const CA_KEY_ENV: &str = "AI_CLUSTER_RPC_CA_KEY";

/// A CA do cluster, pronta para assinar. A chave dá o poder de emitir: só o controller a lê
/// (o gateway não vê o arquivo, ver a unit no `setup`).
pub struct CaSigner {
    issuer: rcgen::Certificate,
    key: KeyPair,
}

/// Sem a chave na saída: um `{:?}` em log ou em um assert não a mostra.
impl std::fmt::Debug for CaSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CaSigner")
    }
}

impl CaSigner {
    /// `None` quando nenhuma das duas variáveis está definida (sem renovação). Só uma delas é erro.
    pub fn from_env() -> Result<Option<Self>, String> {
        match (std::env::var(CA_CERT_ENV), std::env::var(CA_KEY_ENV)) {
            (Ok(cert), Ok(key)) => {
                let cert = std::fs::read_to_string(&cert)
                    .map_err(|e| format!("{CA_CERT_ENV} {cert}: {e}"))?;
                let key = std::fs::read_to_string(&key)
                    .map_err(|e| format!("{CA_KEY_ENV} {key}: {e}"))?;
                Self::from_pem(&cert, &key).map(Some)
            }
            (Err(_), Err(_)) => Ok(None),
            _ => Err(format!("{CA_CERT_ENV} e {CA_KEY_ENV} vêm juntos")),
        }
    }

    pub fn from_pem(cert_pem: &str, key_pem: &str) -> Result<Self, String> {
        let key = KeyPair::from_pem(key_pem).map_err(|e| format!("chave da CA: {e}"))?;
        let params = CertificateParams::from_ca_cert_pem(cert_pem)
            .map_err(|e| format!("certificado da CA: {e}"))?;
        // A chave tem de ser a do certificado; senão o que sai não encadeia com a CA dos workers.
        let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes())
            .map_err(|e| format!("certificado da CA: {e}"))?;
        let (_, ca) = x509_parser::parse_x509_certificate(&pem.contents)
            .map_err(|e| format!("certificado da CA: {e}"))?;
        if ca.public_key().raw != key.public_key_der().as_slice() {
            return Err("a chave não é a da CA".into());
        }
        let issuer = params.self_signed(&key).map_err(|e| format!("CA: {e}"))?;
        Ok(Self { issuer, key })
    }

    /// Impressão digital (SHA-256, hex) da chave pública do CSR: é o que o controller fixa por worker.
    pub fn key_fingerprint(csr_pem: &str) -> Result<String, String> {
        use rcgen::PublicKeyData;
        use sha2::{Digest, Sha256};
        let request = CertificateSigningRequestParams::from_pem(csr_pem)
            .map_err(|e| format!("pedido de certificado recusado: {e}"))?;
        let digest = Sha256::digest(request.public_key.der_bytes());
        Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Emite o certificado do worker `node_id` a partir do CSR dele. O SAN e o nome vêm do `node_id`
    /// que o controller já conhece, nunca do pedido; a assinatura do CSR prova que quem pede tem a chave.
    pub fn sign_worker(
        &self,
        node_id: &str,
        csr_pem: &str,
        now: OffsetDateTime,
    ) -> Result<String, String> {
        // Mesmo conjunto aceito pelo convite: o nome vai para um SAN de DNS.
        let valid = !node_id.is_empty()
            && node_id.len() <= 40
            && node_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
        if !valid {
            return Err("node_id inválido para um certificado de worker".into());
        }
        let request = CertificateSigningRequestParams::from_pem(csr_pem)
            .map_err(|e| format!("pedido de certificado recusado: {e}"))?;
        let san = format!("aic-worker-{node_id}")
            .try_into()
            .map_err(|e| format!("SAN: {e}"))?;
        let mut params = CertificateParams::default();
        params.distinguished_name.push(DnType::CommonName, node_id);
        params.subject_alt_names = vec![SanType::DnsName(san)];
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.not_before = now - Duration::days(1);
        params.not_after = now + Duration::days(WORKER_CERT_DAYS);
        // Serial sorteado: o padrão do rcgen sai da chave, e a renovação reaproveita a chave.
        let mut serial = [0u8; 16];
        getrandom::fill(&mut serial).map_err(|e| format!("sorteio do serial: {e}"))?;
        serial[0] &= 0x7f;
        params.serial_number = Some(SerialNumber::from(serial.to_vec()));
        let cert = params
            .signed_by(&request.public_key, &self.issuer, &self.key)
            .map_err(|e| format!("assinatura: {e}"))?;
        Ok(cert.pem())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, DnType, IsCa, KeyPair, KeyUsagePurpose};
    use x509_parser::extensions::GeneralName;

    /// Uma CA de teste e a chave dela, em PEM.
    fn ca() -> (String, String) {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec![]).unwrap();
        params
            .distinguished_name
            .push(DnType::CommonName, "ai-cluster-ca");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    /// O CSR de um worker: a chave dele e o SAN que ele pede (o controller não deve obedecer).
    fn csr(key: &KeyPair, asked_san: &str) -> String {
        CertificateParams::new(vec![asked_san.to_string()])
            .unwrap()
            .serialize_request(key)
            .unwrap()
            .pem()
            .unwrap()
    }

    /// DER do primeiro certificado de um PEM.
    fn parse(pem: &str) -> Vec<u8> {
        x509_parser::pem::parse_x509_pem(pem.as_bytes())
            .unwrap()
            .1
            .contents
    }

    #[test]
    fn emite_com_o_san_do_no_e_uso_de_servidor_mesmo_que_o_pedido_diga_outra_coisa() {
        let (cert_pem, key_pem) = ca();
        let signer = CaSigner::from_pem(&cert_pem, &key_pem).unwrap();
        let node_key = KeyPair::generate().unwrap();
        let now = OffsetDateTime::now_utc();
        let issued = signer
            .sign_worker("node-a", &csr(&node_key, "aic-gateway"), now)
            .unwrap();
        let der = parse(&issued);
        let (_, cert) = x509_parser::parse_x509_certificate(&der).unwrap();
        let san: Vec<String> = cert
            .subject_alternative_name()
            .unwrap()
            .unwrap()
            .value
            .general_names
            .iter()
            .filter_map(|name| match name {
                GeneralName::DNSName(dns) => Some(dns.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(san, vec!["aic-worker-node-a".to_string()]);
        let eku = cert.extended_key_usage().unwrap().unwrap().value;
        assert!(eku.server_auth && !eku.client_auth);
        let constraints = cert
            .basic_constraints()
            .unwrap()
            .expect("basicConstraints ausente");
        assert!(!constraints.value.ca, "a folha deve dizer CA:FALSE");
        assert_eq!(
            cert.subject()
                .iter_common_name()
                .next()
                .unwrap()
                .as_str()
                .unwrap(),
            "node-a"
        );
        // Validade: começa antes de agora (folga de relógio) e dura o prazo do convite.
        let days = (cert.validity().not_after.timestamp() - now.unix_timestamp()) / 86_400;
        assert!(
            (WORKER_CERT_DAYS - 1..=WORKER_CERT_DAYS).contains(&days),
            "{days} dias"
        );
        assert!(cert.validity().not_before.timestamp() < now.unix_timestamp());
        // O certificado é da CA deste cluster: o emissor é o sujeito dela.
        let ca_der = parse(&cert_pem);
        let (_, ca_cert) = x509_parser::parse_x509_certificate(&ca_der).unwrap();
        assert_eq!(cert.issuer(), ca_cert.subject());
    }

    #[test]
    fn recusa_node_id_fora_do_formato_e_csr_sem_prova_de_chave() {
        let (cert_pem, key_pem) = ca();
        let signer = CaSigner::from_pem(&cert_pem, &key_pem).unwrap();
        let key = KeyPair::generate().unwrap();
        let now = OffsetDateTime::now_utc();
        for bad in ["", "no de", "a/b", "x".repeat(41).as_str()] {
            assert!(
                signer.sign_worker(bad, &csr(&key, "x"), now).is_err(),
                "{bad:?}"
            );
        }
        assert!(signer.sign_worker("node-a", "lixo", now).is_err());
    }

    #[test]
    fn recusa_chave_que_nao_e_da_ca() {
        let (cert_pem, _) = ca();
        let (_, other_key) = ca();
        assert!(CaSigner::from_pem(&cert_pem, &other_key).is_err());
    }
}
