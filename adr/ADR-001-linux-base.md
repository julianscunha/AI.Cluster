---
title: "ADR-001 — Linux Base"
type: adr
status: proposed
tags: [adr, linux-base]
---

# ADR-001 — Linux Base

Status: PROPOSED

## Decision

Construir o AI Cluster OS sobre uma base Linux madura, não escrever um kernel/sistema inteiro do zero.

## Rationale

- drivers;
- ARM64/x86_64;
- hardware ecosystem;
- boot/recovery;
- menor custo inicial.

## Consequence

O diferencial estará no runtime, provisioning, scheduler, storage intelligence e observability, não no kernel inteiro.
