//! Comandos que instalam, atualizam ou administram o computador. O `aic` só declara os argumentos
//! (a ajuda e os erros de uso saem em português pelo clap) e repassa a linha ao instalador de shell:
//! `<libexec>/setup <substantivo> <verbo> [argumentos…]`. Quem faz o trabalho é o `setup` (ADR-024).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use clap::{Args, Subcommand};

/// Pastas procuradas quando `AIC_LIBEXEC` não aponta para um instalador.
const PADROES: [&str; 2] = ["/opt/ai-cluster/libexec", "/usr/libexec/aic"];

/// Registros de um serviço (`logs`).
#[derive(Args, Debug)]
pub struct Registros {
    /// Continua mostrando os registros novos à medida que chegam.
    #[arg(short = 'f', long)]
    follow: bool,
    /// Só as últimas N linhas.
    #[arg(short = 'n', long, value_name = "N")]
    lines: Option<u32>,
}

impl Registros {
    fn args(&self) -> Vec<String> {
        let mut out = vec!["logs".to_string()];
        marca(&mut out, "-f", self.follow);
        valor(&mut out, "-n", self.lines);
        out
    }
}

/// `aic cluster invite`: abre o convite para computadores novos.
#[derive(Args, Debug)]
pub struct Convite {
    /// Minutos que o convite fica aberto.
    #[arg(long)]
    minutes: Option<u32>,
    /// Porta em que o convite atende.
    #[arg(long)]
    port: Option<u16>,
}

impl Convite {
    pub fn args(&self) -> Vec<String> {
        let mut out = vec!["invite".to_string()];
        valor(&mut out, "--minutes", self.minutes);
        valor(&mut out, "--port", self.port);
        out
    }
}

#[derive(Subcommand, Debug)]
pub enum Controller {
    /// Instala o controller neste Linux.
    Install {
        /// Instala também o gateway, no mesmo computador.
        #[arg(long)]
        with_gateway: bool,
        /// IP deste computador que os workers usam para chegar ao controller.
        #[arg(long)]
        ip: Option<String>,
        /// Pasta com os modelos.
        #[arg(long)]
        models_dir: Option<String>,
        /// Não pergunta nada: aceita os padrões.
        #[arg(long)]
        yes: bool,
        /// Idioma do instalador: pt (padrão) ou en.
        #[arg(long, value_parser = ["pt", "en"], hide_possible_values = true)]
        lang: Option<String>,
    },
    /// Estado do controller deste Linux.
    Status,
    /// Registros do controller.
    Logs(Registros),
}

