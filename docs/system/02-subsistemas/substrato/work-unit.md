---
title: "Unidade de trabalho"
aliases: ["Unidade de trabalho", "Work Unit", "docs/08", "08 — Work Unit", "08 — Unidade de trabalho"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-03-execucao-distribuida]]"]
---

# Unidade de trabalho

Work Unit é a menor unidade executável que o scheduler pode atribuir a um recurso. Tudo o que o cluster executa (uma multiplicação, uma fatia de modelo, uma etapa de um grafo) chega ao worker como Work Unit.

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

| Campo | Função |
|---|---|
| `work_id`, `execution_id`, `plan_id` | Identificam a unidade, a execução e o plano a que pertence. |
| `type`, `precision` | Operação e precisão numérica. |
| `dependencies` | Unidades que precisam terminar antes. |
| `requirements` | Restrições de atribuição (ver abaixo). |
| `priority` | Classe de prioridade ([[worker-runtime]]). |
| `deadline_ms` | Prazo a partir da atribuição. |
| `retryable` | Se pode ser reexecutada após falha. |

## Requisitos

Podem incluir:

- memória mínima;
- precisão;
- requisito de acelerador;
- limite de latência;
- capacidade de compute;
- requisito de localidade.

## Ciclo de vida

`CREATED` → `READY` → `QUEUED` → `RUNNING` → `COMPLETED`.

Exceções: `WAITING`, `RETRYING`, `FAILED`, `CANCELLED`, `RECOVERY`.

## Idempotência

Work Units indicam se são recomputáveis ou idempotentes. Isso permite retry e execução especulativa de forma controlada.

> [!info] O que já existe
> **Unidade atual** (`WorkUnit` em `code/proto/cluster.proto`): é uma multiplicação de matrizes, não a unidade genérica do alvo.
> - Identificador: `work_id`, gerado pelo controller (`next_work_id` em `code/crates/controller/src/work.rs`).
> - Operandos: `rows`, `shared`, `cols`, `matrix_a`, `matrix_b` (`repeated double`).
> - Dependências: `depends_on`; a entrada pode vir da saída de outra unidade (`matrix_a_from`, `matrix_b_from`). Unidade bloqueada não é atribuída até suas dependências completarem.
> - Requisito: `min_cpu_threads` (o controller não entrega a um nó com menos threads).
> - Rastreio: `request_id` liga as unidades de uma mesma requisição (H3); `GetTrace` as reconstrói.
>
> **Ciclo real**: fila → atribuída a um nó (`claim`, que escolhe o nó de menor custo previsto) → concluída (`complete`), ou devolvida à fila quando o nó some (`requeue_for_node`). Uma unidade tem no máximo 3 tentativas de despacho (a primeira mais duas reatribuições, `MAX_ATTEMPTS`); esgotada, falha e leva junto as dependentes (`fail_cascade`). Resultado vem com `checksum` e `compute_time_us` ([[f03-tolerancia-a-falhas]], [[f03-grafo-de-dependencias]]).
>
> **Reservas**: uma requisição pode reservar nós (`Reserve`, `ReleaseReservation`); a reserva expira ([[f04-reservas]]).
>
> Saídas intermediárias ficam em memória até nenhuma unidade pendente lê-las (`drop_unreferenced_outputs`), com teto de resultados mantidos (`MAX_KEPT_RESULTS`).

> [!todo] Campos `execution_id`, `plan_id`, `type`, `precision`, `priority`, `deadline_ms`, `retryable`, `requirements` com memória e localidade; estados `WAITING`, `RETRYING`, `CANCELLED`, `RECOVERY` como estados nomeados (hoje são inferidos pelas filas); execução especulativa; prazo aplicado no controller.

> [!warning] Divergência: o `WorkUnit` atual carrega os operandos inline em `repeated double` e não tem prioridade, prazo, precisão ou flag de retry. A tentativa de reexecução é controlada só pelo limite fixo de três despachos, sem distinguir unidade idempotente de não idempotente. O alvo descreve isso como campo da unidade. Nenhum slice planeja a migração ainda; ela precisa entrar em [[fase-03-execucao-distribuida]] antes de um workload que não seja matmul.
