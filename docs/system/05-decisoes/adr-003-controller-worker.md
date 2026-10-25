---
title: "ADR-003 — Controller e Worker"
aliases: ["ADR-003", "ADR-003 — Controller / Worker"]
tags: [sistema/adr, subsistema/execucao]
type: sistema/adr
status: decidido
implementacao: implementado
fases: ["[[fase-01-nucleo-do-cluster]]"]
---

# ADR-003 — Controller e Worker

Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

O cluster reúne hardware heterogêneo e precisa de uma visão única para planejar e distribuir trabalho, sem perder a execução local eficiente em cada nó.

## Decisão

Separar o control plane do cluster (o controller) do Worker Runtime ([[worker-runtime]]).

## Consequências

Permite hardware heterogêneo, scheduler central e execução local otimizada.

## Alternativas

- Não registradas no ADR original.

> [!info] O que já existe
> `code/crates/controller` e `code/crates/worker` são binários separados que falam gRPC ([[protocolo-controller-worker]]). O registro, a aprovação e o trabalho passam pelo controller ([[fase-01-nucleo-do-cluster]]).

> [!todo] Não há controller em alta disponibilidade. A cópia do estado num nó cobre a perda do volume do controller ([[f06-recuperacao]]); a recuperação completa está em [[fase-06-estado-e-recuperacao]].
