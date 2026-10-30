//! Etapa de espera da instalação (ADR-023): o aparelho conta o que é ao controller, mostra a
//! impressão digital na tela e aguarda o operador aprovar. Nada é instalado nem apagado aqui.
//!
//!   aic-wait http://controller:50052 [--once]
//!
//! Com `https://` a CA vem de `AI_CLUSTER_TLS_CA` (e o nome do servidor de `AI_CLUSTER_TLS_DOMAIN`).
//! Aprovado, grava `/run/aic-install.json` (ou `AIC_INSTALL_OUT`) com o token e o disco, e sai 0.
//! Enter pergunta de novo na hora. `--once` pergunta uma vez e sai (teste).

mod hardware;

use std::time::Duration;

use ai_cluster_protocol::cluster::AnnounceRequest;
use ai_cluster_protocol::cluster::enrollment_client::EnrollmentClient;
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let url = args
        .next()
        .ok_or("uso: aic-wait <url do controller> [--once]")?;
    let once = args.next().as_deref() == Some("--once");
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
                    std::fs::write(
                        &out,
                        format!(
                            "{{\"token\":\"{}\",\"disk\":\"{}\"}}\n",
                            reply.install_token, reply.disk
                        ),
                    )?;
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
                let message = format!("Sem resposta do controller: {}", error.message());
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
