---
title: "09 — Execution Graph"
type: spec
status: draft
tags: [spec, execution-graph]
---

# 09 — Execution Graph

## Objetivo

Representar uma requisição como um grafo de operações com dependências explícitas.

## Model Graph vs Execution Graph

Model Graph descreve a arquitetura do modelo.

Execution Graph descreve aquela execução concreta considerando request, precision, caches, hardware e política.

## DAG

Preferir DAGs quando o workload permitir.

## Paralelismo

Operações independentes podem ser executadas em paralelo.

```text
W1 ----+
       |
W2 ----+--> SYNC --> W4
       |
W3 ----+
```

## Barreiras

Usar barreiras locais apenas quando necessário. Evitar barreiras globais.

## Versionamento

Cada Execution Graph está associado a uma versão de plano. Mudanças de placement não devem alterar o significado lógico do grafo.

## Recuperação

Se um node falhar, Work Units afetadas entram em recovery/retry e podem ser reassociadas a outro node.
