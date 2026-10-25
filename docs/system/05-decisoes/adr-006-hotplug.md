---
title: "ADR-006 — Modelo de recuperação em hot-plug"
aliases: ["ADR-006", "ADR-006 — Hot-Plug Recovery Model"]
tags: [sistema/adr, subsistema/execucao]
type: sistema/adr
status: decidido
implementacao: parcial
fases: ["[[fase-03-execucao-distribuida]]", "[[fase-06-estado-e-recuperacao]]"]
---

# ADR-006 — Modelo de recuperação em hot-plug

Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

Um nó que entra ou sai do cluster não pode levar consigo a memória viva de um processo. Migrar memória arbitrariamente é caro e frágil.

## Decisão

Hot-plug e hot-remove são tratados por state objects, checkpoints, retries, promoção de réplica e replanejamento. Não se assume migração arbitrária de memória viva.

## Consequências

Não registradas no ADR original.

## Alternativas

- Migração de memória viva: não adotada, pela decisão acima.

> [!info] O que já existe
> Entrada e saída de nós ([[fase-01-nucleo-do-cluster]]), tolerância a falha com retry e reatribuição de unidades ([[f03-tolerancia-a-falhas]]), e cópia do estado recuperada quando o volume do controller morre ([[f06-recuperacao]]).

> [!todo] Hot-plug de dispositivos (USB, aceleradores) depende de [[fase-16-aceleradores]] e [[fase-18-worker-movel]]. O modelo completo está em [[hotplug-e-recuperacao]].
