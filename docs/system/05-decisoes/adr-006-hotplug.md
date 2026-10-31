---
title: "ADR-006 — Modelo de recuperação em hot-plug"
aliases: ["ADR-006", "ADR-006 — Hot-Plug Recovery Model"]
tags: [sistema/adr, subsistema/execucao]
type: sistema/adr
---

# ADR-006 — Modelo de recuperação em hot-plug

Status: decidido · Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

Um worker que entra ou sai do cluster não pode levar consigo a memória viva de um processo. Migrar memória arbitrariamente é caro e frágil.

## Decisão

Hot-plug e hot-remove são tratados por state objects, checkpoints, retries, promoção de réplica e replanejamento. Não se assume migração arbitrária de memória viva.

## Consequências

- Entrada e saída de workers funcionam. Na falha de um worker, as unidades dele voltam à fila e o grafo segue ([[worker-runtime]]).
- Se o volume do controller morre, a cópia do estado num worker é recuperada ([[adr-013-estado-do-controller-e-recuperacao]]).

## Alternativas

- Migração de memória viva: não adotada, pela decisão acima.

## Limites

- A recuperação do estado por chave (KV cache, estado de modelo) não existe.
- Hot-plug de dispositivos (USB, aceleradores) é planejado. O modelo completo está em [[hotplug-e-recuperacao]].
