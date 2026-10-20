---
title: "ADR-004 — Graph as Projection"
type: adr
status: proposed
tags: [adr, graph-as-projection]
---

# ADR-004 — Graph as Projection

Status: PROPOSED

## Decisão

Usar o grafo como camada de relações/traversal/index, sem obrigar o grafo a ser a única fonte de verdade.

## Justificativa

- rebuild possível;
- queries relacionais complexas;
- topology + knowledge + execution;
- menor risco operacional.
