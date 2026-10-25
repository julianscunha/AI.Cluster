---
title: "Estratégia de testes"
aliases: ["Testing strategy", "implementation/testing-strategy"]
tags: [implementacao/transversal, implementacao/testes]
type: implementacao/transversal
status: em-andamento
---

# Estratégia de testes

Esta nota define as camadas e os cenários que o projeto precisa cobrir e registra o que a suíte real já cobre. A prova de cada script está em [[evidencias-e2e]]; o estado de cada fase está em [[status]].

## Suíte atual

| Camada | Onde | Como roda |
|---|---|---|
| Unitária e de integração Rust | `code/crates/*` (~211 atributos `#[test]`/`#[tokio::test]` por contagem de grep) | `cargo test --workspace --all-features` |
| Paridade de modelo | `code/crates/engine/tests/gpt2_parity.rs`, `smollm2_parity.rs`, `smollm2_chat_parity.rs` (com `fixtures/`) | parte do `cargo test` |
| E2E de cluster | `code/tests/*.sh` (30 scripts, subindo controller e workers reais) | `bash code/tests/<script>.sh`, a partir de `code/` |
| Qualidade de código | CI em `.github/workflows/rust.yml` | `cargo fmt --check`, `cargo check --all-targets`, `clippy -D warnings`, `cargo test` |

Scripts de inferência (`phase-j-*`) precisam dos pesos baixados em `code/models/` (não versionados). Os demais sobem o cluster sozinhos.

## Camadas de teste

| # | Camada | Estado | Evidência |
|---|---|---|---|
| 1 | Testes unitários | `[x]` | `cargo test` nos crates |
| 2 | Compatibilidade de protocolo | `[ ]` | sem suíte dedicada; o schema é compartilhado via `code/proto` |
| 3 | Integração | `[x]` | scripts `code/tests/*.sh` |
| 4 | Injeção de falhas | `[~]` | morte de worker, controller interrompido com unidade em voo, perda do volume de estado; sem injeção de latência ou perda de rede |
| 5 | Benchmarks de desempenho | `[~]` | `scaling-sweep.sh`, `phase-c-benchmark.sh`, `phase-h-predictions.sh`; sem matriz completa |
| 6 | Soak | `[ ]` | não existe ([[fase-19-hardening-de-producao]]) |
| 7 | Recuperação | `[x]` | `phase-i4-recovery.sh`, `phase-i4-replica.sh` |

## Cenários obrigatórios

| Cenário | Cobertura | Evidência |
|---|---|---|
| Entrada de nó: entra, recebe configuração e benchmark | `[x]` | `phase-1-leave.sh`, `phase-c-benchmark.sh`, `phase-c-current.sh` |
| Drenagem de nó com requisições ativas | `[~]` | `phase-1-leave.sh` (saída planejada com SIGTERM); sem drenagem de unidade em voo |
| Falha brusca de nó | `[x]` | `phase-2-failure.sh`, `phase-i4-recovery.sh` |
| Degradação térmica: scheduler muda o peso | `[x]` | `phase-c4-health.sh` |
| Degradação de rede: replanejamento ciente do transporte | `[ ]` | sem E2E que degrade o link; a sonda atual existe ([[f02-descoberta-de-rede]]) |
| Degradação de storage: política muda | `[ ]` | depende de [[fase-10-inteligencia-de-storage]] |
| Acurácia de predição: previsto vs real | `[x]` | `phase-h-predictions.sh`, `phase-h-traces.sh` |
| Otimização de tokens | `[ ]` | depende de [[fase-14-edge-optimizer]] |

## Métricas de escala

Nunca alegar escala linear sem medição. O primeiro experimento compara 1, 2 e 3 workers em tamanhos de 32 a 256 (`code/tests/scaling-sweep.sh`, ver [[f03-varredura-compute-vs-transferencia]]): o compute cresce com o cubo da aresta e a transferência com o quadrado, e o cruzamento fica acima de 256 com o teto de mensagem atual do gRPC.

Comparar sempre:

```text
N nós
vs
throughput
vs
latência de requisição única
vs
overhead de comunicação
```

## Matriz de benchmark

Mínimo exigido, com o que existe hoje:

| Condição | Estado |
|---|---|
| mesmo hardware / mesma rede | `[~]` só containers no mesmo host; máquinas físicas pendentes ([[f00-runbook-duas-maquinas]]) |
| hardware heterogêneo / mesma rede | `[ ]` |
| mesmo hardware / rede degradada | `[ ]` |
| só CPU | `[x]` (único modo implementado) |
| CPU + acelerador | `[ ]` ([[fase-16-aceleradores]]) |
| só RAM vs assistido por SSD | `[ ]` |
| cache habilitado vs desabilitado | `[ ]` ([[fase-14-edge-optimizer]]) |
| recuperação por grafo vs recuperação base | `[ ]` ([[fase-13-graph-engine]]) |
