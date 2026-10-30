//! A ajuda e os erros de uso do `ai-cluster` em português. O clap só fala inglês nos títulos
//! ("Usage", "Commands", "Options"), no subcomando `help` e nas mensagens de erro; aqui os
//! títulos vêm de um modelo próprio e os erros são reescritos. O texto de cada comando e opção
//! já é português nos comentários de `main.rs`.

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Arg, ArgAction, Command};

const COM_SUBCOMANDOS: &str =
    "{about}\n\nUso: {usage}\n\nComandos:\n{subcommands}\n\nOpções:\n{options}\n";
const COM_ARGUMENTOS: &str =
    "{about}\n\nUso: {usage}\n\nArgumentos:\n{positionals}\n\nOpções:\n{options}\n";
const SO_OPCOES: &str = "{about}\n\nUso: {usage}\n\nOpções:\n{options}\n";

/// Aplica o modelo em português a um comando e, recursivamente, a todos os subcomandos.
pub fn localize(cmd: Command) -> Command {
    let template = if cmd.has_subcommands() {
        COM_SUBCOMANDOS
    } else if cmd.get_positionals().next().is_some() {
        COM_ARGUMENTOS
    } else {
        SO_OPCOES
    };
    let cmd = cmd.mut_args(|arg| {
        let mut notes = Vec::new();
        let defaults: Vec<String> = arg
            .get_default_values()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        if !defaults.is_empty() {
            notes.push(format!("padrão: {}", defaults.join(", ")));
        }
        if let Some(env) = arg.get_env() {
            notes.push(format!("variável: {}", env.to_string_lossy()));
        }
        if notes.is_empty() {
            return arg;
        }
        let help = arg.get_help().map(ToString::to_string).unwrap_or_default();
        arg.help(format!("{help} ({})", notes.join("; ")))
            .hide_default_value(true)
            .hide_env(true)
    });
    cmd.help_template(template)
        .disable_help_subcommand(true)
        .disable_help_flag(true)
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help("Mostra esta ajuda"),
        )
        .mut_subcommands(localize)
}

fn values(err: &clap::Error, kind: ContextKind) -> Vec<String> {
    match err.get(kind) {
        Some(ContextValue::String(one)) => vec![one.clone()],
        Some(ContextValue::Strings(many)) => many.clone(),
        _ => Vec::new(),
    }
}

