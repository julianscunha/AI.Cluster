use std::time::{Duration, Instant};

use ai_cluster_common::matrix::Matrix;
use ai_cluster_protocol::cluster::{
    ApproveNodeRequest, CancelRequestRequest, CheckModelRequest, EnqueueGraphRequest,
    EnqueueWorkRequest, GetMetricsRequest, GetTraceRequest, KeyAddRequest, KeyListRequest,
    KeyRemoveRequest, ListEventsRequest, ListModelsRequest, ListNodesRequest,
    ListReservationsRequest, ModelState, ModelStatusRequest, Release, ReleaseReservationRequest,
    ReserveRequest, RevokeNodeRequest, SetReleaseRequest, StartModelRequest, StopModelRequest,
    WatchEventsRequest, WorkReportRequest, WorkReportResponse,
};
use ai_cluster_protocol::metadata::{RECOVERY_NODE_SUFFIX, connect_operator};
mod delegados;
mod ptbr;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "aic",
    about = "Um comando para todo o AI Cluster: cluster, controller, gateway, worker e atualização."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Operação do cluster visto do controller: workers, instalação, reservas, modelos e diagnósticos.
    Cluster {
        /// Endereço gRPC do controller.
        #[arg(
            long,
            global = true,
            env = "AI_CLUSTER_CONTROLLER",
            default_value = "http://127.0.0.1:50051"
        )]
        controller: String,
        #[command(subcommand)]
        action: ClusterAction,
    },
    /// Controller deste Linux: instalação, estado e registros.
    Controller {
        #[command(subcommand)]
        action: delegados::Controller,
    },
    /// Gateway de inferência deste Linux: instalação, estado e registros.
    Gateway {
        #[command(subcommand)]
        action: delegados::Gateway,
    },
    /// Worker deste Linux ou do AI Cluster OS.
    Worker {
        #[command(subcommand)]
        action: delegados::Worker,
    },
    /// Atualização deste computador: consultar ou aplicar.
    Update {
        #[command(subcommand)]
        action: delegados::Update,
    },
    /// Versão do aic e do sistema.
    Version,
}

#[derive(Subcommand, Debug)]
enum ClusterAction {
    /// Tabela dos workers: estado, recursos, saúde, confiança e a versão de cada um.
    Nodes,
    /// Um worker: `show` mostra tudo; `approve`, `retire`, `revoke` e `remove` mudam o estado dele.
    Node {
        #[command(subcommand)]
        action: NodeAction,
    },
    /// Aparelhos em espera de instalação (ADR-023) e a porta em que eles se apresentam.
    Install {
        #[command(subcommand)]
        action: InstallAction,
    },
    /// Reservas: um worker retido para uma requisição futura (G6).
    Reservation {
        #[command(subcommand)]
        action: ReservationAction,
    },
    /// A estrutura do cluster em árvore: o controller e, abaixo, os workers agrupados por papel.
    Topology,
    /// Resumo do cluster: workers por estado, fila de trabalho e tempos das unidades concluídas.
    Metrics,
    /// Eventos recentes, do mais antigo ao mais novo: entradas, saídas, workers dados como offline,
    /// trabalho enfileirado e concluído.
    Events {
        /// Os N eventos mais recentes. 0 mostra tudo o que o controller ainda guarda.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Depois de listar, continua mostrando os eventos novos à medida que acontecem.
        #[arg(long)]
        follow: bool,
    },
    /// Arquivos de modelo: manifesto com versão e somas de verificação, e a conferência dele (F12). Local.
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Acompanha o caminho de uma requisição de cliente pelo cluster (H3).
    Request {
        #[command(subcommand)]
        action: RequestAction,
    },
    /// Despacha unidades de trabalho de matriz e compara o custo de cálculo com o de transferência.
    Benchmark {
        /// Lado da matriz quadrada.
        #[arg(long, default_value_t = 256)]
        size: u32,
        /// Quantas unidades de trabalho despachar.
        #[arg(long, default_value_t = 1)]
        repetitions: u32,
        /// Calcula a mesma carga no próprio processo, sem despachar.
        #[arg(long)]
        local: bool,
        /// Não entrega a unidade a um worker que informe menos threads de CPU que isto.
        /// 0 não exige nada.
        #[arg(long, default_value_t = 0)]
        min_threads: u32,
        /// Enfileira com este id de requisição, em vez de o controller criar um.
        /// Passe o id de uma reserva para as unidades irem ao worker retido (G6).
        #[arg(long, default_value = "")]
        request_id: String,
        /// Classe de prioridade das unidades: maior despacha primeiro, 0 é o padrão (G4).
        #[arg(long, default_value_t = 0)]
        priority: u32,
        /// Só workers atestados (release desejada, medida do slot assinada) podem rodar as unidades.
        #[arg(long)]
        require_attested: bool,
    },
    /// Despacha um grafo de dependências (losango) e mostra os ramos esperando uns pelos outros.
    Graph {
        /// Lado da matriz quadrada de cada unidade do grafo.
        #[arg(long, default_value_t = 128)]
        size: u32,
    },
    /// Define a release que os workers devem rodar: um canário primeiro, os demais depois (F19).
    Rollout {
        /// Versão (ex.: 1.2.0). Vazia, com --cancel, encerra o rollout.
        #[arg(default_value = "")]
        version: String,
        /// URL do artefato (http/https, sem usuário nem query).
        #[arg(long, default_value = "")]
        url: String,
        /// SHA-256 do artefato (64 hexadecimais).
        #[arg(long, default_value = "")]
        sha256: String,
        /// SHA-256 da imagem ext4 descomprimida (ver os/image/sign-release.sh).
        #[arg(long, default_value = "")]
        image_sha256: String,
        /// Assinatura Ed25519 em base64 (ver os/image/sign-release.sh).
        #[arg(long, default_value = "")]
        signature: String,
        /// Assinatura Ed25519 em base64 do SHA256SUMS da release Linux comum (scripts/package-release.sh).
        /// `--url` é então o diretório da release (ADR-025).
        #[arg(long, default_value = "")]
        linux_signature: String,
        /// Cancela o rollout em andamento.
        #[arg(long)]
        cancel: bool,
    },
    /// Painel de uma tela: workers, fila de trabalho e reservas (H4).
    Dashboard {
        /// Atualiza a cada N segundos até Ctrl-C. 0 imprime uma vez e sai.
        #[arg(long, default_value_t = 0)]
        watch: u64,
    },
    /// Abre o convite para computadores novos entrarem no cluster (instalador).
    Invite(delegados::Convite),
}

