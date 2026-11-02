//! Seletor único de idioma dos binários Rust. O padrão é português, como o instalador de shell:
//! `AIC_LANG=en` (também `en_US.UTF-8`) troca para inglês. Texto novo de console passa por `t`.

/// Verdadeiro quando o valor de `AIC_LANG` pede inglês.
pub fn pede_ingles(valor: &str) -> bool {
    let v = valor.trim().to_ascii_lowercase();
    v == "en" || v.starts_with("en_") || v.starts_with("en-") || v.starts_with("en.")
}

/// Verdadeiro quando `AIC_LANG` pede inglês.
pub fn ingles() -> bool {
    std::env::var("AIC_LANG").is_ok_and(|valor| pede_ingles(&valor))
}

/// O texto no idioma escolhido.
pub fn t(pt: &'static str, en: &'static str) -> &'static str {
    if ingles() { en } else { pt }
}

#[cfg(test)]
mod testes {
    use super::pede_ingles;

    #[test]
    fn reconhece_ingles() {
        for v in ["en", " EN ", "en_US.UTF-8", "en-GB"] {
            assert!(pede_ingles(v), "{v}");
        }
        for v in ["", "pt", "pt_BR", "ent", "de"] {
            assert!(!pede_ingles(v), "{v}");
        }
    }
}
