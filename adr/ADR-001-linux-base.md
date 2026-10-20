---
title: "ADR-001 — Linux Base"
type: adr
status: proposed
tags: [adr, linux-base]
---

# ADR-001 — Linux Base

Status: PROPOSED

## Decisão

Construir o AI Cluster OS sobre uma base Linux madura, não escrever um kernel/sistema inteiro do zero.

## Justificativa

- drivers;
- ARM64/x86_64;
- hardware ecosystem;
- boot/recovery;
- menor custo inicial.

## Consequência

O diferencial estará no runtime, provisioning, scheduler, storage intelligence e observability, não no kernel inteiro.
