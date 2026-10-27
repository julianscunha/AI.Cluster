//! A identidade visual do Real.AI.Cluster, compartilhada por todos os componentes (CLI, painel,
//! controller, gateway, worker). Os scripts do SO do nó seguem a mesma paleta e o mesmo logotipo em
//! `os/image/overlay/usr/lib/aic/ui.sh`; mexa nos dois juntos.
//!
//! Cor e glifos de caixa só saem num terminal que os aceita: a saída redirecionada para arquivo ou
//! para um coletor de logs, `NO_COLOR` e `AI_CLUSTER_COLOR=never` ficam em texto puro, e
//! `AI_CLUSTER_ASCII=1` troca os glifos Unicode por ASCII.

use std::io::IsTerminal;

/// O papel de um texto na tela. A cor de cada papel é a mesma em todo o sistema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Título e nome do produto.
    Strong,
    /// Rótulos e o logotipo (ciano).
    Accent,
    /// Tudo certo (verde).
    Ok,
    /// Atenção: pendente, degradado (amarelo).
    Warn,
    /// Falha ou sem sinal (vermelho).
    Bad,
    /// Apoio, de segundo plano (cinza).
    Dim,
}

impl Tone {
    fn code(self) -> &'static str {
        match self {
            Tone::Strong => "1;97",
            Tone::Accent => "36",
            Tone::Ok => "92",
            Tone::Warn => "93",
            Tone::Bad => "91",
            Tone::Dim => "2;37",
        }
    }
}

/// Se a saída padrão aceita cor. `AI_CLUSTER_COLOR=always` força, `never` e `NO_COLOR` desligam;
/// do contrário, só um terminal de verdade (não `TERM=dumb`) recebe cor.
pub fn color_enabled() -> bool {
    decide_color(
        std::env::var("AI_CLUSTER_COLOR").ok().as_deref(),
        std::env::var_os("NO_COLOR").is_some(),
        std::env::var("TERM").ok().as_deref(),
        std::io::stdout().is_terminal(),
    )
}

/// A decisão pura, para testar sem mexer no ambiente.
fn decide_color(setting: Option<&str>, no_color: bool, term: Option<&str>, is_tty: bool) -> bool {
    match setting {
        Some("always") => return true,
        Some("never") => return false,
        _ => {}
    }
    !no_color && is_tty && term != Some("dumb")
}

/// Pinta `text` com o tom, se a cor está ligada; do contrário devolve o texto como veio.
pub fn paint(text: &str, tone: Tone) -> String {
    paint_with(text, tone, color_enabled())
}

fn paint_with(text: &str, tone: Tone, on: bool) -> String {
    if on {
        format!("\x1b[{}m{text}\x1b[0m", tone.code())
    } else {
        text.to_string()
    }
}

/// O tom de um estado de nó: pronto é verde, sem sinal ou em quarentena é vermelho, o resto
/// (drenando, pendente, desconhecido) é atenção.
pub fn state_tone(state: &str) -> Tone {
    match state {
        "READY" | "VIVO" => Tone::Ok,
        "OFFLINE" | "QUARANTINED" | "SEM SINAL" => Tone::Bad,
        _ => Tone::Warn,
    }
}

/// Os glifos, em Unicode ou em ASCII (`AI_CLUSTER_ASCII=1`, ou um terminal que não é UTF-8).
pub struct Glyphs {
    pub node: &'static str,
    pub rule: &'static str,
    pub up: &'static str,
    pub flat: &'static str,
    pub down: &'static str,
}

pub fn glyphs() -> Glyphs {
    let ascii = std::env::var("AI_CLUSTER_ASCII").is_ok_and(|v| v == "1")
        // a primeira variável de locale definida decide (ordem do POSIX: LC_ALL, LC_CTYPE, LANG)
        || ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .is_some_and(|value| !value.to_uppercase().contains("UTF"));
    if ascii {
        Glyphs {
            node: "o",
            rule: "-",
            up: "^",
            flat: "=",
            down: "v",
        }
    } else {
        Glyphs {
            node: "■",
            rule: "─",
            up: "↑",
            flat: "→",
            down: "↓",
        }
    }
}