/// As palavras fixas do clap que sobram no texto de uso.
fn words(text: &str) -> String {
    // `--controller` é global, com padrão e variável de ambiente: o clap o põe na dica de uso como se
    // fosse obrigatório, e não é. Só as linhas de uso mudam; a lista de opções fica como está.
    text.lines()
        .map(|line| {
            if line.trim_start().starts_with("--") {
                line.to_string()
            } else {
                line.replace("--controller <CONTROLLER> ", "")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .replace("[OPTIONS]", "[OPÇÕES]")
        .replace("<COMMAND>", "<COMANDO>")
        .replace("Usage:", "Uso:")
        + if text.ends_with('\n') { "\n" } else { "" }
}

/// Mostra ajuda (saída 0) ou erro de uso (saída 2) em português e encerra.
pub fn show(err: &clap::Error) -> ! {
    if err.use_stderr() {
        eprint!("{}", message(err));
        std::process::exit(2);
    }
    print!("{}", words(&err.render().to_string()));
    std::process::exit(0);
}

/// O primeiro trecho entre aspas simples da primeira linha do erro (`unexpected argument '--x' found`).
fn quoted(first_line: &str) -> String {
    first_line
        .split('\'')
        .nth(1)
        .unwrap_or_default()
        .to_string()
}

/// A mensagem de um erro de uso, em português. Ajuda e versão não passam por aqui. O clap guarda
/// pouco do erro no contexto, então o que falta (subcomandos, uso) vem do texto que ele renderiza.
pub fn message(err: &clap::Error) -> String {
    let rendered = err.render().to_string();
    let first = rendered.lines().next().unwrap_or_default().to_string();
    let usage = rendered
        .lines()
        .skip_while(|line| !line.starts_with("Usage:"))
        .take_while(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join(
            "
",
        )
        .replacen("Usage:", "Uso:", 1);
    let usage = words(&usage);
    let invalid = values(err, ContextKind::InvalidArg).join(", ");
    let invalid = if invalid.is_empty() {
        quoted(&first)
    } else {
        invalid
    };
    let head = match err.kind() {
        ErrorKind::MissingSubcommand | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            // O clap já renderiza a ajuda do grupo (com a lista de subcomandos): vem logo após o erro.
            return format!(
                "erro: falta o subcomando

{}",
                words(&rendered)
            );
        }
        ErrorKind::InvalidSubcommand => format!("subcomando desconhecido: {}", quoted(&first)),
        ErrorKind::UnknownArgument => format!("argumento desconhecido: {invalid}"),
        ErrorKind::MissingRequiredArgument => {
            let missing: Vec<&str> = rendered
                .lines()
                .skip(1)
                .take_while(|line| !line.trim().is_empty())
                .map(str::trim)
                .collect();
            format!("faltam argumentos obrigatórios: {}", missing.join(" "))
        }
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => {
            format!("valor inválido para {invalid}: {first}")
        }
        ErrorKind::ArgumentConflict => format!("opções que não combinam: {first}"),
        // O que não tem texto próprio sai como o clap o escreveu.
        _ => first.trim_start_matches("error: ").to_string(),
    };
    let mut out = format!(
        "erro: {head}
"
    );
    if !usage.is_empty() {
        out.push_str(&format!(
            "
{usage}
"
        ));
    }
    out.push_str(
        "
Para saber mais: ai-cluster --help (ou <comando> --help)
",
    );
    out
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser, Subcommand};

    #[derive(Parser, Debug)]
    #[command(name = "t")]
    struct Cli {
        #[command(subcommand)]
        command: Group,
    }

    #[derive(Subcommand, Debug)]
    enum Group {
        /// Mostra algo.
        Show {
            /// Id.
            id: String,
        },
    }

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::command()
            .pipe(super::localize)
            .try_get_matches_from(args)
            .map(|_| Cli {
                command: Group::Show { id: String::new() },
            })
    }

    trait Pipe: Sized {
        fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
            f(self)
        }
    }
    impl<T> Pipe for T {}

    #[test]
    fn os_erros_de_uso_saem_em_portugues() {
        let missing = parse(&["t"]).unwrap_err();
        let text = super::message(&missing);
        assert!(text.starts_with("erro: falta o subcomando"), "{text}");
        assert!(
            text.contains("Comandos:") && text.contains("show"),
            "{text}"
        );
        assert!(
            !text.contains("Usage") && !text.contains("<COMMAND>"),
            "{text}"
        );

        let unknown = super::message(&parse(&["t", "xyz"]).unwrap_err());
        assert!(
            unknown.contains("subcomando desconhecido: xyz"),
            "{unknown}"
        );

        let flag = super::message(&parse(&["t", "show", "--zzz"]).unwrap_err());
        assert!(flag.contains("argumento desconhecido: --zzz"), "{flag}");
        assert!(flag.contains("Uso:"), "{flag}");
    }

    #[test]
    fn a_ajuda_usa_os_titulos_em_portugues() {
        let help = parse(&["t", "--help"]).unwrap_err().render().to_string();
        assert!(
            help.contains("Uso:") && help.contains("Comandos:") && help.contains("Opções:"),
            "{help}"
        );
        assert!(!help.contains("Usage") && !help.contains("Print"), "{help}");
    }

    #[test]
    fn a_dica_de_uso_nao_mostra_o_controller_como_obrigatorio() {
        assert_eq!(
            super::words("Usage: ai-cluster node show --controller <CONTROLLER> <ID>"),
            "Uso: ai-cluster node show <ID>"
        );
    }
}
