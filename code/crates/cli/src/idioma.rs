//! Idioma das mensagens da CLI que mudam por aparelho (lista e aprovação de instalação). O padrão é
//! português, como o instalador de shell: `AIC_LANG=en` troca para inglês.

/// Verdadeiro quando `AIC_LANG` pede inglês.
pub fn ingles() -> bool {
    std::env::var("AIC_LANG").is_ok_and(|valor| valor.trim() == "en")
}

/// O texto no idioma escolhido.
pub fn t(pt: &'static str, en: &'static str) -> &'static str {
    if ingles() { en } else { pt }
}
