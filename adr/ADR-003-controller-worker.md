---
title: "ADR-003 — Controller / Worker"
type: adr
status: decided
tags: [adr, controller-worker]
---

# ADR-003 — Controller / Worker

Status: DECIDED

## Decision

Separar o cluster control plane do Worker Runtime.

## Rationale

Permite hardware heterogêneo, scheduler central e execução local otimizada.
