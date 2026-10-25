---
title: "F01 — Núcleo do cluster"
aliases: ["phase-1", "Fase 1", "Fase 1 — Cluster mínimo", "roadmap 1", "Fase B", "EP B"]
tags: [implementacao/fase/01, implementacao/estado/implementado]
type: implementacao/fase
status: implementado
---
# F01 — Núcleo do cluster

## Objetivo

Colocar um cluster mínimo de pé: estado autoritativo do controller, registro de nós, worker agent, entrada e saída de nós e CLI de operador. Ao fim, nós entram e saem sem operação manual.

## Depende de

[[fase-00-fundacao]] (repositório, protocolo, tipos comuns).

## Entrega

- Controller com estado em memória e persistência parcial.
- Node registry com entrada, atualização, saída e lifecycle.
- Worker agent: daemon Linux mínimo com heartbeat, capability report e Work Unit runner.
- CLI: `ai-cluster status`, `nodes`, `node show <id>`, `events`.
- Saída planejada de nó com requeue de trabalho em voo ([[f01-entrada-e-saida-de-nos]]).

## Checklist

Derivado de EP §3 (B1–B4), roadmap Fase 1 e pacotes de trabalho da Fase 1 em `phases.md`.

**Controller core**

- [x] Estado do cluster em memória (B1).
- [~] Persistência (B1): snapshot da fila e dos outputs; registry e eventos não sobrevivem a restart. Ver [[f06-persistencia-e-checkpoint]].
- [ ] Event bus: hoje é um ring buffer de eventos (`events.rs`), não um bus.
- [x] Node registry (B2).
- [x] Scheduler interface: o dispatch escolhe por custo (`placement`); `AI_CLUSTER_PLACEMENT=off` volta ao primeiro-que-pergunta.

**Worker core**

- [x] Heartbeat, desacoplado do laço de trabalho (commit `b219ec6`).
- [x] Capability report (B3): threads, RAM, compute medido no registro ([[f02-benchmark-de-capability]]).
- [x] CPU benchmark.
- [x] Work Unit runner.

**CLI (B4)**

- [x] `status`.
- [x] `nodes`.
- [x] `node show <id>`: capability e saúde completas e a reserva do nó. Evidência: `code/tests/phase-b4-node-show.sh`.
- [x] `events`.

**Roadmap Fase 1**

- [x] Controller, worker, registration, heartbeat, capabilities, health, CLI status.
- [x] Health: liveness por heartbeat e score de saúde com pressão de memória, swap e térmica ([[f02-modelo-de-saude]]).

## Gate

Critério de saída: **nós entram e saem com segurança**.

- [x] Entrada e saída seguras: `code/tests/phase-1-leave.sh` e [[f01-entrada-e-saida-de-nos]].
- [~] Cluster de 2–4 máquinas funciona sem operação manual contínua: validado com contêineres; 2–4 máquinas físicas pendentes.

Gate padrão de fase: código ✔ · benchmark ✔ (capability) · telemetria ✔ (`events`, `metrics`) · teste de falha ✔ (saída graciosa com trabalho em voo) · documentação ✔ · ADR: a arquitetura controller-worker está em [[adr-003-controller-worker]].

> [!todo] Pendente
> Validação em máquinas físicas (ver [[f00-runbook-duas-maquinas]]) e event bus de verdade.

## Slices

| slice | conteúdo | estado |
|---|---|---|
| [[f01-entrada-e-saida-de-nos]] | `LeaveNode`, drenagem, requeue de trabalho em voo | implementado |

## Códigos antigos

- EP B1, B2, B3, B4 (Fase B — Cluster Core).
- `roadmap 1`.
- `phase-1/join-and-leave`.
- Pacotes de trabalho da Fase 1 (`phases.md`).

## Specs do sistema

- [[hotplug-e-recuperacao]]
- [[protocolo-controller-worker]]
- [[no-e-hardware]]
- [[worker-runtime]]
- [[adr-003-controller-worker]]