#[derive(Subcommand, Debug)]
enum NodeAction {
    /// Um worker pelo id, com tudo que o controller sabe sobre ele.
    Show {
        /// Id do worker, como `aic cluster nodes` o imprime.
        id: String,
    },
    /// Aprova um worker novo para que ele receba trabalho (S2).
    Approve {
        /// Worker a aprovar.
        id: String,
        /// Impressão digital que o worker imprime ao registrar-se (e `nodes` mostra). Obrigatória, salvo
        /// `--trust-listed`.
        #[arg(long)]
        fingerprint: Option<String>,
        /// Aceita a impressão que o controller mostra agora, sem conferi-la com a do worker. Anula a proteção
        /// contra um id tomado por outro registro.
        #[arg(long, conflicts_with = "fingerprint")]
        trust_listed: bool,
    },
    /// Tira o worker do trabalho mantendo a identidade dele: volta com `approve` e a impressão de antes.
    Retire {
        /// Worker a retirar.
        id: String,
    },
    /// Revoga a credencial do worker: ele fica offline, perde a renovação do certificado do túnel
    /// (ADR-026) e não recebe mais o túnel RPC; só volta com um novo join (S5).
    Revoke {
        /// Worker a revogar.
        id: String,
    },
    /// Remove o worker do cluster: o controller o esquece por inteiro.
    Remove {
        /// Worker a remover.
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum InstallAction {
    /// Aparelhos em espera, aguardando aprovação para instalar (ADR-023).
    List,
    /// Aprova a instalação de um aparelho da lista, apagando o disco informado.
    Approve {
        /// Impressão digital que o aparelho mostra na tela (e `install list` lista).
        fingerprint: String,
        /// Disco que será apagado (`sda`, `mmcblk0`); precisa ser um dos que o aparelho informou.
        #[arg(long)]
        disk: String,
    },
    /// Tira um aparelho da lista de espera.
    Reject {
        /// Impressão digital do aparelho.
        fingerprint: String,
    },
    /// Abre a porta em que os aparelhos se apresentam para instalar. Depois fecha sozinha.
    Open {
        /// Minutos (1 a 120).
        #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u32).range(1..=120))]
        minutes: u32,
    },
    /// Fecha a porta agora.
    Close,
}

#[derive(Subcommand, Debug)]
enum ReservationAction {
    /// Reservas ativas: worker, requisição dona e vencimento.
    List,
    /// Retém um worker para uma requisição futura (G6).
    Add {
        /// Worker a reter.
        node: String,
        /// Requisição para quem o worker fica retido.
        request_id: String,
        /// Minutos que a reserva vale. 0 usa o padrão do controller.
        #[arg(long, default_value_t = 0)]
        minutes: u32,
        /// Quantos slots de trabalho do worker guardar para a requisição. 0 retém o worker inteiro.
        #[arg(long, default_value_t = 0)]
        slots: u32,
    },
    /// Libera a reserva de um worker antes do vencimento.
    Remove {
        /// Worker a liberar.
        node: String,
    },
}

/// The `model` subcommands: arquivos, conferência, subida e chaves.
#[derive(Subcommand, Debug)]
enum ModelAction {
    /// Grava o `manifest.json` numa pasta de modelo: nome, versão e o SHA-256 de cada arquivo (F12).
    Manifest {
        /// Pasta do modelo (a que o gateway e os workers leem).
        dir: std::path::PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        version: String,
    },
    /// Confere os arquivos de uma pasta de modelo com o `manifest.json` dela.
    Verify { dir: std::path::PathBuf },
    /// Diz se o cluster comporta um modelo da pasta: OK, ou NOK com o motivo e quanto falta (ADR-022, E2).
    Check {
        /// Id do modelo, como `model list` o mostra.
        id: String,
        /// Contexto em tokens para estimar o cache (padrão 4096).
        #[arg(long, default_value_t = 0)]
        ctx: u64,
    },
    /// Sobe o modelo no cluster: o controller confere a RAM dos workers, escolhe os que dividem o
    /// modelo e o gateway o carrega pelo llama.cpp. Mostra a chave de API (uma vez). ADR-022, E6.
    Start {
        /// Id do modelo, como `model list` o mostra (um GGUF).
        id: String,
        /// Contexto em tokens (padrão 4096, limitado ao do modelo).
        #[arg(long, default_value_t = 0)]
        ctx: u64,
    },
    /// Para o modelo no gateway. As chaves continuam valendo para o próximo `start`.
    Stop,
    /// O modelo pedido, os workers do plano, o estado que o gateway reportou e o benchmark.
    Status,
    /// Chaves de API do llama.cpp: uma por tenant, com modelos, cota e classe.
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    /// Lista os modelos da pasta do controller (`AI_CLUSTER_MODELS_DIR`): formato, arquitetura, tamanho e
    /// se o motor atual os roda (ADR-022, E1).
    List,
}

#[derive(Subcommand, Debug)]
enum KeyAction {
    /// Cria o tenant e mostra a chave (uma vez; o controller guarda só o hash).
    Add {
        tenant: String,
        /// Modelos que a chave alcança, separados por vírgula (padrão: todos).
        #[arg(long, value_delimiter = ',')]
        models: Vec<String>,
        /// Tokens por janela (prompt mais gerados); 0 = sem cota.
        #[arg(long, default_value_t = 0)]
        tokens: u64,
        /// Classe máxima: background, batch, normal, interactive ou realtime.
        #[arg(long, default_value = "")]
        class: String,
    },
    /// Troca a chave de um tenant que existe; a antiga deixa de valer. Mantém os limites.
    Rotate { tenant: String },
    /// Tira o tenant.
    Remove { tenant: String },
    /// Lista os tenants (prefixo da chave e limites; nunca a chave).
    List,
}

#[derive(Subcommand, Debug)]
enum RequestAction {
    /// As unidades que uma requisição gerou, em que ponto cada uma está e quanto custou.
    Trace {
        /// Id da requisição, como `benchmark` ou `graph` o imprimem.
        request_id: String,
    },
    /// Tira da fila as unidades de uma requisição. As que já estão num worker terminam (F11).
    Cancel {
        /// Id da requisição, como `benchmark` ou `graph` o imprimem.
        request_id: String,
    },
}

#[tokio::main]
async fn main() {
    // O `Error: …` do Rust sairia em inglês: toda falha do aic aparece como `erro: …`.
    if let Err(error) = run().await {
        eprintln!("erro: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let matches = match ptbr::localize(Cli::command()).try_get_matches() {
        Ok(matches) => matches,
        Err(error) => ptbr::show(&error),
    };
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    match cli.command {
        Command::Cluster { controller, action } => {
            ai_cluster_protocol::metadata::check_secret_files()?;
            run_cluster(&controller, action).await?
        }
        Command::Controller { action } => delegados::encaminha("controller", action.args()),
        Command::Gateway { action } => delegados::encaminha("gateway", action.args()),
        Command::Worker { action } => delegados::encaminha("worker", action.args()),
        Command::Update { action } => delegados::encaminha("update", action.args()),
        Command::Version => versao(),
    }
    Ok(())
}

/// `aic version`: a versão deste comando e, se o computador grava uma release, a do sistema.
fn versao() {
    println!(
        "{}",
        ai_cluster_common::brand::banner(
            "aic",
            env!("CARGO_PKG_VERSION"),
            "um comando para todo o AI Cluster"
        )
    );
    if let Ok(release) = std::fs::read_to_string("/etc/aic-release") {
        let release = release.trim();
        if !release.is_empty() {
            println!("sistema {release}");
        }
    }
}

async fn run_cluster(
    controller: &str,
    action: ClusterAction,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        ClusterAction::Nodes => print_nodes(controller).await?,
        ClusterAction::Topology => print_topology(controller).await?,
        ClusterAction::Node { action } => match action {
            NodeAction::Show { id } => print_node(controller, &id).await?,
            NodeAction::Approve {
                id,
                fingerprint,
                trust_listed,
            } => approve_node(controller, &id, fingerprint, trust_listed).await?,
            NodeAction::Retire { id } => {
                let mut client = connect_operator(controller.to_string()).await?;
                let reply = client
                    .retire_node(ai_cluster_protocol::cluster::RetireNodeRequest {
                        node_id: id.clone(),
                    })
                    .await?
                    .into_inner();
                if reply.retired {
                    println!(
                        "WORKER retirado no={id} reenfileiradas={}; para devolvê-lo: aic cluster node approve {id} --fingerprint <a impressão de antes>",
                        reply.requeued
                    );
                } else {
                    println!("WORKER não encontrado no={id}");
                }
            }
            NodeAction::Revoke { id } => revoke_node(controller, &id).await?,
            NodeAction::Remove { id } => {
                let mut client = connect_operator(controller.to_string()).await?;
                let reply = client
                    .remove_node(ai_cluster_protocol::cluster::RemoveNodeRequest {
                        node_id: id.clone(),
                    })
                    .await?
                    .into_inner();
                if reply.removed {
                    println!("WORKER removido no={id} reenfileiradas={}", reply.requeued);
                } else {
                    println!("WORKER não encontrado no={id}");
                }
            }
        },
        ClusterAction::Install { action } => match action {
            InstallAction::List => print_installs(controller).await?,
            InstallAction::Approve { fingerprint, disk } => {
                let mut client = connect_operator(controller.to_string()).await?;
                let approved = client
                    .approve_install(ai_cluster_protocol::cluster::ApproveInstallRequest {
                        fingerprint: fingerprint.clone(),
                        disk: disk.clone(),
                    })
                    .await?
                    .into_inner()
                    .approved;
                if approved {
                    println!("INSTALAÇÃO aprovada fingerprint={fingerprint} apaga={disk}");
                } else {
                    println!("INSTALAÇÃO não encontrada fingerprint={fingerprint}");
                }
            }
            InstallAction::Reject { fingerprint } => {
                let mut client = connect_operator(controller.to_string()).await?;
                let rejected = client
                    .reject_install(ai_cluster_protocol::cluster::RejectInstallRequest {
                        fingerprint: fingerprint.clone(),
                    })
                    .await?
                    .into_inner()
                    .rejected;
                let what = if rejected {
                    "recusada"
                } else {
                    "não encontrada"
                };
                println!("INSTALAÇÃO {what} fingerprint={fingerprint}");
            }
            InstallAction::Open { minutes } => set_enrollment(controller, minutes).await?,
            InstallAction::Close => set_enrollment(controller, 0).await?,
        },
        ClusterAction::Reservation { action } => match action {
            ReservationAction::List => print_reservations(controller).await?,
            ReservationAction::Add {
                node,
                request_id,
                minutes,
                slots,
            } => {
                let ttl_ms = u64::from(minutes) * 60_000;
                run_reserve(controller, &node, &request_id, ttl_ms, slots).await?
            }
            ReservationAction::Remove { node } => release_reservation(controller, &node).await?,
        },
        ClusterAction::Metrics => print_metrics(controller).await?,
        ClusterAction::Model { action } => match action {
            ModelAction::Manifest { dir, name, version } => {
                let manifest = ai_cluster_engine::manifest::Manifest::build(&dir, &name, &version)?;
                std::fs::write(
                    dir.join(ai_cluster_engine::manifest::FILE),
                    manifest.to_json(),
                )?;
                for file in &manifest.files {
                    println!("{}  {:>12}  {}", file.sha256, file.bytes, file.path);
                }
                println!(
                    "manifest de {name} {version} gravado ({} arquivos)",
                    manifest.files.len()
                );
            }
            ModelAction::List => print_models(controller).await?,
            ModelAction::Start { id, ctx } => start_model(controller, &id, ctx).await?,
            ModelAction::Stop => stop_model(controller).await?,
            ModelAction::Status => model_status(controller).await?,
            ModelAction::Key { action } => model_key(controller, action).await?,
            ModelAction::Check { id, ctx } => check_model(controller, &id, ctx).await?,
            ModelAction::Verify { dir } => {
                match ai_cluster_engine::manifest::Manifest::load(&dir)? {
                    Some(manifest) => {
                        manifest.verify(&dir)?;
                        println!(
                            "ok: {} {} confere ({} arquivos)",
                            manifest.name,
                            manifest.version,
                            manifest.files.len()
                        );
                    }
                    None => return Err(format!("{}: sem manifest.json", dir.display()).into()),
                }
            }
        },
        ClusterAction::Events { limit, follow } => print_events(controller, limit, follow).await?,
        ClusterAction::Request { action } => match action {
            RequestAction::Trace { request_id } => print_trace(controller, &request_id).await?,
            RequestAction::Cancel { request_id } => {
                let mut client = connect_operator(controller.to_string()).await?;
                let reply = client
                    .cancel_request(CancelRequestRequest {
                        request_id: request_id.clone(),
                    })
                    .await?
                    .into_inner();
                println!(
                    "requisicao={request_id} removidas={} em_voo={}",
                    reply.removed, reply.in_flight
                );
            }
        },
        ClusterAction::Benchmark {
            size,
            repetitions,
            local,
            min_threads,
            request_id,
            priority,
            require_attested,
        } => {
            run_benchmark(
                controller,
                size,
                repetitions,
                local,
                min_threads,
                &request_id,
                priority,
                require_attested,
            )
            .await?
        }
        ClusterAction::Graph { size } => run_graph(controller, size).await?,
        ClusterAction::Rollout {
            version,
            url,
            sha256,
            image_sha256,
            signature,
            linux_signature,
            cancel,
        } => {
            let mut client = connect_operator(controller.to_string()).await?;
            let release = (!cancel).then_some(Release {
                version: version.clone(),
                url,
                sha256,
                image_sha256,
                signature,
                linux_signature,
            });
            client.set_release(SetReleaseRequest { release }).await?;
            println!(
                "ROLLOUT {}",
                if cancel {
                    "cancelado".into()
                } else {
                    format!("versão={version}")
                }
            );
        }
        ClusterAction::Dashboard { watch } => print_dashboard(controller, watch).await?,
        ClusterAction::Invite(convite) => delegados::encaminha("cluster", convite.args()),
    }
    Ok(())
}

/// Abre (`minutes` > 0) ou fecha (0) a porta de instalação do controller.
async fn set_enrollment(endpoint: &str, minutes: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let until = client
        .set_enrollment(ai_cluster_protocol::cluster::SetEnrollmentRequest { minutes })
        .await?
        .into_inner()
        .open_until_unix_ms;
    if until == 0 {
        println!("INSTALAÇÃO porta fechada");
    } else {
        // O controller limita a janela a 120 minutos: mostra o efetivo, não o pedido.
        println!("INSTALAÇÃO porta aberta minutos={}", minutes.min(120));
    }
    Ok(())
}

/// Enqueues the diamond graph and polls until it drains.
///
/// A single-node run cannot show branch parallelism; what it shows is that the
/// gating works: B and C wait for A, and D waits for both. Run it against two
/// workers to see B and C land on different nodes.
async fn run_graph(endpoint: &str, size: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let response = client
        .enqueue_graph(EnqueueGraphRequest {
            size,
            seed: 42,
            request_id: String::new(),
            keep_result: false,
            priority: 0,
        })
        .await?
        .into_inner();
    println!(
        "grafo enfileirado={} bloqueados={} tamanho={size} requisicao={}",
        response.enqueued, response.blocked, response.request_id
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let report = loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let report = client
            .work_report(WorkReportRequest { limit: 0 })
            .await?
            .into_inner();
        if report.pending == 0 || tokio::time::Instant::now() >= deadline {
            break report;
        }
    };
    print_report(&report);
    Ok(())
}

/// Retém um worker para uma requisição futura (G6).
///
/// A reserva é criada antes de o trabalho existir: o cliente diz "este worker é meu para a
/// requisição X" e depois enfileira a requisição X com esse mesmo id. Um `granted=false`
/// significa que o worker não está disponível para reserva — normalmente porque saiu ou está
/// drenando — e é reportado sem erro, porque não reservar não é uma falha de transporte.
async fn run_reserve(
    endpoint: &str,
    node: &str,
    request_id: &str,
    ttl_ms: u64,
    slots: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let response = client
        .reserve(ReserveRequest {
            node_id: node.to_string(),
            request_id: request_id.to_string(),
            ttl_ms,
            slots,
        })
        .await?
        .into_inner();
    if response.granted {
        println!(
            "RESERVA concedida no={} requisicao={} expira_em_unix_ms={}",
            node, request_id, response.expires_at_unix_ms
        );
    } else {
        println!("RESERVA recusada no={node} motivo={}", response.message);
        return Err(format!("worker {node} não está disponível para reserva").into());
    }
    Ok(())
}

/// Lista as reservas ativas: worker, requisição dona e vencimento.
async fn print_reservations(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reservations = client
        .list_reservations(ListReservationsRequest {})
        .await?
        .into_inner()
        .reservations;
    println!("RESERVAS {}", reservations.len());
    println!(
        "{:<20} {:<16} {:>14} {:>6}",
        "WORKER", "REQUISIÇÃO", "EXPIRA_EM_MS", "SLOTS"
    );
    for reservation in reservations {
        println!(
            "{:<20} {:<16} {:>14} {:>6}",
            clean(&reservation.node_id),
            clean(&reservation.request_id),
            reservation.expires_at_unix_ms,
            // 0 guarda o worker inteiro
            if reservation.slots == 0 {
                "todos".to_string()
            } else {
                reservation.slots.to_string()
            }
        );
    }
    Ok(())
}

/// Libera a reserva de um worker antes do vencimento. Um worker sem reserva não é erro.
async fn approve_node(
    endpoint: &str,
    node: &str,
    fingerprint: Option<String>,
    trust_listed: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let fingerprint = match fingerprint {
        Some(given) => given.trim().to_ascii_lowercase(),
        None if !trust_listed => {
            return Err(
                "informe --fingerprint (a que o worker imprimiu ao registrar-se) ou --trust-listed"
                    .into(),
            );
        }
        None => {
            eprintln!(
                "AVISO: --trust-listed; aprovando a credencial que o controller mostra agora. Confira com a impressão que o worker imprimiu ao registrar-se."
            );
            client
                .list_nodes(ListNodesRequest {})
                .await?
                .into_inner()
                .nodes
                .into_iter()
                .find(|known| known.identity.as_ref().is_some_and(|id| id.node_id == node))
                .map(|known| known.fingerprint)
                .unwrap_or_default()
        }
    };
    if fingerprint.is_empty() {
        println!("WORKER não encontrado no={node}");
        return Err("worker não encontrado".into());
    }
    let approved = client
        .approve_node(ApproveNodeRequest {
            node_id: node.to_string(),
            fingerprint,
        })
        .await?
        .into_inner()
        .approved;
    if approved {
        println!("WORKER aprovado no={node}");
    } else {
        println!("WORKER não encontrado no={node}");
    }
    Ok(())
}

async fn print_installs(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client
        .list_pending_installs(ai_cluster_protocol::cluster::ListPendingInstallsRequest {})
        .await?
        .into_inner();
    if reply.pending.is_empty() {
        println!("nenhum aparelho aguardando");
    }
    for item in reply.pending {
        let disks = item
            .disks
            .iter()
            .map(|d| format!("{}={}GB", d.name, d.size_bytes / 1_000_000_000))
            .collect::<Vec<_>>()
            .join(",");
        let waiting = reply.now_unix_ms.saturating_sub(item.first_seen_unix_ms) / 1000;
        let state = if item.approved_disk.is_empty() {
            "aguardando".to_string()
        } else {
            format!("aprovado apaga={}", item.approved_disk)
        };
        println!(
            "INSTALAÇÃO fingerprint={} {state} mac={} host={} arch={} firmware={} cpu=\"{}\" núcleos={} ram={}MB discos={disks} espera={waiting}s",
            item.fingerprint,
            item.mac,
            item.host,
            item.arch,
            item.firmware,
            item.cpu_model,
            item.cpu_cores,
            item.ram_bytes / 1_000_000,
        );
    }
    Ok(())
}

async fn revoke_node(endpoint: &str, node: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client
        .revoke_node(RevokeNodeRequest {
            node_id: node.to_string(),
        })
        .await?
        .into_inner();
    if reply.revoked {
        println!(
            "WORKER revogado no={node} reenfileiradas={}",
            reply.requeued
        );
    } else {
        println!("WORKER não encontrado no={node}");
    }
    Ok(())
}

async fn release_reservation(endpoint: &str, node: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let released = client
        .release_reservation(ReleaseReservationRequest {
            node_id: node.to_string(),
        })
        .await?
        .into_inner()
        .released;
    if released {
        println!("RESERVA liberada no={node}");
    } else {
        println!("RESERVA não encontrada no={node}");
    }
    Ok(())
}

/// Texto vindo do controller, seguro para a tela do operador: controle (ESC, CSI, OSC) e os
/// caracteres invisíveis que reordenam ou escondem texto viram `?`. O worker escolhe o próprio id e a
/// própria versão ao se registrar, mesmo pendente, então a CLI não pode confiar neles.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            let hidden = matches!(
                c,
                '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
            );
            if c.is_control() || hidden { '?' } else { c }
        })
        .collect()
}

