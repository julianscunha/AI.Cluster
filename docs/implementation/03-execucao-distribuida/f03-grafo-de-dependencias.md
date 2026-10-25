---
title: "F03 — Grafo de dependências"
aliases: ["phase-2/dependency-graph", "F1", "F2"]
tags: [implementacao/fase/03, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-03-execucao-distribuida]]"
codigos-antigos: ["F1", "F2", "roadmap-2"]
sistema: ["[[execution-graph]]", "[[work-unit]]"]
evidencia: ["code/tests/phase-2-graph.sh"]
---
# F03 — Grafo de dependências

- [ ] Superado

## Objetivo

Unidades carregam `depends_on`, e o output de uma unidade vira input de outra. O controller só despacha uma unidade depois que suas dependências completaram, substituindo os outputs delas nos inputs vinculados.

## O que foi feito

- `WorkUnit.depends_on` em `code/proto/cluster.proto`: ids que precisam completar antes.
- `WorkUnit.matrix_a_from` e `matrix_b_from`: no dispatch, esses inputs são substituídos pelo output do work id nomeado. Vazio significa usar o payload semeado.
- RPC `EnqueueGraph`: enfileira um diamante `A -> (B, C) -> D` com bindings reais.
- `WorkStore::claim` varre a fila atrás da primeira unidade pronta, para que uma unidade bloqueada na cabeça não pare as de trás, e então preenche os inputs vinculados a partir dos outputs das dependências.
- Outputs concluídos ficam num `HashMap<work_id, Vec<f64>>` do lado do controller. Volátil, não persistido.
- O requeue mantém dependências e bindings: uma unidade re-enfileirada continua bloqueada até suas dependências.
- CLI: `ai-cluster graph --size N` enfileira o diamante e faz poll até drenar.

## Decisões

- **Semântica de prontidão.** Uma unidade é despachável quando todo id em `depends_on` completou **e** todo input vinculado está disponível. Um binding para um id que nunca completa deixa a unidade bloqueada, em vez de despachar com payload vazio.
- **Resultado intermediário passa pelo controller.** O worker devolve a matriz de output no resultado; o controller a guarda e a injeta no input da próxima unidade. Um resultado cruza a rede duas vezes (worker → controller → próximo worker). Um caminho worker-a-worker economizaria um hop; fica para quando a transferência dominar.

## Evidência

Reproduzir com `bash code/tests/phase-2-graph.sh`. Dois workers (`node-b`, `node-c`) registrados; `graph --size 128` reportou `grafo enfileirado=4 bloqueados=3` (A pronta, B/C/D bloqueadas) e depois drenou até `pendentes=0`:

| trabalho | nó | compute_us | transfer_us | total_us | checksum |
|---|---|---|---|---|---|
| w-0 (A) | node-c | 804 | 9,196 | 10,000 | 476.977911 |
| w-1 (B) | node-b | 1,117 | 3,883 | 5,000 | 150.229166 |
| w-2 (C) | node-c | 1,150 | 3,850 | 5,000 | -3293.334375 |
| w-3 (D) | node-b | 915 | 3,085 | 4,000 | -218950.430782 |

Os checksums **diferem**, o que prova o fluxo de dados: B e C consomem o output de A, e D consome os de B e C. Antes dos bindings, toda unidade computava o mesmo produto semeado e os quatro checksums eram idênticos.

Cobertura unitária: `blocked_units_are_not_claimed_until_dependencies_complete`, `claim_skips_blocked_units_to_reach_a_ready_one`, `requeue_keeps_dependencies_intact`, `dependent_unit_receives_its_dependency_output_as_input`, `a_binding_to_an_unknown_work_id_keeps_the_unit_blocked` e `enqueue_graph_builds_a_diamond_with_blocked_branches`.

## Limites

> [!warning] Outputs retidos por toda a vida do processo
> A memória cresce com o trabalho concluído. Dá para liberar um output quando todos os dependentes forem reivindicados, ou limitar o histórico, quando controllers de longa duração começarem a importar.

> [!warning] Latência de dispatch limitada pelo heartbeat
> A latência de dispatch ainda é limitada pelo tick de heartbeat de cinco segundos: uma unidade é reivindicada até cinco segundos depois de ficar pronta. Aceitável para correção; revisitar quando o throughput importar.

- O tratamento de falhas (retry, cascata) está em [[f03-tolerancia-a-falhas]].

## Próximos passos

- Limpar outputs consumidos ou limitar o histórico em controllers de longa duração.
- Caminho worker-a-worker para resultados intermediários, se a transferência dominar.
