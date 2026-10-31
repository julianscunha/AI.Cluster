//! Aparelhos na etapa de espera da instalação (ADR-023): a lista e a aprovação. Sem argumentos,
//! `approve` escolhe o aparelho (se só houver um) e o disco interno, e pede confirmação antes de apagar.

use std::io::{IsTerminal, Write};

use ai_cluster_protocol::cluster::{
    ApproveInstallRequest, InstallDisk, ListPendingInstallsRequest, PendingInstall,
};
use ai_cluster_protocol::metadata::connect_operator;

use crate::idioma::t;

type Erro = Box<dyn std::error::Error>;

fn tamanho(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

fn tipo(disco: &InstallDisk) -> &'static str {
    if disco.removable {
        t("removível", "removable")
    } else {
        t("interno", "internal")
    }
}

/// `sda Samsung SSD 500.0 GB`: o que o operador lê antes de confirmar o que vai ser apagado.
fn descreve(disco: &InstallDisk) -> String {
    format!(
        "{} {} {}",
        disco.name,
        disco.model,
        tamanho(disco.size_bytes)
    )
}

/// O disco que a aprovação sugere: o único interno (não removível). Com dois internos, ou só
/// removíveis, quem escolhe é o operador.
pub fn disco_sugerido(discos: &[InstallDisk]) -> Option<&InstallDisk> {
    let mut internos = discos.iter().filter(|d| !d.removable);
    let primeiro = internos.next()?;
    internos.next().is_none().then_some(primeiro)
}

/// Os aparelhos que ainda esperam decisão (sem disco aprovado).
fn pendentes(lista: &[PendingInstall]) -> Vec<&PendingInstall> {
    lista
        .iter()
        .filter(|p| p.approved_disk.is_empty())
        .collect()
}

fn le_linha() -> Result<String, Erro> {
    let mut linha = String::new();
    std::io::stdin().read_line(&mut linha)?;
    Ok(linha.trim().to_string())
}

fn pergunta(texto: &str) -> Result<String, Erro> {
    print!("{texto}");
    std::io::stdout().flush()?;
    le_linha()
}

/// Pede um número de 1 a `n` e devolve a posição (0 a n-1).
fn escolhe(n: usize, quem: &str) -> Result<usize, Erro> {
    let resposta = pergunta(&format!("{quem} [1-{n}]: "))?;
    resposta
        .parse::<usize>()
        .ok()
        .filter(|k| (1..=n).contains(k))
        .map(|k| k - 1)
        .ok_or_else(|| t("escolha inválida", "invalid choice").into())
}

/// `aic cluster install list`. No terminal, uma tabela; com a saída num arquivo ou num pipe (o instalador
/// lê), as linhas `INSTALAÇÃO fingerprint=…` de sempre.
pub async fn lista(endpoint: &str) -> Result<(), Erro> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let reply = client
        .list_pending_installs(ListPendingInstallsRequest {})
        .await?
        .into_inner();
    if !std::io::stdout().is_terminal() {
        maquina(&reply.pending, reply.now_unix_ms);
    } else if reply.pending.is_empty() {
        println!("{}", t("nenhum aparelho aguardando", "no device waiting"));
    } else {
        tabela(&reply.pending, reply.now_unix_ms);
    }
    Ok(())
}