fn sanitize_node(
    mut node: ai_cluster_protocol::cluster::NodeSnapshot,
) -> ai_cluster_protocol::cluster::NodeSnapshot {
    node.state = clean(&node.state);
    if let Some(identity) = node.identity.as_mut() {
        for field in [
            &mut identity.node_id,
            &mut identity.device_id,
            &mut identity.architecture,
            &mut identity.os_version,
            &mut identity.runtime_version,
        ] {
            *field = clean(field);
        }
    }
    if let Some(capability) = node.capability.as_mut() {
        for list in [
            &mut capability.storage_classes,
            &mut capability.precisions,
            &mut capability.accelerators,
        ] {
            list.iter_mut().for_each(|item| *item = clean(item));
        }
        capability.storage_device = clean(&capability.storage_device);
        for disk in &mut capability.storage_devices {
            disk.name = clean(&disk.name);
        }
    }
    node
}

fn sanitize_gateway(
    mut gateway: ai_cluster_protocol::cluster::GatewayInfo,
) -> ai_cluster_protocol::cluster::GatewayInfo {
    gateway.gateway_id = clean(&gateway.gateway_id);
    gateway.url = clean(&gateway.url);
    for model in &mut gateway.models {
        model.model_id = clean(&model.model_id);
        model
            .shard_nodes
            .iter_mut()
            .for_each(|node| *node = clean(node));
    }
    gateway
}

async fn start_model(
    endpoint: &str,
    id: &str,
    context: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client
        .start_model(StartModelRequest {
            model_id: id.to_string(),
            context,
        })
        .await?
        .into_inner();
    println!(
        "{} {}",
        if reply.ok { "OK " } else { "NOK" },
        clean(&reply.message)
    );
    if let Some(check) = &reply.check {
        let gb = |bytes: u64| bytes as f64 / 1_073_741_824.0;
        println!(
            "pesos {:.2} GB + contexto {:.2} GB + margem {:.2} GB = {:.2} GB; disponível {:.2} GB em {} workers",
            gb(check.weights_bytes),
            gb(check.kv_bytes),
            gb(check.margin_bytes),
            gb(check.required_bytes),
            gb(check.available_bytes),
            check.nodes
        );
    }
    if !reply.ok {
        std::process::exit(1);
    }
    println!(
        "geração {}; o gateway carrega o modelo (acompanhe com `model status`)",
        reply.generation
    );
    for key in reply.keys {
        println!(
            "CHAVE DE API tenant={} {}  (guarde agora: o controller guarda só o hash)",
            clean(&key.tenant),
            clean(&key.key)
        );
    }
    Ok(())
}

async fn model_key(endpoint: &str, action: KeyAction) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    match action {
        KeyAction::Add {
            tenant,
            models,
            tokens,
            class,
        } => key_request(endpoint, tenant, models, tokens, class, false).await,
        KeyAction::Rotate { tenant } => {
            key_request(endpoint, tenant, Vec::new(), 0, String::new(), true).await
        }
        KeyAction::Remove { tenant } => {
            let removed = client
                .remove_key(KeyRemoveRequest {
                    tenant: tenant.clone(),
                })
                .await?
                .into_inner()
                .removed;
            if removed {
                println!("tenant {} removido", clean(&tenant));
                Ok(())
            } else {
                Err(format!("o tenant {} não existe", clean(&tenant)).into())
            }
        }
        KeyAction::List => {
            let keys = client.list_keys(KeyListRequest {}).await?.into_inner().keys;
            println!("CHAVES {}", keys.len());
            println!(
                "{:<20} {:<12} {:>10} {:<12} MODELOS",
                "TENANT", "PREFIXO", "TOKENS", "CLASSE"
            );
            for key in keys {
                println!(
                    "{:<20} {:<12} {:>10} {:<12} {}",
                    clean(&key.tenant),
                    clean(&key.prefix),
                    if key.tokens == 0 {
                        "-".to_string()
                    } else {
                        key.tokens.to_string()
                    },
                    if key.class.is_empty() {
                        "-"
                    } else {
                        key.class.as_str()
                    },
                    if key.models.is_empty() {
                        "todos".to_string()
                    } else {
                        clean(&key.models.join(","))
                    },
                );
            }
            Ok(())
        }
    }
}

async fn key_request(
    endpoint: &str,
    tenant: String,
    models: Vec<String>,
    tokens: u64,
    class: String,
    rotate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client
        .add_key(KeyAddRequest {
            tenant,
            models,
            tokens,
            class,
            rotate,
        })
        .await?
        .into_inner();
    match (reply.ok, reply.key) {
        (true, Some(key)) => {
            println!(
                "CHAVE DE API tenant={} {}  (guarde agora: o controller guarda só o hash)",
                clean(&key.tenant),
                clean(&key.key)
            );
            Ok(())
        }
        _ => Err(clean(&reply.message).into()),
    }
}

async fn stop_model(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client.stop_model(StopModelRequest {}).await?.into_inner();
    println!("modelo parado (geração {})", reply.generation);
    Ok(())
}

async fn model_status(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client
        .model_status(ModelStatusRequest {})
        .await?
        .into_inner();
    let Some(desired) = reply.desired else {
        println!("nenhum modelo pedido (use `model start <id>`)");
        return Ok(());
    };
    println!(
        "MODELO {} (geração {}, contexto {})",
        clean(&desired.model_id),
        desired.generation,
        if desired.context == 0 {
            "padrão".to_string()
        } else {
            desired.context.to_string()
        }
    );
    if desired.endpoints.is_empty() {
        println!("workers: nenhum (o modelo roda só no gateway)");
    } else {
        for worker in &desired.endpoints {
            println!(
                "worker {} em {}",
                clean(&worker.node_id),
                clean(&worker.address)
            );
        }
    }
    if let Some(running) = reply.running {
        let state = match ModelState::try_from(running.state) {
            Ok(ModelState::ModelReady) => "PRONTO",
            Ok(ModelState::ModelLoading) => "CARREGANDO",
            Ok(ModelState::ModelFailed) => "FALHOU",
            Ok(ModelState::ModelStopped) => "PARADO",
            _ => "?",
        };
        println!(
            "estado {state}  {:.2} tokens/s no benchmark{}",
            running.tokens_per_second,
            if running.reason.is_empty() {
                String::new()
            } else {
                format!("  ({})", clean(&running.reason))
            }
        );
    }
    for prefix in reply.key_prefixes {
        println!("chave {}...", clean(&prefix));
    }
    for warning in reply.warnings {
        println!("AVISO {}", clean(&warning));
    }
    Ok(())
}

async fn check_model(
    endpoint: &str,
    id: &str,
    context: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client
        .check_model(CheckModelRequest {
            model_id: id.to_string(),
            context,
        })
        .await?
        .into_inner();
    let gb = |bytes: u64| bytes as f64 / 1_073_741_824.0;
    println!(
        "{} {}",
        if reply.ok { "OK " } else { "NOK" },
        clean(&reply.message)
    );
    if reply.gateway_bytes > 0 {
        println!(
            "gateway: o llama-server ocupa cerca de {:.2} GB na máquina dele",
            gb(reply.gateway_bytes)
        );
    }
    if reply.estimated_tokens_per_second > 0.0 {
        println!(
            "velocidade estimada ~{:.1} tokens/s (a partir do modelo em execução, mesmos workers)",
            reply.estimated_tokens_per_second
        );
    }
    println!(
        "pesos {:.2} GB + contexto {:.2} GB + margem {:.2} GB = {:.2} GB; disponível {:.2} GB em {} workers",
        gb(reply.weights_bytes),
        gb(reply.kv_bytes),
        gb(reply.margin_bytes),
        gb(reply.required_bytes),
        gb(reply.available_bytes),
        reply.nodes
    );
    if !reply.ok {
        std::process::exit(1);
    }
    Ok(())
}

