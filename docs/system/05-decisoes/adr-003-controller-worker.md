---
title: "ADR-003 — Controller e Worker"
aliases: ["ADR-003", "ADR-003 — Controller / Worker"]
tags: [sistema/adr, subsistema/execucao]
type: sistema/adr
---

# ADR-003 — Controller e Worker

Status: decidido · Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

O cluster reúne hardware heterogêneo e precisa de uma visão única para planejar e distribuir trabalho, sem perder a execução local eficiente em cada nó.

## Decisão

Separar o control plane do cluster (o controller) do Worker Runtime ([[worker-runtime]]).

## Consequências

- Permite hardware heterogêneo, scheduler central e execução local otimizada.
- Controller e worker são binários separados que falam gRPC ([[protocolo-controller-worker]]). O registro, a aprovação e o trabalho passam pelo controller.

## Alternativas

- Nenhuma alternativa registrada.

## Limites

- Não há controller em alta disponibilidade. Uma cópia do estado num nó cobre a perda do volume do controller ([[adr-013-estado-do-controller-e-recuperacao]]).