fn maquina(lista: &[PendingInstall], agora: u64) {
    if lista.is_empty() {
        println!("nenhum aparelho aguardando");
    }
    for item in lista {
        let disks = item
            .disks
            .iter()
            .map(|d| format!("{}={}GB", d.name, d.size_bytes / 1_000_000_000))
            .collect::<Vec<_>>()
            .join(",");
        let waiting = agora.saturating_sub(item.first_seen_unix_ms) / 1000;
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
}

fn tabela(lista: &[PendingInstall], agora: u64) {
    println!(
        "{} ({})\n",
        t(
            "APARELHOS AGUARDANDO INSTALAÇÃO",
            "DEVICES WAITING TO INSTALL"
        ),
        lista.len()
    );
    for (i, p) in lista.iter().enumerate() {
        let espera = agora.saturating_sub(p.first_seen_unix_ms) / 1000;
        let estado = if p.approved_disk.is_empty() {
            t("aguardando aprovação", "waiting for approval").to_string()
        } else {
            format!(
                "{} {}",
                t("aprovado, apaga", "approved, erases"),
                p.approved_disk
            )
        };
        println!(
            "{}. {} {} · {} · {} {espera} s",
            i + 1,
            t("impressão", "fingerprint"),
            p.fingerprint,
            estado,
            t("esperando há", "waiting for")
        );
        println!(
            "   MAC      {} · {} {}",
            p.mac,
            t("endereço", "address"),
            p.host
        );
        println!(
            "   CPU      {} · {} {} · {}",
            p.cpu_model,
            p.cpu_cores,
            t("núcleos", "cores"),
            p.arch
        );
        println!(
            "   RAM      {:.1} GB · firmware {}",
            p.ram_bytes as f64 / 1e9,
            p.firmware
        );
        tabela_discos(&p.disks);
        println!();
    }
}

fn tabela_discos(discos: &[InstallDisk]) {
    let cabeca = [
        t("NOME", "NAME"),
        t("TAMANHO", "SIZE"),
        t("MODELO", "MODEL"),
        t("TIPO", "TYPE"),
    ];
    if discos.is_empty() {
        println!("   DISCOS   {}", t("nenhum informado", "none reported"));
        return;
    }
    let tamanhos: Vec<String> = discos.iter().map(|d| tamanho(d.size_bytes)).collect();
    let nome = discos
        .iter()
        .map(|d| d.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(cabeca[0].len());
    let tam = tamanhos
        .iter()
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(0)
        .max(cabeca[1].len());
    let modelo = discos
        .iter()
        .map(|d| d.model.chars().count())
        .max()
        .unwrap_or(0)
        .max(cabeca[2].len());
    println!(
        "   DISCOS   {:<nome$}  {:<tam$}  {:<modelo$}  {}",
        cabeca[0],
        cabeca[1],
        cabeca[2],
        cabeca[3],
        nome = nome,
        tam = tam,
        modelo = modelo
    );
    for (disco, size) in discos.iter().zip(&tamanhos) {
        println!(
            "            {:<nome$}  {:<tam$}  {:<modelo$}  {}",
            disco.name,
            size,
            disco.model,
            tipo(disco),
            nome = nome,
            tam = tam,
            modelo = modelo
        );
    }
}

/// `aic cluster install approve [impressão] [--disk D] [--yes] [--force-removable]`. Sem a impressão,
/// aprova o único pendente ou pergunta qual; sem `--disk`, usa o único disco interno ou pergunta qual.
/// Antes de apagar mostra o disco e pede confirmação, salvo `--yes` (automação).
pub async fn aprova(
    endpoint: &str,
    fingerprint: Option<String>,
    disk: Option<String>,
    yes: bool,
    force_removable: bool,
) -> Result<(), Erro> {
    let mut client = connect_operator(endpoint.to_string()).await?;
    let lista = client
        .list_pending_installs(ListPendingInstallsRequest {})
        .await?
        .into_inner()
        .pending;
    let interativo = std::io::stdin().is_terminal();
    let aparelho = match fingerprint {
        Some(fp) => match lista
            .iter()
            .find(|p| p.fingerprint.eq_ignore_ascii_case(fp.trim()))
        {
            Some(p) => p,
            None => {
                println!("INSTALAÇÃO não encontrada fingerprint={fp}");
                return Ok(());
            }
        },
        None => {
            let esperando = pendentes(&lista);
            match esperando.len() {
                0 => {
                    println!(
                        "{}",
                        t(
                            "nenhum aparelho aguardando aprovação",
                            "no device waiting for approval"
                        )
                    );
                    return Ok(());
                }
                1 => esperando[0],
                n => {
                    if !interativo {
                        return Err(t(
                            "há mais de um aparelho aguardando; diga qual: aic cluster install approve <impressão>",
                            "more than one device is waiting; say which: aic cluster install approve <fingerprint>",
                        )
                        .into());
                    }
                    for (i, p) in esperando.iter().enumerate() {
                        let discos: Vec<String> = p.disks.iter().map(descreve).collect();
                        println!(
                            "{}) {} · {} · {}",
                            i + 1,
                            p.fingerprint,
                            p.cpu_model,
                            discos.join(", ")
                        );
                    }
                    esperando[escolhe(n, t("qual aparelho", "which device"))?]
                }
            }
        }
    };
    if aparelho.disks.is_empty() {
        return Err(t(
            "o aparelho não informou nenhum disco",
            "the device reported no disk",
        )
        .into());
    }
    let disco = match disk {
        Some(nome) => aparelho
            .disks
            .iter()
            .find(|d| d.name == nome)
            .ok_or_else(|| {
                format!(
                    "{} {nome}",
                    t(
                        "o aparelho não informou o disco",
                        "the device did not report disk"
                    )
                )
            })?,
        None => match disco_sugerido(&aparelho.disks) {
            Some(sugerido) => sugerido,
            None => {
                if !interativo {
                    return Err(t(
                        "o aparelho tem vários discos ou nenhum interno; diga qual com --disk",
                        "the device has several disks or no internal one; say which with --disk",
                    )
                    .into());
                }
                for (i, d) in aparelho.disks.iter().enumerate() {
                    println!("{}) {} ({})", i + 1, descreve(d), tipo(d));
                }
                &aparelho.disks[escolhe(aparelho.disks.len(), t("qual disco", "which disk"))?]
            }
        },
    };
    if !yes {
        if !interativo {
            return Err(t(
                "sem terminal para confirmar; acrescente --yes para aprovar sem perguntar",
                "no terminal to confirm; add --yes to approve without asking",
            )
            .into());
        }
        let resposta = pergunta(&format!(
            "{} {} ({}). {}",
            t("vai apagar", "will erase"),
            descreve(disco),
            tipo(disco),
            t("Confirma? [s/N] ", "Confirm? [y/N] ")
        ))?;
        if !matches!(resposta.to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
            println!(
                "{}",
                t(
                    "INSTALAÇÃO cancelada; nada foi apagado",
                    "INSTALL cancelled; nothing was erased"
                )
            );
            return Ok(());
        }
    }
    let aprovado = client
        .approve_install(ApproveInstallRequest {
            fingerprint: aparelho.fingerprint.clone(),
            disk: disco.name.clone(),
            force_removable,
        })
        .await?
        .into_inner()
        .approved;
    if aprovado {
        println!(
            "INSTALAÇÃO aprovada fingerprint={} apaga={}",
            aparelho.fingerprint, disco.name
        );
    } else {
        println!(
            "INSTALAÇÃO não encontrada fingerprint={}",
            aparelho.fingerprint
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disco(nome: &str, removivel: bool) -> InstallDisk {
        InstallDisk {
            name: nome.into(),
            size_bytes: 500_000_000_000,
            model: "SSD".into(),
            removable: removivel,
        }
    }

    #[test]
    fn sugere_o_unico_disco_interno() {
        let um_interno = [disco("mmcblk0", false), disco("sdb", true)];
        assert_eq!(
            disco_sugerido(&um_interno).map(|d| d.name.as_str()),
            Some("mmcblk0")
        );
    }

    #[test]
    fn nao_chuta_quando_nao_ha_um_unico_interno() {
        let dois = [disco("sda", false), disco("sdc", false), disco("sdb", true)];
        assert!(
            disco_sugerido(&dois).is_none(),
            "dois internos: o operador escolhe"
        );
        assert!(
            disco_sugerido(&[disco("sdb", true)]).is_none(),
            "só removível não é sugerido"
        );
        assert!(disco_sugerido(&[]).is_none());
    }

    #[test]
    fn so_aparelho_sem_disco_aprovado_espera_decisao() {
        let mut aprovado = PendingInstall {
            fingerprint: "a".into(),
            approved_disk: "sda".into(),
            ..Default::default()
        };
        let aguardando = PendingInstall {
            fingerprint: "b".into(),
            ..Default::default()
        };
        let lista = [aprovado.clone(), aguardando.clone()];
        let esperando = pendentes(&lista);
        assert_eq!(esperando.len(), 1);
        assert_eq!(esperando[0].fingerprint, "b");
        aprovado.approved_disk.clear();
        assert_eq!(pendentes(&[aprovado, aguardando]).len(), 2);
    }
}