async fn print_models(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client.list_models(ListModelsRequest {}).await?.into_inner();
    if reply.directory.is_empty() {
        println!("O controller não tem pasta de modelos (defina AI_CLUSTER_MODELS_DIR).");
        return Ok(());
    }
    println!("MODELOS {} em {}", reply.models.len(), reply.directory);
    println!(
        "{:<34} {:<12} {:<10} {:>9} {:<8} {:>8} {:>7} {:>8}  MOTIVO",
        "MODELO", "FORMATO", "ARQ", "PARAMS", "PREC", "GB", "CTX", "ROD"
    );
    for model in reply.models {
        let (params, gb) = (
            model.parameters as f64 / 1e9,
            model.bytes as f64 / 1_073_741_824.0,
        );
        println!(
            "{:<34} {:<12} {:<10} {:>8.2}B {:<8} {:>8.2} {:>7} {:>8}  {}",
            clean(&model.id),
            clean(&model.format),
            clean(&model.architecture),
            params,
            clean(&model.precision),
            gb,
            model.context,
            if model.supported { "sim" } else { "não" },
            clean(&model.reason),
        );
    }
    Ok(())
}

async fn print_nodes(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let nodes = client
        .list_nodes(ListNodesRequest {})
        .await?
        .into_inner()
        .nodes
        .into_iter()
        .map(sanitize_node)
        .collect::<Vec<_>>();
    println!("WORKERS {}", nodes.len());
    println!(
        "{:<20} {:<10} {:<10} {:>7} {:>9} {:>7} {:>8} {:>8} {:>7} {:>7} {:<10} {:<12} {:>11} {:<10} {:<8} {:<16} {:>6}",
        "WORKER",
        "ESTADO",
        "ARQ",
        "THREADS",
        "RAM_GB",
        "COMPUTE",
        "SAÚDE",
        "CPU_%",
        "TEMP_C",
        "SWAP_%",
        "CONFIANÇA",
        "ARMAZ",
        "ARMAZ_SCORE",
        "VERSÃO",
        "MEDIDA",
        "IMPRESSÃO",
        "VELOC"
    );
    for node in nodes {
        let identity = node.identity.unwrap_or_default();
        let capability = node.capability.unwrap_or_default();
        let health = node.health.unwrap_or_default();
        // Empty means "not measured" — a node whose /sys/block could not be read — so it
        // must not look like a node with no storage.
        let storage = if capability.storage_classes.is_empty() {
            "-".to_string()
        } else {
            capability.storage_classes.join(",")
        };
        println!(
            "{:<20} {} {:<10} {:>7} {:>9.2} {:>7.2} {:>8.2} {:>8.2} {:>7.2} {:>7.2} {:<10} {:<12} {:>11} {:<10} {:<8} {:<16} {:>6.2}",
            identity.node_id,
            // a largura vem antes da cor: os códigos de escape não contam como coluna
            ai_cluster_common::brand::paint(
                &format!("{:<10}", node.state),
                ai_cluster_common::brand::state_tone(&node.state)
            ),
            identity.architecture,
            capability.cpu_threads,
            capability.memory_bytes as f64 / 1_073_741_824.0,
            capability.compute_score,
            health.health_score,
            health.cpu_utilization * 100.0,
            health.temperature_c,
            health.swap_utilization * 100.0,
            if node.trusted { "aprovado" } else { "pendente" },
            storage,
            // Zero is "not measured" (the probe could not write), never a slow disk.
            if capability.storage_score > 0.0 {
                format!("{:.2}", capability.storage_score)
            } else {
                "-".to_string()
            },
            if identity.runtime_version.is_empty() {
                "-"
            } else {
                &identity.runtime_version
            },
            if node.attested { "confere" } else { "-" },
            if node.fingerprint.is_empty() {
                "-"
            } else {
                &node.fingerprint
            },
            node.estimated_speed,
        );
    }
    Ok(())
}

/// O papel de um worker, pelo que o controller sabe: o id de recuperação (`-rec`) é o boot de rede,
/// que só marca presença; um cache de fatias maior que zero é um worker que guarda partes de modelo.
fn node_role(node_id: &str, shard_cache_bytes: u64) -> &'static str {
    if node_id.ends_with(RECOVERY_NODE_SUFFIX) {
        "Recuperação (só presença, não recebe trabalho)"
    } else if shard_cache_bytes > 0 {
        "Computação + fatias de modelo"
    } else {
        "Computação"
    }
}

