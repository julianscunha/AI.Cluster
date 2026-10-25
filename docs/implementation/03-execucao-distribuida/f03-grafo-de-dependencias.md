---
title: "Fase 2 — Grafo de dependências"
type: implementation
status: implemented
tags: [implementation, phase-2, execution-graph]
---

# Fase 2 — Grafo de dependências

A fatia do Execution Graph: unidades carregam `depends_on`, e o output de uma unidade
vira input de outra. O controller só despacha uma unidade depois que suas dependências
completaram, substituindo os outputs delas nos inputs vinculados.

## O que mudou

- `WorkUnit.depends_on` em `code/proto/cluster.proto`: ids que precisam completar primeiro.
- `WorkUnit.matrix_a_from` / `matrix_b_from`: no dispatch esses inputs são substituídos
  pelo output do work id nomeado. Vazio significa usar o payload semeado como está.
- RPC `EnqueueGraph`: enfileira um diamante `A -> (B, C) -> D` com bindings reais.
- `WorkStore::claim` varre a fila atrás da primeira unidade pronta, para que uma unidade
  bloqueada na cabeça não pare as que estão atrás dela, e então preenche os inputs
  vinculados a partir dos outputs das dependências.
- O `WorkStore` guarda outputs concluídos num `HashMap<work_id, Vec<f64>>` — a área de
  retenção, do lado do controller, para resultados intermediários. Volátil, não persistida.
- O requeue mantém dependências e bindings intactos: uma unidade re-enfileirada continua
  bloqueada até suas dependências.
- CLI: `ai-cluster graph --size N` enfileira o diamante e faz poll até drenar.

## Semântica

Uma unidade é despachável quando todo id em seu `depends_on` completou **e** todo input
vinculado está disponível. A checagem de binding é defensiva: grafos construídos pelo
controller sempre vinculam a dependências declaradas, mas uma unidade vinculada a um id
que nunca completa fica bloqueada em vez de despachar com payload vazio.

Os resultados intermediários vivem na memória do controller. Um worker devolve sua matriz
de output no resultado; o controller a guarda e a injeta no input da próxima unidade
quando ela é reivindicada. Então um resultado cruza a rede duas vezes — worker para
controller, controller para o próximo worker. Um caminho worker-a-worker economizaria um
hop; adicione quando a transferência dominar.

## Evidência — Docker, dois workers, diamante com fluxo de dados

Reproduza com `bash code/tests/phase-2-graph.sh`.

`node-b` e `node-c` registrados; `graph --size 128` reportou `grafo enfileirado=4 bloqueados=3`
(A pronta, B/C/D bloqueadas), depois drenou até `pendentes=0`:

| trabalho | nó | compute_us | transfer_us | total_us | checksum |
|---|---|---|---|---|---|
| w-0 (A) | node-c | 804 | 9,196 | 10,000 | 476.977911 |
| w-1 (B) | node-b | 1,117 | 3,883 | 5,000 | 150.229166 |
| w-2 (C) | node-c | 1,150 | 3,850 | 5,000 | -3293.334375 |
| w-3 (D) | node-b | 915 | 3,085 | 4,000 | -218950.430782 |

Os checksums **diferem**, o que é a prova do fluxo de dados: B e C consomem o output de A,
e D consome os outputs de B e C. Antes dos bindings existirem, toda unidade computava o
mesmo produto semeado e os quatro checksums eram idênticos.

O gating e o binding são cobertos pelos testes unitários
`blocked_units_are_not_claimed_until_dependencies_complete`,
`claim_skips_blocked_units_to_reach_a_ready_one`,
`requeue_keeps_dependencies_intact`,
`dependent_unit_receives_its_dependency_output_as_input`,
`a_binding_to_an_unknown_work_id_keeps_the_unit_blocked` e
`enqueue_graph_builds_a_diamond_with_blocked_branches`.

## Teto conhecido

**O tratamento de falhas vive na sua própria fatia.** A reatribuição agora é limitada: uma
unidade cujo worker morre é devolvida enquanto seu orçamento de retry dura e falha passado
ele, com seus dependentes falhando junto. Ver
[Tolerância a falhas](fault-tolerance.md). Os tetos que permanecem — um worker que
não consegue fazer heartbeat enquanto computa, e um worker que descarta uma unidade
inexequível em silêncio — estão registrados lá.

**Os outputs são retidos por toda a vida do processo.** A memória cresce com o trabalho
concluído; pode limpar um output assim que todos os seus dependentes forem reivindicados,
ou limitar o histórico, quando controllers de longa duração começarem a importar.

A latência de dispatch ainda é limitada pelo tick de heartbeat de cinco segundos, então uma
unidade é reivindicada até cinco segundos depois de ficar pronta. Ok para correção; revisitar
quando throughput importar.
