---
title: "ADR-029 — Versão única do produto"
aliases: ["ADR-029", "versão única", "AIC_RELEASE", "SemVer do produto"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
---

# ADR-029 — Versão única do produto

Status: decidido · Data: 2026-11-01 · Voltar para [[decisoes]].

## Contexto

Havia três linhas de versão: a release SemVer (0.2.1: crates, setup, binários Linux), a numeração própria das imagens do SO (x86 1.0/2.0/3.0, Pi 12.0.1, de `AIC_RELEASE` à mão) e o `min-version` do instalador dedicado. Um worker instalado mostrava `1.0` enquanto a release era 0.2.1.

## Decisão

1. **A release SemVer X.Y.Z vale para tudo:** crates, setup, binários Linux, imagem do SO (x86 e Pi), instalador e pasta dedicado.
2. **Fonte única:** `version` em `[workspace.package]` de `code/Cargo.toml` (os crates herdam com `version.workspace = true`). Scripts leem por `code/scripts/version.sh`, que valida X.Y.Z. `AIC_RELEASE` só sobrescreve, para teste (também validado).
3. **Comparação por SemVer numérico por componente** (`sort -V` no `aic-updater`; `awk` no `aic-install.sh`). A regra de não baixar versão vale por essa comparação. Leitura de formatos antigos (`1.0`, `12.0.1`) continua aceita onde já era; nada exige migração. O Pi é regravado com a primeira release do regime único.
4. O controller só compara igualdade entre a versão que o worker roda (`/etc/aic-release`, ou a do binário) e a da release; `validate` aceita o mesmo alfabeto de antes.

## Mapa antes e depois

| Ponto | Antes | Depois |
|---|---|---|
| Crates, `aic version`, banner | `version` em cada `Cargo.toml` | `[workspace.package]` (um só) |
| `package-release.sh` (VERSION da release e da pasta dedicado) | lia `crates/controller/Cargo.toml` | `scripts/version.sh` |
| `os/image/build-rootfs.sh` (Pi), `x86/build-rootfs.sh` | `AIC_RELEASE` à mão, padrão `dev` | `scripts/version.sh` |
| `/etc/aic-release` (Dockerfiles) | o número à mão | X.Y.Z do produto |
| `x86/build-installer.sh` (`/etc/aic/min-version`) | `AIC_RELEASE` obrigatório (ex.: 1.0) | `scripts/version.sh` |
| `sign-release.sh <versão>` | número à mão | o operador passa `$(sh code/scripts/version.sh)` |
| `aic-updater`, `aic-install.sh` (`versao_ok`) | compara numericamente | igual (já aceita X.Y.Z) |
| Rollout (`rollout.rs`), `aic update status`, `aic cluster nodes` (VERSÃO), telas do console | exibem o que o worker reporta | igual; agora é sempre X.Y.Z |

## Consequências

- Rebuild de imagem sem subir a versão é recusado pelo atualizador (mesma versão). Para testar, use `AIC_RELEASE=X.Y.Z`.
- Imagens e `rootfs-*.ext4` antigas (1.0, 12.0.1) não participam: o Pi é regravado e os arquivos antigos são limpos (ver [[pendencias]]).

## Alternativas descartadas

- Manter a numeração própria da imagem: dois números para o mesmo produto confundem o operador.
- `AIC_RELEASE` como fonte: dependeria de memória do operador a cada build.
