---
title: "ADR-008 — Stack inicial"
aliases: ["ADR-008", "Decisão de stack inicial", "current-stack"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
status: vivo
implementacao: implementado
fases: ["[[fase-00-fundacao]]"]
---

# ADR-008 — Stack inicial

Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

O primeiro protótipo precisa de uma stack concreta. Estas são escolhas de implementação para o protótipo, não contratos arquiteturais irreversíveis. A documentação upstream confirmou, na data do registro, as linhas de release usadas: tonic 0.14.x, prost 0.14.x, clap 4.6.x e Ratatui 0.30.x.

## Decisão

| Área | Escolha | Versão no `code/Cargo.lock` |
|---|---|---|
| Linguagem | Rust, edition 2024 | toolchain do workspace |
| Runtime assíncrono | tokio (macros, rt-multi-thread, time, sync, signal) | 1.53.3 |
| RPC | gRPC sobre HTTP/2 com tonic; TLS pela feature `tls-ring` | tonic 0.14.4 |
| Schema | Protocol Buffers, tipos gerados com prost e tonic-prost-build | prost 0.14.6 |
| Compilador de proto | `protoc` vendorado (`protoc-bin-vendored`); o `build.rs` de `crates/protocol` aponta `PROTOC` para ele, sem instalar protoc no sistema | protoc-bin-vendored 3.3.0 |
| HTTP | axum (exportador de métricas no controller e API do gateway) | 0.8.9 |
| CLI | clap, com derive e env | 4.6.7 |
| TUI | Ratatui (Fase H) | não adotado |

## Consequências

- O build não depende de `protoc` instalado; o protocolo é gerado a partir de `code/proto/`.
- Trocar a stack exige mexer no protocolo gerado e nos binários. A decisão é reversível, mas não é barata.

## Alternativas

- Não registradas no ADR original.

> [!warning] Desvio: TUI
> Ratatui não aparece em nenhum `Cargo.toml`. O painel de uma tela (`ai-cluster dashboard`, Fase H4) é saída de texto, com ou sem `--watch`. Ver [[cli-tui]].

> [!info] O que já existe
> Workspace `code/` com nove crates (`common`, `protocol`, `controller`, `worker`, `transport`, `benchmark`, `cli`, `gateway`, `engine`), todos em edition 2024, e o protocolo em `code/proto/cluster.proto`. Ver [[fase-00-fundacao]] e [[protocolo-controller-worker]].

> [!todo] Ratatui só entra se a TUI for implementada; ver [[cli-tui]].
