//! `aic cluster model pull`: baixa um `.gguf` para a pasta de modelos do controller. Só a UX: o arquivo é o
//! mesmo que o `model list` já lê, sem formato novo. Usa o `curl` do sistema (como o instalador), só por HTTPS.

use crate::idioma::t;
use std::io::Read;
use std::path::Path;
use std::process::Command;

/// URL de download e nome do arquivo a partir de `https://…/x.gguf` ou `dono/repo/[pasta/]x.gguf` (Hugging Face).
pub fn alvo(origem: &str) -> Result<(String, String), String> {
    let origem = origem.trim();
    if origem.starts_with("http://") {
        return Err(t("só https:// é aceito", "only https:// is accepted").into());
    }
    let (url, caminho) = if let Some(resto) = origem.strip_prefix("https://") {
        (
            origem.to_string(),
            resto.split(['?', '#']).next().unwrap_or(""),
        )
    } else {
        let partes: Vec<&str> = origem.splitn(3, '/').collect();
        if partes.len() != 3 || partes.iter().any(|p| p.is_empty()) {
            return Err(t(
                "use uma URL https:// ou dono/repo/arquivo.gguf",
                "use an https:// URL or owner/repo/file.gguf",
            )
            .into());
        }
        (
            format!(
                "https://huggingface.co/{}/{}/resolve/main/{}",
                partes[0], partes[1], partes[2]
            ),
            partes[2],
        )
    };
    let nome = caminho.rsplit('/').next().unwrap_or("");
    let valido = nome.to_ascii_lowercase().ends_with(".gguf")
        && !nome.starts_with('.')
        && nome
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c));
    if !valido {
        return Err(t(
            "o arquivo deve ser um .gguf (letras, dígitos, ponto, hífen, sublinhado)",
            "the file must be a .gguf (letters, digits, dot, dash, underscore)",
        )
        .into());
    }
    Ok((url, nome.to_string()))
}

/// O último `Content-Length` de uma resposta de `curl -sIL` (com redirecionamentos há vários blocos).
pub fn tamanho(cabecalhos: &str) -> Option<u64> {
    cabecalhos
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())?
        })
        .last()
}

fn curl(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("curl")
        .args(["--proto", "=https", "--tlsv1.2", "--proto-redir", "=https"])
        .args(args)
        .output()
        .map_err(|e| format!("curl: {e}"))
}

/// Baixa `origem` para `dir`, confere tamanho (quando o servidor informa), assinatura `GGUF` e, se dado, o SHA-256.
pub fn pull(origem: &str, dir: &Path, sha256: Option<&str>) -> Result<(), String> {
    let (url, nome) = alvo(origem)?;
    let destino = dir.join(&nome);
    if destino.exists() {
        return Err(format!(
            "{}: {}",
            destino.display(),
            t("já existe", "already exists")
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let head = curl(&["-sIL", "--max-time", "30", &url])?;
    let esperado = tamanho(&String::from_utf8_lossy(&head.stdout));
    let parcial = dir.join(format!("{nome}.part"));
    let parcial_s = parcial.to_string_lossy();
    // `-C -` retoma um download interrompido; o `.part` fica para a próxima tentativa.
    let status = Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "--proto-redir",
            "=https",
            "-fL",
            "-C",
            "-",
            "--progress-bar",
            "-o",
        ])
        .arg(&*parcial_s)
        .arg(&url)
        .status()
        .map_err(|e| format!("curl: {e}"))?;
    if !status.success() {
        return Err(t(
            "o download falhou (rode de novo para retomar)",
            "the download failed (run again to resume)",
        )
        .into());
    }
    let confere = (|| {
        let (bytes, soma) = ai_cluster_engine::manifest::hash_file(&parcial)?;
        if esperado.is_some_and(|e| e != bytes) {
            return Err(format!(
                "{}: {bytes} != {}",
                t(
                    "tamanho diferente do anunciado",
                    "size differs from the announced one"
                ),
                esperado.unwrap_or(0)
            ));
        }
        let mut magia = [0u8; 4];
        std::fs::File::open(&parcial)
            .and_then(|mut f| f.read_exact(&mut magia))
            .map_err(|e| e.to_string())?;
        if &magia != b"GGUF" {
            return Err(t("o arquivo não é um GGUF", "the file is not a GGUF").into());
        }
        if sha256.is_some_and(|s| !s.trim().eq_ignore_ascii_case(&soma)) {
            return Err(format!(
                "{}: {soma}",
                t(
                    "SHA-256 diferente do informado",
                    "SHA-256 differs from the given one"
                )
            ));
        }
        Ok((bytes, soma))
    })();
    match confere {
        Ok((bytes, soma)) => {
            std::fs::rename(&parcial, &destino).map_err(|e| e.to_string())?;
            println!("{}  {bytes:>12}  {nome}", soma);
            println!(
                "{} {}",
                t("modelo guardado em", "model stored in"),
                destino.display()
            );
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&parcial); // arquivo ruim não serve para retomar
            Err(e)
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn alvo_de_url_e_de_repo() {
        assert_eq!(
            alvo("https://huggingface.co/a/b/resolve/main/m-Q4_K_M.gguf?download=true").unwrap(),
            (
                "https://huggingface.co/a/b/resolve/main/m-Q4_K_M.gguf?download=true".into(),
                "m-Q4_K_M.gguf".into()
            )
        );
        assert_eq!(
            alvo("bartowski/x-GGUF/sub/x-Q4.gguf").unwrap(),
            (
                "https://huggingface.co/bartowski/x-GGUF/resolve/main/sub/x-Q4.gguf".into(),
                "x-Q4.gguf".into()
            )
        );
    }

    #[test]
    fn recusa_o_que_nao_e_gguf_https() {
        for ruim in [
            "http://h/x.gguf",
            "a/b",
            "a/b/x.bin",
            "https://h/",
            "a//x.gguf",
            "https://h/.gguf",
            "a/b/x y.gguf",
        ] {
            assert!(alvo(ruim).is_err(), "{ruim}");
        }
    }

    #[test]
    fn ultimo_content_length_vence() {
        let h = "HTTP/2 302\r\ncontent-length: 5\r\n\r\nHTTP/2 200\r\nContent-Length: 1234\r\n\r\n";
        assert_eq!(tamanho(h), Some(1234));
        assert_eq!(tamanho("HTTP/2 200\r\n"), None);
    }
}
