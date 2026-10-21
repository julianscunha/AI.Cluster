---
title: "08 — Work Unit"
type: spec
status: draft
tags: [spec, work-unit]
---

# 08 — Unidade de trabalho

## Definição

Work Unit é a menor unidade executável que o scheduler pode atribuir a um recurso.

## Campos obrigatórios

```json
{
  "work_id": "wu-...",
  "execution_id": "exec-...",
  "plan_id": "plan-...",
  "type": "MATMUL",
  "precision": "INT4",
  "dependencies": [],
  "requirements": {},
  "priority": "INTERACTIVE",
  "deadline_ms": 80,
  "retryable": true
}
```

## Requisitos

Podem incluir:

- minimum memory;
- precision;
- accelerator requirement;
- latency limit;
- compute capability;
- locality requirement.

## Ciclo de vida

CREATED → READY → QUEUED → RUNNING → COMPLETED.

Exceções:

WAITING, RETRYING, FAILED, CANCELLED, RECOVERY.

## Idempotência

Work Units devem indicar se são recomputáveis/idempotentes. Isso permite retry e speculative execution de forma controlada.
