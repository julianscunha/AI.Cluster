//! Contrato com o instalador de shell: `aic` roda `<libexec>/setup <substantivo> <verbo> [args…]`
//! e repassa os argumentos sem reinterpretá-los (ADR-024). Só unix: o `setup` é um script de shell.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn pasta(nome: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aic-teste-{}-{nome}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Grava um `setup` executável em `dir` com o corpo de shell dado.
fn instala_setup(dir: &Path, corpo: &str) {
    let caminho = dir.join("setup");
    fs::write(&caminho, format!("#!/bin/sh\n{corpo}\n")).unwrap();
    fs::set_permissions(&caminho, fs::Permissions::from_mode(0o755)).unwrap();
}

fn aic(libexec: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aic"))
        .args(args)
        .env("AIC_LIBEXEC", libexec)
        .output()
        .unwrap()
}

#[test]
fn cada_comando_delegado_repassa_a_linha_ao_setup() {
    let dir = pasta("contrato");
    let saida = dir.join("recebido.txt");
    instala_setup(
        &dir,
        &format!("printf '%s\\n' \"$@\" > '{}'", saida.display()),
    );
    let casos: &[(&[&str], &[&str])] = &[
        (
            &[
                "controller",
                "install",
                "--with-gateway",
                "--ip",
                "10.0.0.5",
                "--models-dir",
                "/m",
                "--yes",
                "--lang",
                "en",
            ],
            &[
                "controller",
                "install",
                "--with-gateway",
                "--ip",
                "10.0.0.5",
                "--models-dir",
                "/m",
                "--yes",
                "--lang",
                "en",
            ],
        ),
        (&["controller", "status"], &["controller", "status"]),
        (
            &["controller", "logs", "-f", "-n", "50"],
            &["controller", "logs", "-f", "-n", "50"],
        ),
        (
            &[
                "gateway",
                "install",
                "--server",
                "10.0.0.5:8099",
                "--code",
                "123456",
                "--yes",
            ],
            &[
                "gateway",
                "install",
                "--server",
                "10.0.0.5:8099",
                "--code",
                "123456",
                "--yes",
            ],
        ),
        (
            &["gateway", "logs", "-n", "20"],
            &["gateway", "logs", "-n", "20"],
        ),
        (&["gateway", "status"], &["gateway", "status"]),
        (
            &[
                "worker", "join", "--server", "10.0.0.5", "--code", "123456", "--yes", "--lang",
                "pt",
            ],
            &[
                "worker", "join", "--server", "10.0.0.5", "--code", "123456", "--yes", "--lang",
                "pt",
            ],
        ),
        (&["worker", "status"], &["worker", "status"]),
        (&["worker", "logs", "-f"], &["worker", "logs", "-f"]),
        (
            &["worker", "role", "--short"],
            &["worker", "role", "--short"],
        ),
        (
            &[
                "worker",
                "change",
                "--server",
                "10.0.0.5",
                "--code",
                "9",
                "--controller",
                "http://10.0.0.5:50051",
            ],
            &[
                "worker",
                "change",
                "--server",
                "10.0.0.5",
                "--code",
                "9",
                "--controller",
                "http://10.0.0.5:50051",
            ],
        ),
        (
            &[
                "worker",
                "change",
                "--controller",
                "http://10.0.0.5:50051",
                "--token",
                "T",
            ],
            &[
                "worker",
                "change",
                "--controller",
                "http://10.0.0.5:50051",
                "--token",
                "T",
            ],
        ),
        (
            &["worker", "passwd", "--remove"],
            &["worker", "passwd", "--remove"],
        ),
        (&["update", "status"], &["update", "status"]),
        (
            &["update", "apply", "--version", "1.2.0", "--force", "--yes"],
            &["update", "apply", "--version", "1.2.0", "--force", "--yes"],
        ),
        (
            &["cluster", "invite", "--minutes", "15", "--port", "8099"],
            &["cluster", "invite", "--minutes", "15", "--port", "8099"],
        ),
        (&["cluster", "invite"], &["cluster", "invite"]),
    ];
    for (args, esperado) in casos {
        let _ = fs::remove_file(&saida);
        let out = aic(&dir, args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let recebido = fs::read_to_string(&saida).unwrap();
        let recebido: Vec<&str> = recebido.lines().collect();
        assert_eq!(&recebido, esperado, "linha para {args:?}");
    }
}

#[test]
fn o_codigo_de_saida_do_setup_vira_o_do_aic() {
    let dir = pasta("codigo");
    instala_setup(&dir, "exit 7");
    let out = aic(&dir, &["worker", "status"]);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn sem_instalador_o_aic_avisa_e_sai_com_erro() {
    if Path::new("/opt/ai-cluster/libexec/setup").exists()
        || Path::new("/usr/libexec/aic/setup").exists()
    {
        eprintln!("pulado: há um instalador nas pastas padrão deste computador");
        return;
    }
    let dir = pasta("ausente");
    let out = aic(&dir, &["update", "status"]);
    assert_eq!(out.status.code(), Some(1));
    let erro = String::from_utf8_lossy(&out.stderr);
    assert!(
        erro.contains("este computador não tem o instalador do AI Cluster; use o install.sh"),
        "{erro}"
    );
}