/// O logotipo, uma malha de sete nós em cinco linhas (a mesma do console do SO).
pub fn logo() -> Vec<String> {
    let g = glyphs();
    let n = |tone| paint(g.node, tone);
    let h = paint(&g.rule.repeat(3), Tone::Accent);
    vec![
        format!("   {}{h}{}", n(Tone::Strong), n(Tone::Strong)),
        paint("  / \\ / \\", Tone::Accent),
        format!(
            " {}{h}{}{h}{}",
            n(Tone::Strong),
            n(Tone::Strong),
            n(Tone::Strong)
        ),
        paint("  \\ / \\ /", Tone::Accent),
        format!("   {}{h}{}", n(Tone::Strong), n(Tone::Strong)),
    ]
}

/// A abertura de um componente: o logotipo à esquerda e, ao lado, o nome, a versão e uma linha de
/// apoio. Em saída que não é terminal (log de contêiner, arquivo) vira uma linha só, sem desenho.
pub fn banner(component: &str, version: &str, detail: &str) -> String {
    if !color_enabled() {
        return format!("Real.AI.Cluster {component} v{version} - {detail}");
    }
    let logo = logo();
    let text = [
        String::new(),
        paint(&format!("Real.AI.Cluster {component}"), Tone::Strong),
        paint("IA distribuida em hardware comum", Tone::Dim),
        paint(&format!("versao {version}"), Tone::Dim),
        paint(detail, Tone::Accent),
    ];
    // Largura visível de cada linha do desenho (sem os códigos de cor), para alinhar o texto.
    const ART_WIDTHS: [usize; 5] = [8, 9, 10, 9, 8];
    logo.iter()
        .zip(text.iter())
        .zip(ART_WIDTHS)
        .map(|((art, line), width)| format!("{art}{}   {line}", " ".repeat(11 - width)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Uma linha de seção: `── Título ─────────` com a largura pedida (no mínimo o título).
pub fn rule(title: &str, width: usize) -> String {
    let g = glyphs();
    let fill = width.saturating_sub(title.chars().count() + 4);
    format!(
        "{} {} {}",
        paint(&g.rule.repeat(2), Tone::Dim),
        paint(title, Tone::Accent),
        paint(&g.rule.repeat(fill.max(2)), Tone::Dim)
    )
}

/// Um ponto de status colorido pelo tom, seguido do texto.
pub fn dot(tone: Tone, text: &str) -> String {
    format!("{} {text}", paint(glyphs().node, tone))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cor_so_liga_num_terminal_de_verdade_ou_se_for_forcada() {
        assert!(decide_color(None, false, Some("xterm"), true));
        assert!(
            !decide_color(None, false, Some("xterm"), false),
            "saída redirecionada"
        );
        assert!(!decide_color(None, true, Some("xterm"), true), "NO_COLOR");
        assert!(!decide_color(None, false, Some("dumb"), true), "TERM=dumb");
        assert!(
            decide_color(Some("always"), true, None, false),
            "forçada vence"
        );
        assert!(!decide_color(Some("never"), false, Some("xterm"), true));
    }

    #[test]
    fn sem_cor_o_texto_sai_como_veio() {
        assert_eq!(paint_with("pronto", Tone::Ok, false), "pronto");
        assert_eq!(
            paint_with("pronto", Tone::Ok, true),
            "\x1b[92mpronto\x1b[0m"
        );
    }

    #[test]
    fn cada_estado_tem_o_seu_tom() {
        assert_eq!(state_tone("READY"), Tone::Ok);
        assert_eq!(state_tone("OFFLINE"), Tone::Bad);
        assert_eq!(state_tone("QUARANTINED"), Tone::Bad);
        assert_eq!(state_tone("DRAINING"), Tone::Warn);
        assert_eq!(state_tone("qualquer outro"), Tone::Warn);
    }

    #[test]
    fn a_linha_de_secao_tem_a_largura_pedida_e_nunca_menos_que_o_titulo() {
        // sem terminal os testes rodam em texto puro
        let line = rule("Nós", 30);
        assert_eq!(line.chars().count(), 30, "{line}");
        assert!(line.contains("Nós"));
        assert!(rule("Um título bem comprido", 5).contains("Um título bem comprido"));
    }

    #[test]
    fn o_logotipo_tem_cinco_linhas_e_o_banner_em_log_e_uma_linha_so() {
        assert_eq!(logo().len(), 5);
        // `cargo test` não é um terminal: o banner é uma linha de texto puro
        let banner = banner("controller", "0.1.1", "escutando em 0.0.0.0:50051");
        assert_eq!(banner.lines().count(), 1, "{banner}");
        assert!(banner.contains("controller") && banner.contains("0.1.1"));
    }
}