impl Controller {
    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Install {
                with_gateway,
                ip,
                models_dir,
                yes,
                lang,
            } => {
                let mut out = vec!["install".to_string()];
                marca(&mut out, "--with-gateway", *with_gateway);
                valor(&mut out, "--ip", ip.as_ref());
                valor(&mut out, "--models-dir", models_dir.as_ref());
                marca(&mut out, "--yes", *yes);
                valor(&mut out, "--lang", lang.as_ref());
                out
            }
            Self::Status => vec!["status".to_string()],
            Self::Logs(registros) => registros.args(),
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Gateway {
    /// Instala o gateway de inferência neste Linux, apontado para um controller.
    Install {
        /// Endereço do convite do controller (`IP[:porta]`).
        #[arg(long)]
        server: String,
        /// Código do convite.
        #[arg(long)]
        code: String,
        /// Pasta com os modelos.
        #[arg(long)]
        models_dir: Option<String>,
        /// Não pergunta nada: aceita os padrões.
        #[arg(long)]
        yes: bool,
        /// Idioma do instalador: pt (padrão) ou en.
        #[arg(long, value_parser = ["pt", "en"], hide_possible_values = true)]
        lang: Option<String>,
    },
    /// Estado do gateway deste Linux.
    Status,
    /// Registros do gateway.
    Logs(Registros),
}

impl Gateway {
    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Install {
                server,
                code,
                models_dir,
                yes,
                lang,
            } => {
                let mut out = vec!["install".to_string()];
                valor(&mut out, "--server", Some(server));
                valor(&mut out, "--code", Some(code));
                valor(&mut out, "--models-dir", models_dir.as_ref());
                marca(&mut out, "--yes", *yes);
                valor(&mut out, "--lang", lang.as_ref());
                out
            }
            Self::Status => vec!["status".to_string()],
            Self::Logs(registros) => registros.args(),
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Worker {
    /// Entra no cluster com um convite: instala o worker neste Linux ou no AI Cluster OS.
    Join {
        /// Endereço do convite do controller (`IP[:porta]`).
        #[arg(long)]
        server: String,
        /// Código do convite.
        #[arg(long)]
        code: String,
        /// Nome do worker no cluster (padrão: o hostname deste computador).
        #[arg(long)]
        name: Option<String>,
        /// Não pergunta nada: aceita os padrões.
        #[arg(long)]
        yes: bool,
        /// Idioma do instalador: pt (padrão) ou en.
        #[arg(long, value_parser = ["pt", "en"], hide_possible_values = true)]
        lang: Option<String>,
    },
    /// Estado do worker deste computador.
    Status,
    /// Registros do worker.
    Logs(Registros),
    /// O papel deste computador no cluster, em uma linha (só no AI Cluster OS). O detalhe é `worker status`.
    Role,
    /// Troca o convite ou só o controller do worker (só no AI Cluster OS).
    #[command(
        group(clap::ArgGroup::new("forma").required(true).args(["server", "controller"])),
        group(clap::ArgGroup::new("token_join").args(["token", "token_file"]))
    )]
    Change {
        /// Endereço do convite do novo controller (com `--code`).
        #[arg(long, requires = "code", conflicts_with_all = ["token", "token_file", "controller"])]
        server: Option<String>,
        /// Código do convite (com `--server`).
        #[arg(long, requires = "server", conflicts_with_all = ["token", "token_file"])]
        code: Option<String>,
        /// Endereço gRPC do controller (ex.: `http://IP:50051`), com `--token` ou `--token-file`.
        #[arg(long, requires = "token_join")]
        controller: Option<String>,
        /// Token de join, para trocar só o controller (com `--controller`).
        #[arg(long, requires = "controller", conflicts_with_all = ["server", "code"])]
        token: Option<String>,
        /// Arquivo com o token de join (alternativa a `--token`, que aparece no `ps`).
        #[arg(long, value_name = "ARQUIVO", requires = "controller", conflicts_with_all = ["server", "code"])]
        token_file: Option<String>,
    },
    /// Senha do console do AI Cluster OS (só no AI Cluster OS).
    Passwd {
        /// Tira a senha, deixando o console sem login.
        #[arg(long)]
        remove: bool,
    },
}

impl Worker {
    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Join {
                server,
                code,
                name,
                yes,
                lang,
            } => {
                let mut out = vec!["join".to_string()];
                valor(&mut out, "--server", Some(server));
                valor(&mut out, "--code", Some(code));
                valor(&mut out, "--name", name.as_ref());
                marca(&mut out, "--yes", *yes);
                valor(&mut out, "--lang", lang.as_ref());
                out
            }
            Self::Status => vec!["status".to_string()],
            Self::Logs(registros) => registros.args(),
            Self::Role => vec!["role".to_string()],
            Self::Change {
                server,
                code,
                controller,
                token,
                token_file,
            } => {
                let mut out = vec!["change".to_string()];
                valor(&mut out, "--server", server.as_ref());
                valor(&mut out, "--code", code.as_ref());
                valor(&mut out, "--controller", controller.as_ref());
                valor(&mut out, "--token", token.as_ref());
                valor(&mut out, "--token-file", token_file.as_ref());
                out
            }
            Self::Passwd { remove } => {
                let mut out = vec!["passwd".to_string()];
                marca(&mut out, "--remove", *remove);
                out
            }
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Update {
    /// Versão instalada × disponível, ou o resultado da última atualização assinada.
    Status,
    /// Atualiza este Linux pelo instalador. No AI Cluster OS a atualização assinada é empurrada pelo
    /// controller (`aic cluster rollout`).
    Apply {
        /// Versão específica (padrão: a mais nova da origem).
        #[arg(long)]
        version: Option<String>,
        /// Instala mesmo se a versão já for a atual.
        #[arg(long)]
        force: bool,
        /// Não pergunta nada: aceita os padrões.
        #[arg(long)]
        yes: bool,
        /// Idioma do instalador: pt (padrão) ou en.
        #[arg(long, value_parser = ["pt", "en"], hide_possible_values = true)]
        lang: Option<String>,
    },
}

impl Update {
    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Status => vec!["status".to_string()],
            Self::Apply {
                version,
                force,
                yes,
                lang,
            } => {
                let mut out = vec!["apply".to_string()];
                valor(&mut out, "--version", version.as_ref());
                marca(&mut out, "--force", *force);
                marca(&mut out, "--yes", *yes);
                valor(&mut out, "--lang", lang.as_ref());
                out
            }
        }
    }
}

fn marca(out: &mut Vec<String>, flag: &str, on: bool) {
    if on {
        out.push(flag.to_string());
    }
}

