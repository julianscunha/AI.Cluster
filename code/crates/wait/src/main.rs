//! Etapa de espera da instalação (ADR-023): o aparelho conta o que é ao controller, mostra a
//! impressão digital na tela e aguarda o operador aprovar. Nada é instalado nem apagado aqui.
//!
//!   aic-wait http://controller:50052 [--once]
//!   aic-wait http://controller:50052 --claim-file token.txt --out worker.env [--name <nome>]  (ou AIC_INSTALL_TOKEN; --claim <token> expõe o token no ps)
//!
//! Com `https://` a CA vem de `AI_CLUSTER_TLS_CA` (e o nome do servidor de `AI_CLUSTER_TLS_DOMAIN`).
//! Aprovado, grava `/run/aic-install.json` (ou `AIC_INSTALL_OUT`) com o token e o disco, e sai 0.
//! Enter pergunta de novo na hora. `--once` pergunta uma vez e sai (teste).
//! `--claim` troca o token de uso único por `worker.env` (600): URL, join token e id do worker.

mod hardware;

use std::time::Duration;

use ai_cluster_protocol::cluster::enrollment_client::EnrollmentClient;
use ai_cluster_protocol::cluster::{AnnounceRequest, ClaimInstallRequest};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint};

fn size_text(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

fn screen(request: &AnnounceRequest, fingerprint: &str) {
    use ai_cluster_common::brand::{self, Tone, paint, rule};
    print!("[2J[H");
    println!(
        "{}
",
        brand::banner(
            "instalador",
            env!("CARGO_PKG_VERSION"),
            "aguardando aprovação"
        )
    );
    println!("{}", rule("Este aparelho", 60));
    println!("  Impressão digital  {}", paint(fingerprint, Tone::Strong));
    println!("  MAC                {}", request.mac);
    println!(
        "  Processador        {} ({} núcleos, {})",
        request.cpu_model, request.cpu_cores, request.arch
    );
    println!("  Memória            {}", size_text(request.ram_bytes));
    println!("  Firmware           {}", request.firmware);
    for disk in &request.disks {
        println!(
            "  Disco {:<10}   {} {}",
            disk.name,
            size_text(disk.size_bytes),
            disk.model
        );
    }
    println!(
        "
{}",
        rule("No controller", 60)
    );
    println!("  aic cluster install list");
    println!(
        "  aic cluster install approve {} --disk <disco>",
        paint(fingerprint, Tone::Accent)
    );
    println!(
        "
{}",
        brand::dot(Tone::Warn, "Aguardando a aprovação do operador")
    );
    println!(
        "{}",
        paint(
            "  Nada foi instalado nem apagado. Enter pergunta de novo.",
            Tone::Dim
        )
    );
}

fn connect(url: &str) -> Result<EnrollmentClient<Channel>, Box<dyn std::error::Error>> {
    let mut endpoint = Endpoint::from_shared(url.to_string())?
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10));
    if url.to_ascii_lowercase().starts_with("https://") {
        let ca = std::env::var("AI_CLUSTER_TLS_CA")
            .map_err(|_| "AI_CLUSTER_TLS_CA é obrigatório com https")?;
        let mut tls =
            ClientTlsConfig::new().ca_certificate(Certificate::from_pem(std::fs::read(ca)?));
        if let Ok(domain) = std::env::var("AI_CLUSTER_TLS_DOMAIN") {
            tls = tls.domain_name(domain);
        }
        endpoint = endpoint.tls_config(tls)?;
    }
    Ok(EnrollmentClient::new(endpoint.connect_lazy()))
}

/// O token é segredo: o arquivo nasce 0600 (no unix) e o conteúdo não passa por aqui em claro.
fn write_install(path: &str, token: &str, disk: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    writeln!(file, "{{\"token\":\"{token}\",\"disk\":\"{disk}\"}}")
}

/// `worker.env` com os três valores do worker. O join token é segredo: o arquivo fica 600 mesmo se já
/// existia com outra permissão.
fn write_env(path: &str, controller: &str, join: &str, node: &str) -> std::io::Result<()> {
    use std::io::Write;
    // Escreve num .tmp novo e troca de uma vez: quem lê nunca vê o arquivo pela metade. Um .tmp de
    // antes (ou um link no lugar dele) sai antes; create_new e O_NOFOLLOW recusam seguir link.
    let tmp = format!("{path}.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&tmp)?;
    writeln!(
        file,
        "AI_CLUSTER_CONTROLLER={controller}\nAI_CLUSTER_JOIN_TOKEN={join}\nAI_CLUSTER_NODE_ID={node}"
    )?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)
}

/// Token de uso único: `--claim-file` (ou `-` para a entrada padrão), depois `--claim`, depois
/// `AIC_INSTALL_TOKEN`. Fora de `--claim-file`/env, o token fica no argv, visível a `ps`.
fn claim_token(
    file: Option<String>,
    arg: Option<String>,
    env: Option<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    let token = match (file, arg, env) {
        (Some(path), _, _) if path == "-" => std::io::read_to_string(std::io::stdin())?,
        (Some(path), _, _) => std::fs::read_to_string(path)?,
        (None, Some(arg), _) => arg,
        (None, None, Some(env)) => env,
        (None, None, None) => return Err("token de instalação ausente".into()),
    };
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("token de instalação vazio".into());
    }
    Ok(token)
}

