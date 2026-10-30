//! Comandos que instalam, atualizam ou administram o computador. O `aic` só declara os argumentos
//! (a ajuda e os erros de uso saem em português pelo clap) e repassa a linha ao instalador de shell:
//! `<libexec>/setup <substantivo> <verbo> [argumentos…]`. Quem faz o trabalho é o `setup` (ADR-024).

use std::ffi::OsString;
use std::path::PathBuf;
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
        /// IP deste computador que os nós usam para chegar ao controller.
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
    /// Papel deste computador no cluster (só no AI Cluster OS).
    Role {
        /// Só a palavra do papel, sem a explicação.
        #[arg(long)]
        short: bool,
    },
    /// Troca o convite ou só o controller do worker (só no AI Cluster OS).
    Change {
        /// Endereço do convite do novo controller (com `--code`).
        #[arg(long, requires = "code", conflicts_with = "token")]
        server: Option<String>,
        /// Código do convite (com `--server`).
        #[arg(long, requires = "server", conflicts_with = "token")]
        code: Option<String>,
        /// Endereço gRPC do controller (ex.: `http://IP:50051`).
        #[arg(long)]
        controller: Option<String>,
        /// Token de join, para trocar só o controller (com `--controller`).
        #[arg(long, requires = "controller", conflicts_with_all = ["server", "code"])]
        token: Option<String>,
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
                yes,
                lang,
            } => {
                let mut out = vec!["join".to_string()];
                valor(&mut out, "--server", Some(server));
                valor(&mut out, "--code", Some(code));
                marca(&mut out, "--yes", *yes);
                valor(&mut out, "--lang", lang.as_ref());
                out
            }
            Self::Status => vec!["status".to_string()],
            Self::Logs(registros) => registros.args(),
            Self::Role { short } => {
                let mut out = vec!["role".to_string()];
                marca(&mut out, "--short", *short);
                out
            }
            Self::Change {
                server,
                code,
                controller,
                token,
            } => {
                let mut out = vec!["change".to_string()];
                valor(&mut out, "--server", server.as_ref());
                valor(&mut out, "--code", code.as_ref());
                valor(&mut out, "--controller", controller.as_ref());
                valor(&mut out, "--token", token.as_ref());
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
    /// Atualiza este computador (no AI Cluster OS, pede ao `aic-updater` que cheque agora).
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

/// Pasta do instalador: `AIC_LIBEXEC`, depois as padrões; a primeira que contém `setup`.
fn procura(env: Option<OsString>, padroes: &[PathBuf]) -> Option<PathBuf> {
    env.filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .into_iter()
        .chain(padroes.iter().cloned())
        .find(|dir| dir.join("setup").is_file())
}

/// Roda `<libexec>/setup <substantivo> <argumentos…>` e encerra com o código dele.
/// Em unix o processo é trocado pelo `setup` (exec); nos demais, o `setup` roda como filho.
pub fn encaminha(substantivo: &str, args: Vec<String>) -> ! {
    let padroes = PADROES.map(PathBuf::from);
    let Some(dir) = procura(std::env::var_os("AIC_LIBEXEC"), &padroes) else {
        eprintln!("erro: este computador não tem o instalador do AI Cluster; use o install.sh");
        exit(1);
    };
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
