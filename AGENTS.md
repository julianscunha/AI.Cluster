---
title: "AI Cluster"
type: system
status: living
tags: [system, agents]
---

# AI Cluster

O AI Cluster é um runtime de IA distribuído Linux-first. A raiz do repositório é a camada de arquitetura/especificação; o único workspace executável é `code/`, que atualmente é um esqueleto Rust da Fase 0 para o substrato Controller/Worker — não um runtime de LLM.

## Fonte da verdade

- Trate arquivos locais como autoritativos.
- `ARCHITECTURE.md` é o documento mestre de arquitetura.
- `STATUS.md` distingue decisões decididas, propostas, experimentais e em aberto.
- Mantenha o escopo da Fase 0 alinhado com `implementation/roadmap.md` e `implementation/phase-0/phase-0-acceptance.md`.

## Workspace e CI

Rode comandos Rust a partir de `code/`:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Esses são os comandos exatos em `.github/workflows/rust.yml`. 24 testes unitários passam, incluindo cobertura de metadata/version-gate; mantenha-os verdes.

## Layout

- `code/crates/common` — tipos de domínio compartilhados como `NodeId`, `WorkId`, `RequestId` e `NodeState`.
- `code/crates/protocol` — fronteira gRPC/Protobuf gerada.
- `code/crates/controller` — ponto de entrada do control-plane do cluster.
- `code/crates/worker` — ponto de entrada do node-agent e interação com hardware local.
- `code/crates/transport` — trait de transporte e tipos de transporte.
- `code/crates/benchmark` — medição de capability; nunca decisões de placement.
- `code/crates/cli` — CLI do operador; não coloque execução de worker nela.
- `code/proto/cluster.proto` — contrato de wire, pacote `ai.cluster.v1`.

## Convenções

- Use Rust 2024 e um crate focado por fronteira de control/data-plane.
- Mantenha tipos compartilhados em `common`; dependa deles via path dependencies locais.
- Mantenha mensagens de wire em `proto/cluster.proto`; `crates/protocol/build.rs` gera os bindings e `src/lib.rs` os expõe sob `cluster`.
- Binários existentes usam `#[tokio::main]` e retornam `Result<(), Box<dyn std::error::Error>>`.
- O histórico existente usa assuntos no estilo Conventional Commit, ex. `test(controller): ...`, `style(controller): ...` e `docs: ...`.

## Armadilhas

- O workspace Cargo é `code/`; o workflow de CI o define como diretório de trabalho para todo comando Cargo.
- Não edite a saída gerada de Protobuf em `target/`; edite `code/proto/cluster.proto`.
- O script de build do protocolo seleciona `protoc-bin-vendored`; não dependa de uma instalação de `protoc` do host.
- `.gitignore` exclui `**/target/`, `.hermes/` e `.env*` exceto `.env.example`.