/// A árvore do cluster: o controller na raiz, um ramo por papel, um worker por folha. Linhas puras para
/// poder testar. Papéis vazios não aparecem. O gateway não se registra no controller, então não
/// entra aqui.
fn topology_lines(
    controller: &str,
    nodes: &[ai_cluster_protocol::cluster::NodeSnapshot],
    gateways: &[ai_cluster_protocol::cluster::GatewayInfo],
) -> Vec<String> {
    let mut groups: Vec<(&'static str, Vec<String>)> = Vec::new();
    for node in nodes {
        let identity = node.identity.clone().unwrap_or_default();
        let capability = node.capability.clone().unwrap_or_default();
        let role = node_role(&identity.node_id, capability.shard_cache_bytes);
        let mut facts = format!(
            "{} {} {}t {:.1}GB v{}",
            ai_cluster_common::brand::paint(
                &node.state,
                ai_cluster_common::brand::state_tone(&node.state)
            ),
            identity.architecture,
            capability.cpu_threads,
            capability.memory_bytes as f64 / 1_073_741_824.0,
            if identity.runtime_version.is_empty() {
                "-"
            } else {
                &identity.runtime_version
            },
        );
        if capability.shard_cache_bytes > 0 {
            facts.push_str(&format!(" cache={}MB", capability.shard_cache_bytes >> 20));
        }
        if !node.trusted {
            facts.push_str(" PENDENTE de aprovação");
        }
        let line = format!("{}  {facts}", identity.node_id);
        match groups.iter_mut().find(|(name, _)| *name == role) {
            Some((_, lines)) => lines.push(line),
            None => groups.push((role, vec![line])),
        }
    }
    // O gateway entra como mais um ramo: o que ele serve e em que workers ficam as fatias.
    if !gateways.is_empty() {
        let lines = gateways
            .iter()
            .map(|gateway| {
                let models = gateway
                    .models
                    .iter()
                    .map(|model| {
                        if model.shard_nodes.is_empty() {
                            format!("{} (inteiro no gateway)", model.model_id)
                        } else {
                            format!(
                                "{} → fatias em {}",
                                model.model_id,
                                model.shard_nodes.join(",")
                            )
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                format!(
                    "{}  {}  {}  {}",
                    gateway.gateway_id,
                    gateway.url,
                    if gateway.alive { "VIVO" } else { "SEM SINAL" },
                    if models.is_empty() {
                        "sem modelos"
                    } else {
                        &models
                    },
                )
            })
            .collect();
        groups.push(("Gateway de inferência", lines));
    }
    let mut out = vec![format!(
        "controller {controller}  ({} workers)",
        nodes.len()
    )];
    let last_group = groups.len().saturating_sub(1);
    for (g, (role, lines)) in groups.iter().enumerate() {
        let (branch, pad) = if g == last_group {
            ("└─", "   ")
        } else {
            ("├─", "│  ")
        };
        out.push(format!("{branch} {role} ({})", lines.len()));
        for (i, line) in lines.iter().enumerate() {
            let leaf = if i + 1 == lines.len() {
                "└─"
            } else {
                "├─"
            };
            out.push(format!("{pad}{leaf} {line}"));
        }
    }
    out
}

async fn print_topology(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let nodes = client
        .list_nodes(ListNodesRequest {})
        .await?
        .into_inner()
        .nodes
        .into_iter()
        .map(sanitize_node)
        .collect::<Vec<_>>();
    // Controller antigo não conhece o RPC: árvore sem o ramo, sem erro. Qualquer outra falha (rede,
    // permissão) é avisada, para a ausência do gateway não passar por "não há gateway".
    let gateways = match client
        .list_gateways(ai_cluster_protocol::cluster::ListGatewaysRequest {})
        .await
    {
        Ok(response) => response
            .into_inner()
            .gateways
            .into_iter()
            .map(sanitize_gateway)
            .collect(),
        Err(status) if status.code() == tonic::Code::Unimplemented => Vec::new(),
        Err(status) => {
            eprintln!(
                "aviso: não consegui listar os gateways: {}",
                clean(status.message())
            );
            Vec::new()
        }
    };
    for line in topology_lines(endpoint, &nodes, &gateways) {
        println!("{line}");
    }
    Ok(())
}

/// Um worker só, com tudo que o controller sabe sobre ele (B4).
///
/// A tabela de `nodes` cabe numa linha por worker, então deixa de fora o que não cabe: o compute
/// medido, as classes de armazenamento, a pressão de memória e a reserva do worker. Este é o
/// lugar onde esses campos aparecem juntos.
///
/// Sem RPC de worker único: `ListNodes` já traz todos numa chamada só, então filtrar aqui não
/// custa uma ida a mais ao controller. Um id desconhecido é informativo, não erro — o mesmo
/// tratamento de um trace de requisição desconhecida.
///
/// Os campos de saúde que o worker nunca preenche —
/// `thermal_throttling` — ficam de fora. Imprimi-los zerados diria que valem zero, que é
/// diferente de não existirem.
async fn print_node(endpoint: &str, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let node = client
        .list_nodes(ListNodesRequest {})
        .await?
        .into_inner()
        .nodes
        .into_iter()
        .find(|node| {
            node.identity
                .as_ref()
                .is_some_and(|identity| identity.node_id == id)
        });
    let Some(node) = node else {
        println!("WORKER {} não encontrado", clean(id));
        return Ok(());
    };
    let node = sanitize_node(node);
    let identity = node.identity.unwrap_or_default();
    let capability = node.capability.unwrap_or_default();
    let health = node.health.unwrap_or_default();
    let reservation = client
        .list_reservations(ListReservationsRequest {})
        .await?
        .into_inner()
        .reservations
        .into_iter()
        .find(|reservation| reservation.node_id == id);

    println!("WORKER {}", identity.node_id);
    println!(
        "  estado={} arquitetura={} confianca={}",
        node.state,
        identity.architecture,
        if node.trusted { "aprovado" } else { "pendente" }
    );
    println!(
        "  dispositivo={} so={} runtime={}",
        identity.device_id, identity.os_version, identity.runtime_version
    );
    println!(
        "  registrado_em_unix_ms={} ultimo_heartbeat_unix_ms={}",
        node.registered_at_unix_ms, node.last_heartbeat_unix_ms
    );
    println!("CAPABILITY");
    println!(
        "  threads={} memoria_bytes={} compute_score={:.4}",
        capability.cpu_threads, capability.memory_bytes, capability.compute_score
    );
    // Zero means "not remeasured", never "compute zero" — the same convention the ARMAZ
    // column uses for "not measured". A pinned node never remeasures, so it stays `-`.
    let (current, confidence) = if capability.current_compute_score > 0.0 {
        (
            format!("{:.4}", capability.current_compute_score),
            format!("{:.2}", capability.compute_confidence),
        )
    } else {
        ("-".to_string(), "-".to_string())
    };
    println!("  current_compute_score={current} compute_confidence={confidence}");
    // The registration benchmark's distribution: the median is the typical run and the
    // deviation how much the runs disagreed. Zero median means "not measured" (a pinned
    // score runs no benchmark), never "compute zero".
    let median = if capability.compute_median_score > 0.0 {
        format!("{:.4}", capability.compute_median_score)
    } else {
        "-".to_string()
    };
    // Aggregate memory bandwidth with the cores busy together; against the single-thread score above
    // it says whether memory scales with the cores. Zero is "not measured".
    println!(
        "  memory_bandwidth_loaded_score={:.2}",
        capability.memory_bandwidth_loaded_score
    );
    // O_DIRECT read rate and the block device the storage numbers were taken on; zero / empty
    // mean "not measured" (tmpfs refuses O_DIRECT; containers have no block device to name).
    println!(
        "  storage_read_score={:.4} storage_device={}",
        capability.storage_read_score,
        if capability.storage_device.is_empty() {
            "-"
        } else {
            &capability.storage_device
        }
    );
    // Each measured disk, not only the one the probes ran on; zero is "not measured".
    for disk in &capability.storage_devices {
        println!(
            "  disco {}: escrita={:.4} leitura={:.4}",
            disk.name, disk.write_score, disk.read_score
        );
    }
    // Packet loss the TCP stack hid (retransmits) and run-to-run jitter of the link probe; "n/d" is
    // a probe with fewer than two runs, which cannot tell stable from unmeasured.
    let jitter = if capability.network_jitter_measured {
        format!("{:.3}", capability.network_jitter)
    } else {
        "n/d".to_string()
    };
    println!(
        "  network_retransmit_ratio={:.4} network_jitter={jitter}",
        capability.network_retransmit_ratio
    );
    // Baseline confidence: 1 = the registration runs agreed, low = the node was disturbed then.
    let baseline = if capability.baseline_confidence > 0.0 {
        format!("{:.3}", capability.baseline_confidence)
    } else {
        "-".to_string()
    };
    println!(
        "  compute_median_score={median} compute_deviation={:.3} baseline_confidence={baseline}",
        capability.compute_deviation
    );
    // Zero means "not measured" — a container's veth has no negotiated link rate, and a
    // pinned node is never read — never "no network", the same convention as the compute
    // fields above. The number is the link relative to the 1 GbE reference.
    let network = if capability.network_score > 0.0 {
        format!("{:.2}", capability.network_score)
    } else {
        "-".to_string()
    };
    let storage_score = if capability.storage_score > 0.0 {
        format!("{:.2}", capability.storage_score)
    } else {
        "-".to_string()
    };
    let memory_score = if capability.memory_bandwidth_score > 0.0 {
        format!("{:.2}", capability.memory_bandwidth_score)
    } else {
        "-".to_string()
    };
    let storage_latency = if capability.storage_latency_ms > 0.0 {
        format!("{:.3}", capability.storage_latency_ms)
    } else {
        "-".to_string()
    };
    println!(
        "  armazenamento={} armaz_score={storage_score} armaz_lat_ms={storage_latency} mem_bw={memory_score} precisoes={} aceleradores={}",
        list_or_dash(&capability.storage_classes),
        list_or_dash(&capability.precisions),
        list_or_dash(&capability.accelerators),
    );
    let achieved = if capability.network_achieved_score > 0.0 {
        format!("{:.2}", capability.network_achieved_score)
    } else {
        "-".to_string()
    };
    let current_network = if capability.current_network_score > 0.0 {
        format!("{:.2}", capability.current_network_score)
    } else {
        "-".to_string()
    };
    println!("  rede_score={network} rede_alcancada={achieved} rede_atual={current_network}");
    println!("SAÚDE");
    println!("  score={:.2}", health.health_score);
    println!(
        "  cpu={:.2}% memoria={:.2}% swap={:.2}%",
        health.cpu_utilization * 100.0,
        health.memory_utilization * 100.0,
        health.swap_utilization * 100.0,
    );
    // Zero is "not measured" (no idle remeasurement yet), never a dead disk.
    let storage_health = if health.storage_health > 0.0 {
        format!("{:.2}", health.storage_health)
    } else {
        "-".to_string()
    };
    // Zero is "not measured" (no idle probe yet), never an instant link.
    let latency = if health.network_latency_ms > 0.0 {
        format!("{:.3}", health.network_latency_ms)
    } else {
        "-".to_string()
    };
    // No battery in view is not an empty one.
    let battery = if health.battery_level > 0.0 || health.on_battery {
        format!(
            "{:.0}%{}",
            health.battery_level * 100.0,
            if health.on_battery {
                " descarregando"
            } else {
                ""
            }
        )
    } else {
        "-".to_string()
    };
    println!(
        "  temperatura_c={:.2} iops={:.2} armaz_saude={storage_health} latencia_ms={latency} bateria={battery}",
        health.temperature_c, health.disk_io_ops
    );
    match reservation {
        Some(reservation) => {
            println!("RESERVA");
            println!(
                "  requisicao={} expira_em_unix_ms={}",
                reservation.request_id, reservation.expires_at_unix_ms
            );
        }
        None => println!("RESERVA nenhuma"),
    }
    Ok(())
}

/// Joins a list for display, or `-` when it is empty. Empty means "not measured" for the
/// storage classes, so it must not read as a blank value.
fn list_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(",")
    }
}

/// Prints the controller's recent lifecycle events, oldest first.
async fn print_events(
    endpoint: &str,
    limit: u32,
    follow: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    // assina antes de listar: nada que aconteça entre as duas chamadas se perde (pode repetir um)
    let mut live = if follow {
        Some(
            client
                .watch_events(WatchEventsRequest {})
                .await?
                .into_inner(),
        )
    } else {
        None
    };
    let events = client
        .list_events(ListEventsRequest { limit })
        .await?
        .into_inner()
        .events;
    println!("EVENTOS {}", events.len());
    println!("{:<14} {:<16} DETALHE", "EM_UNIX_MS", "TIPO");
    for event in events {
        println!(
            "{:<14} {:<16} {}",
            event.at_unix_ms, event.kind, event.detail
        );
    }
    if let Some(live) = live.as_mut() {
        while let Some(event) = live.message().await? {
            println!(
                "{:<14} {:<16} {}",
                event.at_unix_ms, event.kind, event.detail
            );
        }
    }
    Ok(())
}

/// Prints the cluster-wide roll-up from the controller (Fase H1).
///
/// Emits `chave=valor` with ASCII keys rather than a column table: `metrics` is the one
/// command a script or a scraper is likely to read, and positional parsing breaks whenever
/// a label changes. The keys match the proto field names, so the output is stable.
async fn print_metrics(endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let metrics = client.get_metrics(GetMetricsRequest {}).await?.into_inner();
    println!(
        "nodes_total={} nodes_available={}",
        metrics.nodes_total, metrics.nodes_available
    );
    println!(
        "units_completed={} units_pending={} units_blocked={} units_failed={}",
        metrics.units_completed, metrics.units_pending, metrics.units_blocked, metrics.units_failed
    );
    println!(
        "total_compute_time_us={} avg_compute_time_us={} total_predicted_compute_us={}",
        metrics.total_compute_time_us,
        metrics.avg_compute_time_us,
        metrics.total_predicted_compute_us
    );
    println!(
        "total_latency_us={} avg_latency_us={}",
        metrics.total_latency_us, metrics.avg_latency_us
    );
    // H1: a média esconde a cauda. p50 é a mediana; p95 mostra o que a cauda sente.
    println!(
        "p50_compute_time_us={} p95_compute_time_us={}",
        metrics.p50_compute_time_us, metrics.p95_compute_time_us
    );
    println!(
        "p50_latency_us={} p95_latency_us={}",
        metrics.p50_latency_us, metrics.p95_latency_us
    );
    Ok(())
}

/// Painel de uma tela (H4): workers, fila de trabalho e reservas ativas no mesmo lugar.
///
/// Junta o que o controlador já expõe. Os campos do painel da spec que ainda não têm
/// fonte — TTFT, ITL, tokens/s, tokens evitados, KV cache, previsto vs real — não
/// aparecem: imprimi-los zerados diria que valem zero, que é diferente de não existirem.
///
/// Sem `--watch` a vista é impressa uma vez e o comando sai, para poder ser lido por um
/// script. Com `--watch N` ela se repete a cada N segundos até Ctrl-C, e a tela é limpa
/// entre uma volta e outra para o painel não virar um log.
async fn print_dashboard(
    endpoint: &str,
    watch_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    loop {
        let nodes = client
            .list_nodes(ListNodesRequest {})
            .await?
            .into_inner()
            .nodes;
        let metrics = client.get_metrics(GetMetricsRequest {}).await?.into_inner();
        let reservations = client
            .list_reservations(ListReservationsRequest {})
            .await?
            .into_inner()
            .reservations;
        // Controller antigo não conhece o RPC: a seção some. Outra falha é avisada, depois do painel
        // (no `--watch` a limpeza da tela apagaria o aviso se ele saísse antes).
        let mut gateway_warning: Option<String> = None;
        let (gateways, gateways_now) = match client
            .list_gateways(ai_cluster_protocol::cluster::ListGatewaysRequest {})
            .await
        {
            Ok(response) => {
                let response = response.into_inner();
                (response.gateways, response.now_unix_ms)
            }
            Err(status) if status.code() == tonic::Code::Unimplemented => (Vec::new(), 0),
            Err(status) => {
                gateway_warning = Some(clean(status.message()));
                (Vec::new(), 0)
            }
        };

        let model = client
            .model_status(ModelStatusRequest {})
            .await
            .ok()
            .map(|response| response.into_inner());

        if watch_secs > 0 {
            // Home + limpa a tela, para a volta seguinte sobrescrever a anterior.
            print!("\x1b[2J\x1b[H");
        }

        // No terminal, o painel com a identidade visual; redirecionado (scripts, testes), o formato
        // de chave=valor de sempre, que não muda. `AI_CLUSTER_COLOR=always` força o painel bonito.
        if ai_cluster_common::brand::color_enabled() {
            for line in pretty_dashboard(
                endpoint,
                &nodes,
                &metrics,
                &reservations,
                &gateways,
                gateways_now,
                model.as_ref(),
            ) {
                println!("{line}");
            }
        } else {
            println!(
                "PAINEL nós_disponíveis={}/{}",
                metrics.nodes_available, metrics.nodes_total
            );
            println!();
            println!(
                "{:<20} {:<12} {:>7} {:>9} {:>8} {:>8} {:>7} {:>7}",
                "WORKER", "ESTADO", "THREADS", "RAM_GB", "COMPUTE", "SAÚDE", "CPU_%", "TEMP_C"
            );
            for node in nodes {
                let identity = node.identity.unwrap_or_default();
                let capability = node.capability.unwrap_or_default();
                let health = node.health.unwrap_or_default();
                println!(
                    "{:<20} {:<12} {:>7} {:>9.2} {:>8.2} {:>8.2} {:>7.2} {:>7.2}",
                    identity.node_id,
                    node.state,
                    capability.cpu_threads,
                    capability.memory_bytes as f64 / 1_073_741_824.0,
                    capability.compute_score,
                    health.health_score,
                    health.cpu_utilization * 100.0,
                    health.temperature_c,
                );
            }
            println!();
            println!(
                "TRABALHO concluídas={} pendentes={} bloqueadas={} falhas={}",
                metrics.units_completed,
                metrics.units_pending,
                metrics.units_blocked,
                metrics.units_failed
            );
            println!(
                "TEMPO compute_total_us={} compute_media_us={} latencia_total_us={} latencia_media_us={} previsto_total_us={}",
                metrics.total_compute_time_us,
                metrics.avg_compute_time_us,
                metrics.total_latency_us,
                metrics.avg_latency_us,
                metrics.total_predicted_compute_us
            );
            println!(
                "TAXA unidades_por_s_1m={:.3} unidades_por_s_5m={:.3} tendencia={}",
                metrics.units_per_second_1m,
                metrics.units_per_second_5m,
                trend(metrics.units_per_second_1m, metrics.units_per_second_5m)
            );
            println!(
                "MEDIAS rede={:.2} escrita={:.2} leitura={:.2} latencia_escrita_ms={:.2}",
                metrics.avg_network_score,
                metrics.avg_storage_score,
                metrics.avg_storage_read_score,
                metrics.avg_storage_latency_ms
            );
            for line in api_lines(&gateways, model.as_ref()) {
                println!("{line}");
            }
            for line in gateway_lines(&gateways, gateways_now) {
                println!("{line}");
            }
            println!();
            println!("RESERVAS {}", reservations.len());
            for reservation in reservations {
                println!(
                    "  {:<20} requisição={} expira_em_unix_ms={}",
                    reservation.node_id, reservation.request_id, reservation.expires_at_unix_ms
                );
            }
        }

        if let Some(warning) = gateway_warning {
            eprintln!("aviso: não consegui listar os gateways: {warning}");
        }
        if watch_secs == 0 {
            return Ok(());
        }
        // Ctrl-C encerra o processo pelo sinal padrão: não é preciso tratá-lo aqui.
        tokio::time::sleep(Duration::from_secs(watch_secs)).await;
    }
}

/// O painel com a identidade visual: logotipo, seções com régua, estados coloridos e a tendência
/// com seta. É o mesmo conteúdo do formato de máquina. Linhas puras, testáveis; sem cor (como num
/// teste) o texto sai igual, só sem os códigos de escape.
fn pretty_dashboard(
    endpoint: &str,
    nodes: &[ai_cluster_protocol::cluster::NodeSnapshot],
    metrics: &ai_cluster_protocol::cluster::MetricsResponse,
    reservations: &[ai_cluster_protocol::cluster::Reservation],
    gateways: &[ai_cluster_protocol::cluster::GatewayInfo],
    now_ms: u64,
    model: Option<&ai_cluster_protocol::cluster::ModelStatusResponse>,
) -> Vec<String> {
    use ai_cluster_common::brand::{self, Tone, paint, rule, state_tone};
    const WIDTH: usize = 78;
    let mut out: Vec<String> = brand::banner(
        "painel",
        env!("CARGO_PKG_VERSION"),
        &format!(
            "{}  -  {}/{} nos disponiveis",
            clean(endpoint),
            metrics.nodes_available,
            metrics.nodes_total
        ),
    )
    .lines()
    .map(String::from)
    .collect();

    out.push(String::new());
    out.push(rule("Nos", WIDTH));
    out.push(paint(
        &format!(
            "    {:<18} {:<11} {:<8} {:>4} {:>8} {:>7} {:>6} {:>6}  {}",
            "NO", "ESTADO", "ARQ", "THR", "RAM", "COMPUTE", "SAUDE", "CPU", "VERSAO"
        ),
        Tone::Dim,
    ));
    if nodes.is_empty() {
        out.push(paint("    nenhum no registrado", Tone::Dim));
    }
    for node in nodes {
        let node = sanitize_node(node.clone());
        let identity = node.identity.clone().unwrap_or_default();
        let capability = node.capability.clone().unwrap_or_default();
        let health = node.health.unwrap_or_default();
        let tone = state_tone(&node.state);
        out.push(format!(
            "  {} {:<18} {} {:<8} {:>3}t {:>6.1}GB {:>7.2} {:>6.2} {:>5.1}%  {}{}",
            paint(brand::glyphs().node, tone),
            identity.node_id,
            paint(&format!("{:<11}", node.state), tone),
            identity.architecture,
            capability.cpu_threads,
            capability.memory_bytes as f64 / 1_073_741_824.0,
            capability.compute_score,
            health.health_score,
            health.cpu_utilization * 100.0,
            if identity.runtime_version.is_empty() {
                "-"
            } else {
                &identity.runtime_version
            },
            if node.trusted {
                String::new()
            } else {
                format!("  {}", paint("pendente de aprovacao", Tone::Warn))
            },
        ));
    }

    out.push(String::new());
    out.push(rule("Trabalho", WIDTH));
    let failures = format!("falhas {}", metrics.units_failed);
    out.push(format!(
        "    concluidas {}   pendentes {}   bloqueadas {}   {}",
        metrics.units_completed,
        metrics.units_pending,
        metrics.units_blocked,
        paint(
            &failures,
            if metrics.units_failed > 0 {
                Tone::Bad
            } else {
                Tone::Dim
            }
        ),
    ));
    out.push(format!(
        "    taxa   1m {:.3}/s   5m {:.3}/s   {}",
        metrics.units_per_second_1m,
        metrics.units_per_second_5m,
        trend(metrics.units_per_second_1m, metrics.units_per_second_5m)
    ));
    out.push(paint(
        &format!(
            "    tempo  compute medio {} us   latencia media {} us   previsto {} us",
            metrics.avg_compute_time_us, metrics.avg_latency_us, metrics.total_predicted_compute_us
        ),
        Tone::Dim,
    ));

    out.push(String::new());
    out.push(rule("Capacidade media dos nos prontos", WIDTH));
    out.push(format!(
        "    rede {:.2}   escrita {:.2}   leitura {:.2}   latencia de escrita {:.2} ms",
        metrics.avg_network_score,
        metrics.avg_storage_score,
        metrics.avg_storage_read_score,
        metrics.avg_storage_latency_ms
    ));

    out.push(String::new());
    out.push(rule("API", WIDTH));
    out.extend(pretty_api(gateways, model));

    if !gateways.is_empty() {
        out.push(String::new());
        out.push(rule("Inferencia", WIDTH));
        for gateway in gateways {
            let gateway = sanitize_gateway(gateway.clone());
            let (word, tone) = if gateway.alive {
                ("VIVO".to_string(), Tone::Ok)
            } else {
                let age = now_ms.saturating_sub(gateway.last_seen_unix_ms) / 1000;
                (format!("SEM SINAL ha {age}s"), Tone::Bad)
            };
            out.push(format!(
                "  {} {}  {}  {}",
                paint(brand::glyphs().node, tone),
                gateway.gateway_id,
                paint(&gateway.url, Tone::Dim),
                paint(&word, tone)
            ));
            match gateway.stats {
                Some(stats) => {
                    out.extend(gateway_detail(&stats).into_iter().map(|l| format!("  {l}")))
                }
                None => out.push(paint("      sem dados (gateway antigo)", Tone::Dim)),
            }
        }
    }

    out.push(String::new());
    out.push(rule("Reservas", WIDTH));
    if reservations.is_empty() {
        out.push(paint("    nenhuma", Tone::Dim));
    }
    for reservation in reservations {
        let left = reservation.expires_at_unix_ms.saturating_sub(now_ms) / 1000;
        out.push(format!(
            "  {} {:<18} {}  {}",
            paint(brand::glyphs().node, Tone::Accent),
            clean(&reservation.node_id),
            clean(&reservation.request_id),
            paint(
                &format!(
                    "{}  expira em {left}s",
                    if reservation.slots == 0 {
                        "no inteiro".to_string()
                    } else {
                        format!("{} slot(s)", reservation.slots)
                    }
                ),
                Tone::Dim
            ),
        ));
    }
    out
}

/// O endereço da API de um gateway, ou o aviso de que ele não o informou.
fn api_address(gateway: &ai_cluster_protocol::cluster::GatewayInfo) -> String {
    let url = clean(&gateway.url);
    if url.is_empty() {
        "(endereço não informado: defina AI_CLUSTER_GATEWAY_URL no gateway)".to_string()
    } else {
        url
    }
}

/// O modelo ativo: (palavra, descrição, tom). Sem modelo pedido, diz como subir um.
fn active_model(
    model: Option<&ai_cluster_protocol::cluster::ModelStatusResponse>,
) -> (&'static str, String, ai_cluster_common::brand::Tone) {
    use ai_cluster_common::brand::Tone;
    let Some(model) = model else {
        return (
            "SEM DADOS",
            "o controller não informou o modelo".into(),
            Tone::Dim,
        );
    };
    let Some(desired) = model.desired.as_ref().filter(|d| !d.model_id.is_empty()) else {
        return (
            "NENHUM",
            "sem modelo ativo: aic cluster model start <id>".into(),
            Tone::Warn,
        );
    };
    let id = clean(&desired.model_id);
    let running = model.running.clone().unwrap_or_default();
    match ModelState::try_from(running.state) {
        Ok(ModelState::ModelReady) => (
            "PRONTO",
            format!("{id}  {:.1} tok/s", running.tokens_per_second),
            Tone::Ok,
        ),
        Ok(ModelState::ModelLoading) => ("CARREGANDO", id, Tone::Warn),
        Ok(ModelState::ModelFailed) => (
            "FALHOU",
            format!("{id}: {}", clean(&running.reason)),
            Tone::Bad,
        ),
        _ => (
            "PEDIDO",
            format!("{id} (o gateway ainda não aplicou)"),
            Tone::Warn,
        ),
    }
}

/// A seção API do painel bonito: onde chamar e que modelo responde.
fn pretty_api(
    gateways: &[ai_cluster_protocol::cluster::GatewayInfo],
    model: Option<&ai_cluster_protocol::cluster::ModelStatusResponse>,
) -> Vec<String> {
    use ai_cluster_common::brand::{self, Tone, paint};
    let mut out = Vec::new();
    if gateways.is_empty() {
        out.push(paint("    nenhum gateway conectado", Tone::Dim));
        return out;
    }
    for gateway in gateways {
        let (word, tone) = if gateway.alive {
            ("NO AR", Tone::Ok)
        } else {
            ("SEM SINAL", Tone::Bad)
        };
        out.push(format!(
            "  {} {}  {}",
            paint(brand::glyphs().node, tone),
            api_address(gateway),
            paint(word, tone)
        ));
    }
    let (word, text, tone) = active_model(model);
    out.push(format!("    modelo  {}  {text}", paint(word, tone)));
    out.push(paint(
        "    POST /v1/chat/completions   GET /v1/models   GET /health",
        Tone::Dim,
    ));
    out
}

/// A API no formato de máquina: uma linha por gateway e uma do modelo.
fn api_lines(
    gateways: &[ai_cluster_protocol::cluster::GatewayInfo],
    model: Option<&ai_cluster_protocol::cluster::ModelStatusResponse>,
) -> Vec<String> {
    let mut out: Vec<String> = gateways
        .iter()
        .map(|gateway| {
            format!(
                "API url={} estado={}",
                clean(&gateway.url),
                if gateway.alive { "no_ar" } else { "sem_sinal" }
            )
        })
        .collect();
    let (word, text, _) = active_model(model);
    out.push(format!("MODELO_ATIVO estado={word} {text}"));
    out
}

/// As quatro linhas do que um gateway mediu: totais, latências, taxas com tendência e cache.
fn gateway_detail(stats: &ai_cluster_protocol::cluster::GatewayStats) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "    requisições={} falhas={} tokens_prompt={} tokens_gerados={}",
        stats.requests, stats.failed, stats.prompt_tokens, stats.generated_tokens
    ));
    out.push(format!(
        "    TTFT p50={:.3}s p95={:.3}s  ITL p50={:.4}s p95={:.4}s  decodificação={:.1} tok/s",
        stats.ttft_p50_seconds,
        stats.ttft_p95_seconds,
        stats.itl_p50_seconds,
        stats.itl_p95_seconds,
        stats.decode_tokens_per_second
    ));
    out.push(format!(
        "    taxa req/s 1m={:.3} 5m={:.3} {}  tok/s 1m={:.2} 5m={:.2} {}",
        stats.requests_per_second_1m,
        stats.requests_per_second_5m,
        trend(stats.requests_per_second_1m, stats.requests_per_second_5m),
        stats.generated_tokens_per_second_1m,
        stats.generated_tokens_per_second_5m,
        trend(
            stats.generated_tokens_per_second_1m,
            stats.generated_tokens_per_second_5m
        )
    ));
    out.push(format!(
        "    KV último={} B máximo={} B  cache hits={} misses={} tokens_evitados={}",
        stats.kv_bytes_last,
        stats.kv_bytes_max,
        stats.cache_hits,
        stats.cache_misses,
        stats.cache_tokens_avoided
    ));
    out
}

