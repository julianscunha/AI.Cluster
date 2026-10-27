---
title: "ADR-008 — Stack inicial"
aliases: ["ADR-008", "Decisão de stack inicial", "current-stack"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
---

# ADR-008 — Stack inicial

Status: vivo · Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

O primeiro protótipo precisa de uma stack concreta. Estas são escolhas de implementação para o protótipo, não contratos arquiteturais irreversíveis. Na data do registro, as linhas de release usadas foram confirmadas na documentação upstream: tonic 0.14.x, prost 0.14.x, clap 4.6.x e Ratatui 0.30.x.

## Decisão

| Área | Escolha | Versão |
|---|---|---|
| Linguagem | Rust, edition 2024 | toolchain do workspace |
| Runtime assíncrono | tokio (macros, rt-multi-thread, time, sync, signal) | 1.53.3 |
| RPC | gRPC sobre HTTP/2 com tonic; TLS pela feature `tls-ring` | tonic 0.14.4 |
| Schema | Protocol Buffers, tipos gerados com prost e tonic-prost-build | prost 0.14.6 |
| Compilador de proto | `protoc` vendorado (`protoc-bin-vendored`); o `build.rs` de `crates/protocol` aponta `PROTOC` para ele | protoc-bin-vendored 3.3.0 |
| HTTP | axum (exportador de métricas no controller e API do gateway) | 0.8.9 |
| CLI | clap, com derive e env | 4.6.7 |
| TUI | Ratatui | planejado |

## Consequências

- O build não depende de `protoc` instalado; o protocolo é gerado a partir de `code/proto/`.
- O workspace `code/` tem nove crates: `common`, `protocol`, `controller`, `worker`, `transport`, `benchmark`, `cli`, `gateway` e `engine`, todos em edition 2024.
- Trocar a stack exige mexer no protocolo gerado e nos binários. A decisão é reversível, mas não é barata.

## Alternativas

- Nenhuma alternativa registrada.

## Limites

- Ratatui não é usado. O painel `ai-cluster dashboard` é saída de texto, com ou sem `--watch`. Ratatui só entra se a TUI full-screen for feita ([[cli-tui]]).
