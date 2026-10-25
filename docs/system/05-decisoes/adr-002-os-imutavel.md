---
title: "ADR-002 — Immutable AI Cluster OS"
type: adr
status: proposed
tags: [adr, immutable-os]
---

# ADR-002 — Immutable AI Cluster OS

Status: PROPOSED

## Decisão

Usar imagem versionada/imutável, com reimage inicial e update A/B.

## Justificativa

Evita configuration drift e facilita rollback/recovery.
