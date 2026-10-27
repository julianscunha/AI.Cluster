---
title: "ADR-005 — Camadas de armazenamento"
aliases: ["ADR-005", "ADR-005 — Storage Tiering"]
tags: [sistema/adr, subsistema/dados]
type: sistema/adr
---

# ADR-005 — Camadas de armazenamento

Status: decidido · Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

Memória e armazenamento têm latências e custos muito diferentes, e o cluster precisa escolher onde cada dado vive.

## Decisão

Tratar RAM e VRAM como HOT, SSD e NVMe como WARM, e armazenamento persistente ou HDD como COLD.

Política adicional: SSD e NVMe saudáveis podem receber uso amplo de cache e swap, mantendo reserva operacional e limites de endurance.

## Consequências

- Cada worker classifica o próprio armazenamento a partir de `/sys/block`: NVMe e SSD como WARM, HDD como COLD ([[storage-intelligence]]).

## Alternativas

- Nenhuma alternativa registrada.

## Limites

- A política de cache e swap em SSD e NVMe não existe. Também não existem leitura de SMART e alocação dinâmica entre HOT, WARM e COLD ([[storage-intelligence]]).