/// A seção INFERÊNCIA do painel em chave=valor: um bloco por gateway, com o que ele mediu e a idade
/// do último sinal quando está sem sinal, para ninguém ler números velhos como atuais. Linhas puras,
/// testáveis.
fn gateway_lines(
    gateways: &[ai_cluster_protocol::cluster::GatewayInfo],
    now_ms: u64,
) -> Vec<String> {
    if gateways.is_empty() {
        return Vec::new();
    }
    let mut out = vec![
        String::new(),
        format!("INFERÊNCIA gateways={}", gateways.len()),
    ];
    for gateway in gateways {
        let gateway = sanitize_gateway(gateway.clone());
        let state = if gateway.alive {
            "VIVO".to_string()
        } else {
            let age = now_ms.saturating_sub(gateway.last_seen_unix_ms) / 1000;
            format!("SEM SINAL há {age}s (números abaixo são do último sinal)")
        };
        out.push(format!("  {} {} {state}", gateway.gateway_id, gateway.url));
        let Some(stats) = gateway.stats else {
            out.push("    sem dados (gateway antigo)".to_string());
            continue;
        };
        out.extend(gateway_detail(&stats));
    }
    out
}

/// A tendência entre a taxa de 1 min e a de 5 min: `-` sem as duas, senão a razão e uma seta. A
/// divisão por zero fica aqui, no leitor, em vez de no protocolo.
fn trend(rate_1m: f64, rate_5m: f64) -> String {
    if rate_1m <= 0.0 || rate_5m <= 0.0 {
        return "-".to_string();
    }
    let ratio = rate_1m / rate_5m;
    let arrow = if ratio > 1.1 {
        "↑"
    } else if ratio < 0.9 {
        "↓"
    } else {
        "→"
    };
    format!("{arrow}{ratio:.2}x")
}

