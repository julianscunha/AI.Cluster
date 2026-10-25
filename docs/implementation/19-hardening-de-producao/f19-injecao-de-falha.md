---
title: "F19 — Injeção de falha e soak"
aliases: ["soak", "chaos"]
tags: [implementacao/fase/19, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-19-hardening-de-producao]]"
sistema: ["[[observabilidade]]"]
evidencia: ["code/tests/phase-t-soak.sh"]
---
# F19 — Injeção de falha e soak

- [ ] Superado

## Objetivo

Falhas repetidas e alternadas, com carga em andamento, sem perder nem duplicar unidade e sem o cluster encolher.

## O que existe

- `phase-t-soak.sh`: controller e três workers; a cada rodada um lote corre enquanto um worker, em rodízio, sofre `kill` (SIGKILL) ou `pause` (congelado além da graça do heartbeat e retomado, o que gera resultado tardio de unidade já reenfileirada).
- Verifica por rodada: o lote fecha (o resumo da CLI é cumulativo, então a contagem é `UNITS * rodada`), sem pendentes nem falhas, e o controller segue de pé. No fim: três nós `READY`, ao menos um `node_offline` e ao menos uma unidade em voo reenfileirada (a falha pegou trabalho de verdade).
- `ROUNDS` controla a duração: 6 rodadas levam ~4 min; `ROUNDS=200` é um soak longo.

## O que o soak achou

- Um worker marcado offline pelo controller (pausa ou partição maior que a graça) recebia `failed_precondition` no `request_work` e **saía do cluster para sempre**, sem esperar o heartbeat re-registrá-lo. Corrigido: só `PermissionDenied`/`Unauthenticated` (revogado, credencial ruim) encerram o worker; o resto espera 1 s e tenta de novo, até 30 recusas seguidas; depois disso o worker sai (nó em drain, quarentena ou removido nunca se cura).

## Limites

- Só falhas de processo (kill/pause). Partição de rede, disco cheio e controller reiniciado no meio ficam para depois.
- Sem verificação de vazamento de memória ao longo de horas; o script mede correção, não recurso.