/// `--claim`: consome o token de uso único. Recusado, não grava nada.
async fn claim(
    url: &str,
    token: &str,
    node_name: &str,
    out: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect(url)?;
    let reply = client
        .claim_install(ClaimInstallRequest {
            token: token.into(),
            node_name: node_name.into(),
        })
        .await
        .map_err(|status| match status.code() {
            tonic::Code::Unavailable => failure_text(&status),
            _ => status.message().to_string(),
        })?
        .into_inner();
    write_env(
        out,
        &reply.controller_url,
        &reply.join_token,
        &reply.node_id,
    )
    .map_err(|err| {
        format!("o token já foi usado e {out} não foi gravado ({err}); aprove o aparelho de novo")
    })?;
    println!(
        "Worker {} pré-aprovado; configuração em {out}",
        reply.node_id
    );
    Ok(())
}

/// Motivo de uma falha do controller, em português. Porta fechada pede ação do operador, não espera.
fn failure_text(status: &tonic::Status) -> String {
    if status.code() == tonic::Code::Unavailable && status.message().contains("instalação fechada")
    {
        "Instalação fechada: o operador precisa abrir a porta com `aic cluster install open`".into()
    } else {
        format!("Sem resposta do controller: {}", status.message())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1);
    let argv: Vec<String> = args.collect();
    let flag = |name: &str| {
        argv.iter()
            .position(|arg| arg == name)
            .and_then(|at| argv.get(at + 1))
            .cloned()
    };
    let url = argv
        .first()
        .filter(|arg| !arg.starts_with("--"))
        .cloned()
        .ok_or("uso: aic-wait <url do controller> [--once] | --claim-file <arquivo|-> (ou AIC_INSTALL_TOKEN) --out <arquivo>")?;
    let env_token = std::env::var("AIC_INSTALL_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty());
    if flag("--claim").is_some() || flag("--claim-file").is_some() || env_token.is_some() {
        let token = claim_token(flag("--claim-file"), flag("--claim"), env_token)?;
        let out = flag("--out").ok_or("--claim exige --out <arquivo>")?;
        return claim(&url, &token, &flag("--name").unwrap_or_default(), &out).await;
    }
    let once = argv.iter().any(|arg| arg == "--once");
    let mut client = connect(&url)?;
    let mut request = hardware::collect();
    // Só existe na memória deste aparelho: dele deriva a impressão digital e só ele recebe o token.
    request.secret = vec![0u8; 32];
    getrandom::fill(&mut request.secret).map_err(|_| "sem fonte de entropia")?;

    let (key_press, mut pressed) = tokio::sync::mpsc::channel::<()>(1);
    if !once {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
            while let Ok(Some(_)) = lines.next_line().await {
                let _ = key_press.try_send(());
            }
        });
    }
    loop {
        let wait = match client.announce(request.clone()).await {
            Ok(reply) => {
                let reply = reply.into_inner();
                screen(&request, &reply.fingerprint);
                if reply.approved && !reply.install_token.is_empty() {
                    let out = std::env::var("AIC_INSTALL_OUT")
                        .unwrap_or_else(|_| "/run/aic-install.json".into());
                    write_install(&out, &reply.install_token, &reply.disk)?;
                    println!("\n  APROVADO. Disco a apagar: {}.", reply.disk);
                    return Ok(());
                }
                if once {
                    println!(
                        "fingerprint={} approved={}",
                        reply.fingerprint, reply.approved
                    );
                    return Ok(());
                }
                u64::from(reply.retry_after_secs.max(1))
            }
            Err(error) => {
                let message = failure_text(&error);
                println!("{message}. Tentando de novo...");
                if once {
                    return Err(message.into());
                }
                5
            }
        };
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
            _ = pressed.recv() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porta_fechada_pede_ao_operador_e_o_resto_segue_como_antes() {
        let closed = tonic::Status::unavailable("instalação fechada: o operador abre com `x`");
        assert!(failure_text(&closed).contains("o operador precisa abrir a porta"));
        let other = tonic::Status::unavailable("conexão recusada");
        assert_eq!(
            failure_text(&other),
            "Sem resposta do controller: conexão recusada"
        );
    }

    #[cfg(unix)]
    #[test]
    fn o_arquivo_do_token_nasce_so_para_o_dono() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("aic-wait-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("install.json");
        let _ = std::fs::remove_file(&path);
        write_install(path.to_str().unwrap(), "abc123", "mmcblk0").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "{\"token\":\"abc123\",\"disk\":\"mmcblk0\"}\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn o_worker_env_sai_com_os_tres_valores_e_fica_600_mesmo_se_ja_existia() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("aic-claim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("worker.env");
        std::fs::write(&path, "velho").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_env(
            path.to_str().unwrap(),
            "https://c:50051",
            "join",
            "x86-ddee01",
        )
        .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "AI_CLUSTER_CONTROLLER=https://c:50051\nAI_CLUSTER_JOIN_TOKEN=join\nAI_CLUSTER_NODE_ID=x86-ddee01\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn o_worker_env_nao_escreve_pelo_link_deixado_no_lugar_do_tmp() {
        let dir = std::env::temp_dir().join(format!("aic-claim-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("alvo.txt");
        std::fs::write(&victim, "intocado").unwrap();
        let path = dir.join("worker.env");
        let tmp = dir.join("worker.env.tmp");
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(&victim, &tmp).unwrap();
        write_env(
            path.to_str().unwrap(),
            "https://c:50051",
            "join",
            "x86-ddee01",
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "intocado");
        assert!(tmp.symlink_metadata().is_err(), "o .tmp não deve sobrar");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("AI_CLUSTER_NODE_ID=x86-ddee01")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