/// Prints the units a client request produced and what each one cost (H3).
///
/// The command that answers "why did this request take so long": every unit's latency is
/// split into the time the worker spent computing and the time spent getting the matrices
/// there and back. A unit still in flight or failed has no timings yet, so it shows zero —
/// its row is there to say the unit exists and where it is, not to report a cost.
async fn print_trace(endpoint: &str, request_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let response = client
        .get_trace(GetTraceRequest {
            request_id: request_id.to_string(),
        })
        .await?
        .into_inner();
    println!(
        "REQUISIÇÃO {} UNIDADES {}",
        response.request_id,
        response.units.len()
    );
    if response.units.is_empty() {
        println!("(nenhuma unidade: id desconhecido ou trace perdido por restart)");
        return Ok(());
    }
    println!(
        "{:<10} {:<14} {:<11} {:>12} {:>12} {:>12} {:>12}",
        "TRABALHO", "WORKER", "ESTADO", "COMPUTE_US", "PREDITO_US", "TRANSFER_US", "TOTAL_US"
    );
    let mut compute_total = 0_u64;
    let mut predicted_total = 0_u64;
    let mut transfer_total = 0_u64;
    let mut latency_total = 0_u64;
    for unit in &response.units {
        compute_total += unit.compute_time_us;
        predicted_total += unit.predicted_compute_us;
        transfer_total += unit.transfer_time_us;
        latency_total += unit.total_latency_us;
        println!(
            "{:<10} {:<14} {:<11} {:>12} {:>12} {:>12} {:>12}",
            unit.work_id,
            if unit.node_id.is_empty() {
                "-"
            } else {
                unit.node_id.as_str()
            },
            unit.status,
            unit.compute_time_us,
            unit.predicted_compute_us,
            unit.transfer_time_us,
            unit.total_latency_us,
        );
    }
    println!(
        "unidades={} compute_somado_us={} transfer_somado_us={} latencia_somada_us={}",
        response.units.len(),
        compute_total,
        transfer_total,
        latency_total,
    );
    // H5: previsto vs real. Só faz sentido quando algo foi previsto: um worker sem compute
    // score reporta zero, e zero ali significa "não previsto", não "instantâneo". Uma
    // unidade não concluída também conta zero dos dois lados e sai da conta sozinha.
    if predicted_total > 0 {
        let error_pct =
            (compute_total as f64 - predicted_total as f64) / predicted_total as f64 * 100.0;
        println!(
            "PREVISTO somado_us={} erro_pct={:.1}",
            predicted_total, error_pct
        );
    }
    Ok(())
}

// Cada flag do comando vira um argumento; agrupar numa struct só moveria a lista de lugar.
#[allow(clippy::too_many_arguments)]
async fn run_benchmark(
    endpoint: &str,
    size: u32,
    repetitions: u32,
    local: bool,
    min_threads: u32,
    request_id: &str,
    priority: u32,
    require_attested: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Version and parameters travel with the output so two runs are comparable.
    println!(
        "versao_benchmark={} escopo={} tamanho={size} repeticoes={repetitions} semente=42",
        env!("CARGO_PKG_VERSION"),
        if local { "no-unico" } else { "distribuido" }
    );
    if local {
        return run_local_baseline(size, repetitions);
    }
    let mut client = connect_operator(endpoint.to_string()).await?;
    let response = client
        .enqueue_work(EnqueueWorkRequest {
            rows: size,
            shared: size,
            cols: size,
            seed: 42,
            repetitions,
            min_cpu_threads: min_threads,
            request_id: request_id.to_string(),
            priority,
            require_attested,
        })
        .await?
        .into_inner();
    println!(
        "enfileiradas {} unidades de trabalho de {size}x{size} requisicao={}",
        response.enqueued, response.request_id
    );

    // Poll until the queue drains. The deadline keeps a stuck worker from hanging the CLI.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let report = loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let report = client
            .work_report(WorkReportRequest { limit: 0 })
            .await?
            .into_inner();
        if report.pending == 0 || tokio::time::Instant::now() >= deadline {
            break report;
        }
    };
    print_report(&report);
    Ok(())
}

fn print_report(report: &WorkReportResponse) {
    println!(
        "{:<10} {:<14} {:>12} {:>12} {:>12}",
        "TRABALHO", "WORKER", "COMPUTE_US", "TRANSFER_US", "TOTAL_US"
    );
    let mut compute_total = 0_u64;
    let mut transfer_total = 0_u64;
    for entry in &report.completed {
        compute_total += entry.compute_time_us;
        transfer_total += entry.transfer_time_us;
        println!(
            "{:<10} {:<14} {:>12} {:>12} {:>12}",
            entry.work_id,
            entry.node_id,
            entry.compute_time_us,
            entry.transfer_time_us,
            entry.total_latency_us,
        );
    }
    let count = report.completed.len().max(1) as u64;
    println!(
        "concluidas={} pendentes={} falharam={} media_compute_us={} media_transfer_us={}",
        report.completed.len(),
        report.pending,
        report.failed,
        compute_total / count,
        transfer_total / count,
    );
}

