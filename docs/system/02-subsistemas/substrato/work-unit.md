---
title: "Unidade de trabalho"
aliases: ["Unidade de trabalho", "Work Unit"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
---

# Unidade de trabalho

Work Unit é a menor unidade que o scheduler atribui a um nó. Hoje a unidade é uma multiplicação de matrizes: os operandos viajam no pedido e o resultado volta no relatório da unidade.

## Campos

| Campo | Função |
|---|---|
| `work_id` | Identificador, gerado pelo controller (`next_work_id`). |
| `request_id` | Liga as unidades de uma mesma requisição. `GetTrace` reconstrói o caminho por esse id. |
| `rows`, `shared`, `cols` | Dimensões da multiplicação `A(rows × shared) × B(shared × cols)`. |
| `matrix_a`, `matrix_b` | Operandos, em `repeated double` (8 bytes por elemento). |
| `depends_on` | Unidades que precisam completar antes. |
| `matrix_a_from`, `matrix_b_from` | Entrada vinculada: o operando vem da saída de outra unidade. Vazio usa o operando enviado. |
| `min_cpu_threads` | Requisito mínimo de threads do nó. `0` significa sem requisito. |
| `priority` | Inteiro de 0 a 9. Maior sai primeiro. |

Uma unidade bloqueada (com dependência ou entrada vinculada ainda não disponível) não é atribuída. Uma entrada vinculada a um identificador que nunca completa deixa a unidade bloqueada, sem despachar com operando vazio.

## Requisitos de atribuição

Um nó recebe a unidade só quando tem:

- threads reportadas em número igual ou maior que `min_cpu_threads`;
- memória reportada suficiente para os operandos e o resultado (A e B lidos, C escrito, 8 bytes por elemento). Memória zero significa "não reportada" e não filtra;
- confiança e estado elegíveis (ver [[saude-e-confianca]]).

Nenhum outro requisito (precisão, acelerador, armazenamento, localidade, prazo) existe hoje.

## Ciclo de vida

```text
na fila --(claim)--> em voo com o nó --(complete)--> concluída
   ^                         |
   +----(nó saiu/caiu)------+
                             \--(orçamento esgotado)--> falhou (com dependentes)
```

- **Atribuição (`claim`).** Ao ser atribuída, a unidade conta uma tentativa. Quem escolhe o nó é o placement (ver [[scheduler]]).
- **Conclusão (`complete`).** Só o nó que recebeu a unidade pode concluí-la. Um resultado atrasado de nó que perdeu a posse é recusado.
- **Devolução.** Ao sair ou cair um nó, suas unidades em voo voltam à fila com dependências e entradas vinculadas intactas.
- **Falha.** Ao esgotar as tentativas, a unidade falha, e todos os dependentes dela, em cascata. Nada fica esperando para sempre.

## Tentativas

Cada unidade tem no máximo **três tentativas**: a primeira e duas reatribuições (`MAX_ATTEMPTS = 3`). Uma unidade que completa zera o contador. Uma unidade rejeitada pela validação de resultado (ver [[saude-e-confianca]]) não gasta tentativa.

O limite é o mesmo para todas as unidades. O sistema não distingue unidade idempotente de não idempotente.

## Resultado

O resultado traz o `checksum` (soma dos elementos, na mesma ordem que o worker calcula) e `compute_time_us`. Os resultados intermediários ficam no controller enquanto alguma unidade não concluída os lê. Depois, são descartados. O controller guarda até 64 resultados finais para consulta (`GetOutput`).

## Tamanho

Cada mensagem gRPC tem teto de 64 MiB (ver [[camada-de-transporte]]). Com A e B na mesma mensagem, 8 bytes por elemento, a unidade quadrada cabe até cerca de 2048 × 2048. Acima disso, a unidade não chega ao worker.

A fila do controller recusa novos grafos quando há mais de 256 unidades pendentes (na fila ou em voo).

## Limites conhecidos

- Os operandos viajam inline em `double` por elemento: tamanho e custo de CPU crescem além do necessário. Não há buffer binário.
- Prioridade é um inteiro sem classes nomeadas. Hoje só o papel de operador enfileira unidades.
- A reatribuição não evita o nó que já falhou a unidade. Com um só nó sobrevivente, a unidade volta para ele.
- Não há sinal de falha vindo do worker. Um worker que não consegue executar uma unidade a descarta em silêncio, e a unidade fica presa a ele até o nó sair.
- Não há reserva de memória por unidade no nó, só o filtro de atribuição.

> Planejado: campos de precisão, prazo (`deadline_ms`), tipo de operação genérico, flag de idempotência, requisitos de memória e localidade, estados nomeados (`WAITING`, `RETRYING`, `CANCELLED`, `RECOVERY`), execução especulativa e falha explícita reportada pelo worker.
