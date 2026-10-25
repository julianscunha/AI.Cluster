---
title: "F00 — Estrutura do repositório"
aliases: ["phase-0/repository-structure", "A1", "Repositório"]
tags: [implementacao/fase/00, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-00-fundacao]]"
codigos-antigos: ["A1", "phase-0/repository-structure"]
sistema: ["[[arquitetura]]"]
evidencia: ["code/Cargo.toml", "code/crates/"]
---
# F00 — Estrutura do repositório

- [ ] Superado

## Objetivo

Separar desde o início o plano de controle (controller) do plano de dados (worker), com fronteiras de crate que impeçam dependências cruzadas. É o critério A1 da [[fase-00-fundacao]].

## O que foi feito

Workspace Rust em `code/`, com build reproduzível, CI e harness de testes unitários. A árvore abaixo é a real (verificada em `code/`); em relação à árvore planejada há duas crates a mais (`engine`, `gateway`) e nenhum `config/` na raiz do workspace.

```text
code/
├── Cargo.toml
├── crates/
│   ├── benchmark/   # mede capability; não decide placement
│   ├── cli/         # interface de operador
│   ├── common/      # tipos de domínio compartilhados
│   ├── controller/  # dono do estado e das decisões de escalonamento
│   ├── engine/      # engine de inferência (F08)
│   ├── gateway/     # gateway HTTP de inferência (F08)
│   ├── protocol/    # contratos de rede e tipos RPC gerados
│   ├── transport/   # abstração da comunicação física
│   └── worker/      # execução local e interação com o hardware
├── proto/           # cluster.proto
├── os/
├── models/
├── scripts/
└── tests/           # evidências E2E (*.sh)
```

Três binários independentes: `ai-cluster-controller`, `ai-cluster-worker` e `ai-cluster` (CLI). O protocolo é gerado a partir de `proto/cluster.proto` pelo `build.rs` do crate `protocol`, com `protoc` vendorizado (ver [[adr-008-stack-inicial]]).

## Decisões

- Fronteiras de crate como regra de arquitetura, não convenção: `common` só tipos de domínio; `protocol` só contratos; `cli` nunca executa lógica de worker diretamente; `benchmark` mede e não decide placement.
- Nenhum código de aplicação ou agente pertence ao compute plane (ver [[worker-runtime]]).

## Evidência

- `cargo build` a partir de checkout limpo: CI verde (gate A1 do aceite da Fase 0).
- Três binários com nomes fixos (`ai-cluster-controller`, `ai-cluster-worker`, `ai-cluster`).

## Limites

- A separação é por convenção de dependência entre crates; não há teste automático que impeça um import indevido.

## Próximos passos

- Nenhum bloqueante. Se `config/` virar necessário (overrides por arquivo em vez de variáveis `AI_CLUSTER_*`), criá-lo com uma fatia própria.
