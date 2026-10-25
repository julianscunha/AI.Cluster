---
title: "ADR-005 — Camadas de armazenamento"
aliases: ["ADR-005", "ADR-005 — Storage Tiering"]
tags: [sistema/adr, subsistema/dados]
type: sistema/adr
status: decidido
implementacao: parcial
fases: ["[[fase-10-inteligencia-de-storage]]"]
---

# ADR-005 — Camadas de armazenamento

Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

Memória e armazenamento têm latências e custos muito diferentes, e o cluster precisa escolher onde cada dado vive.

## Decisão

Tratar RAM e VRAM como HOT, SSD e NVMe como WARM, e armazenamento persistente ou HDD como COLD.

Política adicional: SSD e NVMe saudáveis podem receber uso amplo de cache e swap, mantendo reserva operacional e limites de endurance.

## Consequências

Não registradas no ADR original.

## Alternativas

- Não registradas no ADR original.

> [!info] O que já existe
> Cada worker classifica o próprio armazenamento ([[f10-classes-de-armazenamento]]), e há benchmark de escrita sequencial com fsync (Fase K2 em [[fase-10-inteligencia-de-storage]]). A política de cache e swap em SSD não está implementada.

> [!todo] Política de cache e swap com limites de endurance, SMART e alocação dinâmica ficam em [[storage-intelligence]] e [[fase-10-inteligencia-de-storage]].