fn valor(out: &mut Vec<String>, flag: &str, value: Option<impl ToString>) {
    if let Some(value) = value {
        out.push(flag.to_string());
        out.push(value.to_string());
    }
}

/// Pasta do instalador: `AIC_LIBEXEC`, depois as padrões; a primeira que contém `setup`. Como root a
/// variável é ignorada: quem roda `sudo -E aic` não escolhe o script que roda como root.
fn procura(env: Option<OsString>, padroes: &[PathBuf], raiz: bool) -> Option<PathBuf> {
    let env = if raiz { None } else { env };
    env.filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .into_iter()
        .chain(padroes.iter().cloned())
        .find(|dir| dir.join("setup").is_file())
}

/// Uid efetivo é zero? No Linux, o terceiro campo de `Uid:` em `/proc/self/status` (real, efetivo…).
fn raiz() -> bool {
    std::fs::read_to_string("/proc/self/status").is_ok_and(|status| {
        status
            .lines()
            .any(|line| line.starts_with("Uid:") && line.split_whitespace().nth(2) == Some("0"))
    })
}

/// Dono root e sem escrita para grupo ou outros: quem pode trocar o arquivo teria root.
fn dono_seguro(uid: u32, mode: u32) -> bool {
    uid == 0 && mode & 0o022 == 0
}

/// O instalador padrão (e a pasta que o contém) passa em `dono_seguro`.
#[cfg(unix)]
fn confiavel(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    [dir.to_path_buf(), dir.join("setup")]
        .iter()
        .all(|path| std::fs::metadata(path).is_ok_and(|meta| dono_seguro(meta.uid(), meta.mode())))
}

#[cfg(not(unix))]
fn confiavel(_dir: &Path) -> bool {
    true
}

/// Roda `<libexec>/setup <substantivo> <argumentos…>` e encerra com o código dele.
/// Em unix o processo é trocado pelo `setup` (exec); nos demais, o `setup` roda como filho.
pub fn encaminha(substantivo: &str, args: Vec<String>) -> ! {
    let padroes = PADROES.map(PathBuf::from);
    let raiz = raiz();
    let Some(dir) = procura(std::env::var_os("AIC_LIBEXEC"), &padroes, raiz) else {
        eprintln!("erro: este computador não tem o instalador do AI Cluster; use o install.sh");
        exit(1);
    };
    if raiz && !confiavel(&dir) {
        eprintln!(
            "erro: o instalador em {} não é de root ou é gravável por grupo ou outros; recuso rodá-lo como root",
            dir.display()
        );
        exit(1);
    }
    let setup = dir.join("setup");
    let mut cmd = Command::new(&setup);
    cmd.arg(substantivo).args(&args);
    #[cfg(unix)]
    let falha = {
        use std::os::unix::process::CommandExt;
        cmd.exec()
    };
    #[cfg(not(unix))]
    let falha = match cmd.status() {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(erro) => erro,
    };
    eprintln!("erro: não foi possível rodar {}: {falha}", setup.display());
    exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aic-libexec-{}-{nome}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("setup"), "#!/bin/sh\n").unwrap();
        dir
    }

    #[test]
    fn como_root_a_variavel_e_ignorada_e_fora_de_root_vale() {
        let env = pasta("env");
        let padrao = pasta("padrao");
        let via_env = Some(env.clone().into_os_string());
        assert_eq!(
            procura(via_env.clone(), std::slice::from_ref(&padrao), true),
            Some(padrao.clone())
        );
        assert_eq!(
            procura(via_env, std::slice::from_ref(&padrao), false),
            Some(env)
        );
        // Como root, a variável não leva a um setup que não está nos padrões.
        let sem_padrao = [PathBuf::from("/nao/existe/aic")];
        let so_env = Some(pasta("so-env").into_os_string());
        assert_eq!(procura(so_env, &sem_padrao, true), None);
    }

    #[test]
    fn so_dono_root_e_sem_escrita_alheia_e_confiavel() {
        assert!(dono_seguro(0, 0o755));
        assert!(!dono_seguro(1000, 0o755), "dono comum");
        assert!(!dono_seguro(0, 0o775), "gravável pelo grupo");
        assert!(!dono_seguro(0, 0o757), "gravável por outros");
    }

    #[cfg(unix)]
    #[test]
    fn instalador_gravavel_por_outros_e_recusado_como_root() {
        use std::os::unix::fs::PermissionsExt;
        let dir = pasta("gravavel");
        std::fs::set_permissions(dir.join("setup"), std::fs::Permissions::from_mode(0o777))
            .unwrap();
        assert!(!confiavel(&dir));
    }
}
