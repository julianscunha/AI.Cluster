# 09 — Execution Graph

## Objective

Representar uma requisição como um grafo de operações com dependências explícitas.

## Model Graph vs Execution Graph

Model Graph descreve a arquitetura do modelo.

Execution Graph descreve aquela execução concreta considerando request, precision, caches, hardware e política.

## DAG

Preferir DAGs quando o workload permitir.

## Parallelism

Operações independentes podem ser executadas em paralelo.

```text
W1 ----+
       |
W2 ----+--> SYNC --> W4
       |
W3 ----+
```

## Barriers

Usar barreiras locais apenas quando necessário. Evitar barreiras globais.

## Versioning

Cada Execution Graph está associado a uma versão de plano. Mudanças de placement não devem alterar o significado lógico do grafo.

## Recovery

Se um node falhar, Work Units afetadas entram em recovery/retry e podem ser reassociadas a outro node.
