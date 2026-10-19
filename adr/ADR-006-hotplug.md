---
title: "ADR-006 — Hot-Plug Recovery Model"
type: adr
status: decided
tags: [adr, hotplug]
---

# ADR-006 — Hot-Plug Recovery Model

Status: DECIDED

## Decision

Hot-plug/hot-remove será realizado por state objects, checkpoints, retries, replica promotion e replan. Não assumir migração arbitrária de memória viva.