/// Runs the same matrices in-process on this host as the comparison baseline.
///
/// The distributed run's transfer time only means something next to the time the
/// same work takes without dispatch, serialization or network.
fn run_local_baseline(size: u32, repetitions: u32) -> Result<(), Box<dyn std::error::Error>> {
    let a = Matrix::seeded(size, size, 42);
    let b = Matrix::seeded(size, size, 42 ^ 0x9E37_79B9_7F4A_7C15);
    let runs = repetitions.max(1);
    let mut total = Duration::ZERO;
    for _ in 0..runs {
        let started = Instant::now();
        let product = a.multiply(&b).expect("square matrices align");
        total += started.elapsed();
        // Keep the optimiser from deleting the work.
        std::hint::black_box(product.checksum());
    }
    let avg_us = total.as_micros() / u128::from(runs);
    println!("baseline local {size}x{size} repeticoes={runs}");
    println!(
        "{:<12} {:>14} {:>12}",
        "ESCOPO", "MEDIA_COMPUTE_US", "TOTAL_US"
    );
    println!(
        "{:<12} {:>14} {:>12}",
        "no-unico",
        avg_us,
        total.as_micros()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_cluster_protocol::cluster::{CapabilityReport, NodeIdentity, NodeSnapshot};

    /// O mesmo caminho que o `main` percorre, sem executar nada.
    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        let matches = ptbr::localize(Cli::command()).try_get_matches_from(args)?;
        Cli::from_arg_matches(&matches)
    }

    #[test]
    fn os_grupos_novos_aceitam_os_verbos_da_adr() {
        let aceitos: &[&[&str]] = &[
            &["aic", "cluster", "nodes"],
            &["aic", "cluster", "node", "show", "n1"],
            &[
                "aic",
                "cluster",
                "node",
                "approve",
                "n1",
                "--fingerprint",
                "ab",
            ],
            &["aic", "cluster", "node", "approve", "n1", "--trust-listed"],
            &["aic", "cluster", "node", "retire", "n1"],
            &["aic", "cluster", "node", "revoke", "n1"],
            &["aic", "cluster", "node", "remove", "n1"],
            &["aic", "cluster", "install", "list"],
            &[
                "aic", "cluster", "install", "approve", "ab", "--disk", "sda",
            ],
            &["aic", "cluster", "install", "reject", "ab"],
            &["aic", "cluster", "install", "open", "--minutes", "5"],
            &["aic", "cluster", "install", "close"],
            &["aic", "cluster", "reservation", "list"],
            &[
                "aic",
                "cluster",
                "reservation",
                "add",
                "n1",
                "req",
                "--minutes",
                "10",
                "--slots",
                "2",
            ],
            &["aic", "cluster", "reservation", "remove", "n1"],
            &["aic", "cluster", "model", "list"],
            &["aic", "cluster", "request", "trace", "r1"],
            &["aic", "cluster", "topology"],
            &["aic", "cluster", "metrics"],
            &["aic", "cluster", "events"],
            &["aic", "cluster", "benchmark"],
            &["aic", "cluster", "graph"],
            &["aic", "cluster", "rollout", "1.2.0", "--cancel"],
            &["aic", "cluster", "dashboard"],
            &[
                "aic",
                "cluster",
                "invite",
                "--minutes",
                "15",
                "--port",
                "8099",
            ],
            &[
                "aic",
                "controller",
                "install",
                "--with-gateway",
                "--yes",
                "--lang",
                "en",
            ],
            &["aic", "controller", "logs", "-f", "-n", "50"],
            &[
                "aic", "gateway", "install", "--server", "10.0.0.5", "--code", "123",
            ],
            &[
                "aic", "worker", "join", "--server", "10.0.0.5", "--code", "123",
            ],
            &["aic", "worker", "role"],
            &["aic", "worker", "passwd", "--remove"],
            &["aic", "update", "apply", "--version", "1.2.0", "--force"],
            &["aic", "update", "status"],
            &["aic", "version"],
        ];
        for args in aceitos {
            assert!(
                parse(args).is_ok(),
                "deveria aceitar {args:?}: {:?}",
                parse(args).err()
            );
        }
    }

    #[test]
    fn nomes_antigos_sao_recusados_sem_apelido() {
        let recusados: &[&[&str]] = &[
            &["aic", "status"],
            &["aic", "nodes"],
            &["aic", "approve", "n1"],
            &["aic", "installs"],
            &["aic", "reserve", "n1", "req"],
            &["aic", "reservations"],
            &["aic", "release", "n1"],
            &["aic", "enrollment", "open"],
            &["aic", "approve-install", "ab", "--disk", "sda"],
            &["aic", "reject-install", "ab"],
            &["aic", "cluster", "status"],
            &["aic", "cluster", "approve", "n1"],
            &["aic", "cluster", "node", "n1"],
            &["aic", "cluster", "retire", "n1"],
            &["aic", "cluster", "installs"],
            &["aic", "cluster", "enrollment", "open"],
            &["aic", "cluster", "reserve", "n1", "req"],
            &["aic", "cluster", "reservations"],
            &["aic", "cluster", "release", "n1"],
            &[
                "aic",
                "cluster",
                "reservation",
                "add",
                "n1",
                "req",
                "--ttl-ms",
                "5",
            ],
            &["aic", "role"],
            &["aic", "change"],
            &["aic", "passwd"],
            &["aic", "logs"],
            &[
                "aic",
                "cluster",
                "nodes",
                "--controller",
                "http://x:1",
                "extra",
            ],
            &["aic", "update", "apply", "--lang", "fr"],
            &["aic", "worker", "change", "--server", "10.0.0.5"],
            &["aic", "worker", "change", "--token", "t"],
            &["aic", "worker", "change", "--token-file", "f"],
            &[
                "aic",
                "worker",
                "change",
                "--controller",
                "u",
                "--token",
                "t",
                "--token-file",
                "f",
            ],
            &[
                "aic",
                "worker",
                "change",
                "--server",
                "s",
                "--code",
                "c",
                "--token-file",
                "f",
            ],
        ];
        for args in recusados {
            assert!(parse(args).is_err(), "deveria recusar {args:?}");
        }
        let antigo = parse(&["aic", "cluster", "approve", "n1"]).unwrap_err();
        assert!(
            ptbr::message(&antigo).contains("subcomando desconhecido: approve"),
            "{}",
            ptbr::message(&antigo)
        );
    }

    #[test]
    fn worker_change_exige_uma_das_duas_formas() {
        assert!(
            parse(&["aic", "worker", "change"]).is_err(),
            "sem argumento"
        );
        assert!(
            parse(&["aic", "worker", "change", "--controller", "u"]).is_err(),
            "controller sem token"
        );
        assert!(
            parse(&[
                "aic",
                "worker",
                "change",
                "--server",
                "s",
                "--code",
                "c",
                "--controller",
                "u"
            ])
            .is_err(),
            "servidor+código e controller não são uma forma só"
        );
    }

    #[test]
    fn erros_de_uso_dos_exemplos_saem_em_portugues() {
        let casos: [(&[&str], &str); 5] = [
            (
                &[
                    "aic", "worker", "join", "--server", "x", "--code", "y", "--lang", "xx",
                ],
                "valor inválido \"xx\" para --lang; use: pt, en",
            ),
            (
                &["aic", "cluster", "install", "open", "--minutes", "abc"],
                "valor inválido \"abc\" para --minutes",
            ),
            (
                &["aic", "cluster", "install", "open", "--minutes", "999"],
                "valor inválido \"999\" para --minutes",
            ),
            (
                &[
                    "aic",
                    "worker",
                    "change",
                    "--controller",
                    "u",
                    "--token",
                    "t",
                    "--server",
                    "s",
                ],
                "não podem ser usados juntos",
            ),
            (
                &["aic", "update", "apply", "--version"],
                "--version pede um valor",
            ),
        ];
        for (args, esperado) in casos {
            let text = ptbr::message(&parse(args).unwrap_err());
            assert!(text.contains(esperado), "{args:?}: {text}");
            for ingles in [
                "invalid",
                "cannot",
                "required",
                "error:",
                "Usage",
                "possible values",
                "For more",
                "not allowed",
                "conflict",
            ] {
                assert!(!text.contains(ingles), "vazou {ingles:?} em {text}");
            }
        }
    }

    #[test]
    fn worker_change_aceita_a_forma_do_controller_com_token() {
        assert!(
            parse(&[
                "aic",
                "worker",
                "change",
                "--controller",
                "u",
                "--token",
                "t"
            ])
            .is_ok()
        );
        assert!(parse(&["aic", "worker", "change", "--server", "s", "--code", "c"]).is_ok());
        assert!(
            parse(&[
                "aic",
                "worker",
                "change",
                "--controller",
                "u",
                "--token-file",
                "f"
            ])
            .is_ok()
        );
    }

    fn node(id: &str, shard_cache_bytes: u64, trusted: bool) -> NodeSnapshot {
        NodeSnapshot {
            identity: Some(NodeIdentity {
                node_id: id.into(),
                architecture: "arm".into(),
                runtime_version: "11.0".into(),
                ..Default::default()
            }),
            capability: Some(CapabilityReport {
                cpu_threads: 4,
                shard_cache_bytes,
                ..Default::default()
            }),
            state: "READY".into(),
            trusted,
            ..Default::default()
        }
    }

    #[test]
    fn texto_do_controller_nao_leva_controle_nem_invisiveis_para_a_tela() {
        assert_eq!(clean("ok-1"), "ok-1");
        assert_eq!(clean("a\u{1b}[2Jb"), "a?[2Jb");
        assert_eq!(clean("x\u{202E}y\u{200B}z\u{7f}"), "x?y?z?");
        let id = sanitize_node(node("n\u{1b}]52;c;x\u{7}", 0, true))
            .identity
            .unwrap()
            .node_id;
        assert!(!id.chars().any(char::is_control), "{id:?}");
    }

    #[test]
    fn o_painel_de_inferencia_mostra_o_cache_e_a_idade_de_quem_sumiu() {
        use ai_cluster_protocol::cluster::{GatewayInfo, GatewayStats};
        let gateway = |alive, stats| GatewayInfo {
            gateway_id: "gw1".into(),
            url: "http://gw:8080".into(),
            alive,
            last_seen_unix_ms: 1_000,
            stats,
            ..Default::default()
        };
        assert!(gateway_lines(&[], 5_000).is_empty());
        let stats = GatewayStats {
            requests: 10,
            cache_hits: 4,
            cache_tokens_avoided: 128,
            generated_tokens_per_second_1m: 2.0,
            generated_tokens_per_second_5m: 1.0,
            ..Default::default()
        };
        let live = gateway_lines(&[gateway(true, Some(stats))], 5_000).join("\n");
        assert!(live.contains("VIVO") && live.contains("tokens_evitados=128"));
        assert!(live.contains("↑2.00x"), "tendência de tokens: {live}");
        let gone = gateway_lines(&[gateway(false, Some(stats))], 31_000).join("\n");
        assert!(gone.contains("SEM SINAL há 30s"), "{gone}");
        let old = gateway_lines(&[gateway(true, None)], 5_000).join("\n");
        assert!(old.contains("sem dados"), "{old}");
    }

    #[test]
    fn o_painel_bonito_traz_as_mesmas_secoes_e_marca_o_que_precisa_de_atencao() {
        use ai_cluster_protocol::cluster::{MetricsResponse, Reservation};
        let metrics = MetricsResponse {
            nodes_total: 2,
            nodes_available: 1,
            units_completed: 3,
            units_failed: 1,
            units_per_second_1m: 2.0,
            units_per_second_5m: 1.0,
            avg_network_score: 1.5,
            ..Default::default()
        };
        let nodes = [node("orangepi-ab", 0, true), node("pendente-1", 0, false)];
        let reservation = Reservation {
            node_id: "orangepi-ab".into(),
            request_id: "req-1".into(),
            slots: 2,
            ..Default::default()
        };
        let lines = pretty_dashboard("http://c:1", &nodes, &metrics, &[reservation], &[], 0, None)
            .join(
                "
",
            );
        for expected in [
            "AI.Cluster painel",
            "1/2 nos disponiveis",
            "orangepi-ab",
            "pendente de aprovacao",
            "concluidas 3",
            "falhas 1",
            "↑2.00x",
            "rede 1.50",
            "req-1",
            "2 slot(s)",
        ] {
            assert!(
                lines.contains(expected),
                "falta {expected:?} em:
{lines}"
            );
        }
        // sem gateway, a seção de inferência não aparece
        assert!(!lines.contains("Inferencia"));
    }

    #[test]
    fn a_tendencia_e_a_razao_entre_as_taxas() {
        assert_eq!(trend(0.0, 1.0), "-");
        assert_eq!(trend(1.0, 0.0), "-");
        assert_eq!(trend(2.0, 1.0), "↑2.00x");
        assert_eq!(trend(0.5, 1.0), "↓0.50x");
        assert_eq!(trend(1.05, 1.0), "→1.05x");
    }

    #[test]
    fn o_papel_vem_do_id_de_recuperacao_e_do_cache_de_fatias() {
        assert!(node_role("pi-rec", 1 << 30).starts_with("Recuperação"));
        assert_eq!(node_role("pi", 1 << 30), "Computação + fatias de modelo");
        assert_eq!(node_role("pi", 0), "Computação");
    }

    #[test]
    fn a_arvore_agrupa_por_papel_e_marca_o_pendente() {
        let nodes = [
            node("a", 2 << 30, true),
            node("b", 0, false),
            node("c", 0, true),
        ];
        let tree = topology_lines("http://x:1", &nodes, &[]);
        assert_eq!(tree[0], "controller http://x:1  (3 workers)");
        assert_eq!(tree[1], "├─ Computação + fatias de modelo (1)");
        assert!(tree[2].starts_with("│  └─ a  READY arm 4t"));
        assert!(tree[2].contains("cache=2048MB"));
        assert_eq!(tree[3], "└─ Computação (2)");
        assert!(tree[4].starts_with("   ├─ b ") && tree[4].contains("PENDENTE"));
        assert!(tree[5].starts_with("   └─ c "));
        assert_eq!(
            topology_lines("u", &[], &[]),
            vec!["controller u  (0 workers)"]
        );
    }

    #[test]
    fn o_gateway_aparece_como_ramo_com_modelos_e_fatias() {
        use ai_cluster_protocol::cluster::{GatewayInfo, GatewayModel};
        let gateway = GatewayInfo {
            gateway_id: "gw1".into(),
            url: "http://gw:8080".into(),
            alive: true,
            models: vec![
                GatewayModel {
                    model_id: "gpt2".into(),
                    shard_nodes: vec![],
                },
                GatewayModel {
                    model_id: "smollm2".into(),
                    shard_nodes: vec!["a".into(), "b".into()],
                },
            ],
            ..Default::default()
        };
        let tree = topology_lines("u", &[node("a", 0, true)], &[gateway]);
        assert_eq!(tree[3], "└─ Gateway de inferência (1)");
        assert!(tree[4].contains("gw1  http://gw:8080  VIVO"));
        assert!(tree[4].contains("gpt2 (inteiro no gateway); smollm2 → fatias em a,b"));
    }

    #[test]
    fn a_api_mostra_o_endereco_e_o_modelo_ativo_ou_como_subir_um() {
        use ai_cluster_protocol::cluster::{
            DesiredModel, GatewayInfo, ModelStatus, ModelStatusResponse,
        };
        let gateway = GatewayInfo {
            gateway_id: "gw".into(),
            url: "http://10.0.0.1:8080".into(),
            alive: true,
            ..Default::default()
        };
        // Sem modelo pedido: diz como subir um.
        let none = ModelStatusResponse::default();
        let lines = pretty_api(std::slice::from_ref(&gateway), Some(&none)).join(
            "
",
        );
        assert!(
            lines.contains("http://10.0.0.1:8080") && lines.contains("NO AR"),
            "{lines}"
        );
        assert!(
            lines.contains("NENHUM") && lines.contains("aic cluster model start"),
            "{lines}"
        );
        assert!(lines.contains("/v1/chat/completions"), "{lines}");
        // Modelo pronto: o id e a velocidade.
        let ready = ModelStatusResponse {
            desired: Some(DesiredModel {
                model_id: "qwen".into(),
                ..Default::default()
            }),
            running: Some(ModelStatus {
                state: ModelState::ModelReady as i32,
                tokens_per_second: 12.34,
                ..Default::default()
            }),
            ..Default::default()
        };
        let lines = api_lines(std::slice::from_ref(&gateway), Some(&ready));
        assert_eq!(lines[0], "API url=http://10.0.0.1:8080 estado=no_ar");
        assert_eq!(lines[1], "MODELO_ATIVO estado=PRONTO qwen  12.3 tok/s");
        // Sem gateway e sem endereço informado.
        assert!(pretty_api(&[], None).join("").contains("nenhum gateway"));
        let silent = GatewayInfo {
            alive: false,
            ..Default::default()
        };
        assert!(
            pretty_api(&[silent], None)
                .join("")
                .contains("AI_CLUSTER_GATEWAY_URL")
        );
    }
}
