---
title: "F04 — Escalonamento"
aliases: ["Fase G — Scheduler", "EP-G", "roadmap 2 (scheduler)"]
tags: [implementacao/fase/04, implementacao/estado/implementado]
type: implementacao/fase
status: implementado
codigos-antigos: ["F3", "G1", "G2", "G3", "G4", "G5", "G6", "roadmap-2"]
sistema: ["[[scheduler]]", "[[execution-planner]]", "[[capability-engine]]", "[[saude-e-confianca]]", "[[no-e-hardware]]"]
---

# F04 — Escalonamento

## Objetivo

Decidir **qual nó** recebe cada Work Unit (scheduler global: filtro, custo, histerese, reservas) e **quanto da máquina** ela recebe (scheduler local: concessão de cores). O critério é que o placement escolha melhor que um despacho sem decisão, medido por benchmark.

## Depende de

- [[fase-02-capability-e-saude]] (capability, benchmark de compute, saúde)
- [[fase-03-execucao-distribuida]] (Work Unit e Execution Graph)

## Entrega

| Item | Status | Onde |
|---|---|---|
| Filtro de candidatos (capability, memória, estado) | [x] | [[f04-placement]] |
| Modelo de custo (compute, transferência, fila, saúde) | [x] | [[f04-placement]] |
| `placement::decide` (decisão pura, com reserva e histerese) | [x] | [[f04-placement]] |
| Scheduler global (escolha pelo custo) | [x] | [[f04-placement]] |
| Histerese de rebalanceamento (G5) | [x] | [[f04-placement]] |
| Scheduler local (concessão de cores) | [~] | [[f04-scheduler-local]] |
| Reservas de nó para requisição futura | [x] | [[f04-reservas]] |

## Checklist

Fonte: EP §8 (G1–G6), EP §7 (F3), roadmap Fase 2 e EP §8 "critério de saída".

- [~] G1 Candidate filtering: capabilities (`cpu_threads`) [x]; memória (footprint) [x]; saúde (heartbeat) [x]; trust [~] (exclusão por estado e token de join; sem identidade por nó, sem TLS; níveis UNKNOWN→TRUSTED dependem de enrollment, ver [[saude-e-confianca]]).
- [x] G2 Cost model: compute, transferência, fila e saúde. Locality ainda não modelada.
- [x] G3 Global scheduler: `WorkStore::claim_placed` rankeia candidatos ociosos pelo custo.
- [~] G4 Local scheduler: concessão de cores [x]; filas de recursos, prioridades e GPU/NPU [ ].
- [x] G5 Rebalancing (threshold + histerese): o nó só cede quando o outro é mais barato além da banda. Rebalanceamento de trabalho já em voo não existe.
- [x] G6 Reservations: `Reserve`, `ListReservations`, `ReleaseReservation`.
- [x] F3 Placement abstraction: a política de ranking saiu de `WorkStore::claim_placed` para `placement::decide`.
- [x] Critério de saída: o scheduler prova, por benchmark, placement melhor que o despacho sem decisão, em `code/tests/phase-g-placement.sh` (2.020.000us sob primeiro-que-pergunta vs 18.000us sob placement).

## Gate

Gate padrão (EP §19): código, benchmark, telemetria, teste de falha, doc e ADR.

- [x] Código: `code/crates/controller/src/placement.rs`, `work.rs`, `reservations.rs`, `service.rs`; worker `core_budget`/`grant_cores`.
- [x] Benchmark: `code/tests/phase-g-placement.sh`, `phase-g-cores.sh`.
- [x] Telemetria: decisão visível em traces e métricas de [[fase-05-observabilidade]].
- [x] Teste de falha: expiração e recusa de reserva cobertas em `work.rs` (unitário); retenção e liberação em `code/tests/phase-g-reservations.sh`.
- [x] Doc: os quatro slices desta pasta.
- [ ] ADR: nenhum ADR específico da fase identificado; confirmar.

> [!warning] Gate parcial
> G4 está parcial (sem filas de recursos nem GPU/NPU) e o trust de G1 depende de enrollment ainda não implementado. A fase fecha como [x] pelo critério de saída, mas esses dois itens são tetos registrados.

## Slices

- [[f04-placement]]: filtro duro por threads e memória, modelo de custo, `placement::decide`, scheduler global e histerese.
- [[f04-scheduler-local]]: concessão de cores pelo worker e checksum idêntico entre 1 e N threads.
- [[f04-reservas]]: retenção de um nó para uma requisição futura, com TTL e recusa a outras requisições.

## Códigos antigos

- F3 (Placement abstraction, Fase F do EP)
- G1–G6 (Fase G — Scheduler do EP)
- roadmap 2 (parte: global/local scheduler, placement)

## Specs do sistema

- [[scheduler]]
- [[execution-planner]]
- [[capability-engine]]
- [[saude-e-confianca]]
- [[no-e-hardware]]
