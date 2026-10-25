---
title: "F04 — Quarentena automática"
aliases: ["G1 quarentena", "quarentena de nó"]
tags: [implementacao/fase/04, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-04-escalonamento]]"
codigos-antigos: ["G1"]
sistema: ["[[saude-e-confianca]]", "[[scheduler]]"]
evidencia: ["code/tests/phase-g-quarantine.sh"]
---
# F04 — Quarentena automática

- [ ] Superado

## Objetivo

Até aqui o controller aceitava qualquer resultado de um nó aprovado. Um worker com defeito (ou malicioso) podia devolver lixo e o grafo seguia com ele. Esta fatia estende a confiança, antes só `trusted` sim/não, com um estado derivado de evidência: **resultado inválido repetido tira o nó de circulação**.

## O que foi feito

- `WorkStore::result_is_valid` confere o resultado contra a unidade despachada: mesma forma, `rows*cols` números, todos finitos e o checksum igual à soma que o worker calcularia (mesma ordem, igualdade exata).
- `submit_work_result` não registra um resultado inválido: devolve `accepted=false`, grava `work_rejected` e chama `WorkStore::reject`, que volta a unidade à fila **sem gastar** a tentativa dela (a culpa é do nó, não da unidade).
- `Registry::strike`: 3 inválidos seguidos (`QUARANTINE_STRIKES`) marcam `quarantined`. Um resultado válido zera a contagem. O nó em quarentena sai de `eligible`, o controller devolve as unidades que ele ainda segura, derruba seus canais de fatias e registra `node_quarantined`. A CLI mostra o estado `QUARANTINED`.
- Libera-se com `ai-cluster approve <nó>` (operador). Registrar de novo não cura.

## Evidência

`bash code/tests/phase-g-quarantine.sh`: o worker com `AI_CLUSTER_CORRUPT_RESULTS=1` (gancho de teste que soma 1 ao checksum) fica `QUARANTINED` sozinho no cluster; um worker honesto entra depois e as 6 unidades terminam sem falhas. Unitários: `repeated_invalid_results_quarantine_a_node_until_approved_again`, `three_invalid_results_in_a_row_quarantine_the_node`, `a_result_is_valid_only_with_the_right_shape_finite_numbers_and_checksum`, `a_rejected_unit_goes_back_to_the_queue_without_spending_its_attempts`.

## Limites

> [!warning] Só detecta o que o controller consegue conferir
> O checksum prova coerência interna do resultado, não que a multiplicação está certa: um nó que devolve uma matriz errada com o checksum da própria matriz passa. Conferência por réplica (executar em dois nós e comparar) é o passo seguinte e custa o dobro.

> [!todo] Níveis e detalhes
> A quarentena persiste junto da credencial (`AI_CLUSTER_NODES`, quarta coluna) e não some com `leave_node` nem com restart; só `approve` a libera. Os estados `UNKNOWN`, `BOOTSTRAPPED` e `VALIDATED` seguem dependendo de enrollment ([[fase-19-hardening-de-producao]]). O limite de 3 é constante, sem variável de ambiente. As unidades que o nó ainda segurava ao ser posto em quarentena voltam por `requeue_for_node` e gastam uma tentativa cada. O gancho `AI_CLUSTER_CORRUPT_RESULTS` só faz o próprio worker mentir.

## Próximos passos

- Verificação por réplica para resultados corretos na forma e errados no valor.
