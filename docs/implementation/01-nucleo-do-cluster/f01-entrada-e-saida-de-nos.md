---
title: "F01 — Entrada e saída de nós"
aliases: ["phase-1/join-and-leave", "roadmap-1 saída", "Saída planejada de nó"]
tags: [implementacao/fase/01, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-01-nucleo-do-cluster]]"
codigos-antigos: ["phase-1/join-and-leave", "roadmap-1"]
sistema: ["[[hotplug-e-recuperacao]]", "[[protocolo-controller-worker]]"]
evidencia: ["code/tests/phase-1-leave.sh"]
---
# F01 — Entrada e saída de nós

- [ ] Superado

## Objetivo

Critério de saída do roadmap para a Fase 1: **nós entram e saem com segurança**. Uma saída planejada não pode deixar trabalho em voo encalhado nem executar uma unidade duas vezes.

## O que foi feito

- RPC `LeaveNode` em `code/proto/cluster.proto`: um nó anuncia uma saída planejada.
- `Registry::begin_drain`, `Registry::remove` e `Registry::is_available` em `code/crates/controller/src/registry.rs`.
- `WorkStore::requeue_for_node` em `code/crates/controller/src/work.rs`: unidades em voo voltam para a fila em vez de ficarem encalhadas.
- O registro de dispatch passa a guardar a própria unidade de trabalho, para que ela possa ser devolvida.
- `RequestWork` recusa nós que estão drenando, offline, com falha ou em quarentena.
- O sweep de offline do controller também faz requeue: um nó que fica em silêncio sem se despedir perde seu trabalho em voo para os sobreviventes.
- O worker instala handler de SIGTERM/SIGINT e chama `LeaveNode` antes de sair.

## Decisões

- **Semântica de posse.** Uma unidade está em voo do `claim` até o `complete`. O requeue remove o registro de dispatch, então um resultado atrasado do nó que partiu é recusado: só o nó que reivindicar a unidade em seguida pode completá-la. Nada roda duas vezes e nada se perde.

## Evidência

Docker, dois nós, uma saída planejada: `bash code/tests/phase-1-leave.sh`. Controller mais `node-b`; `node-c` entra; `node-b` sai com SIGTERM.

| trabalho | nó | compute_us | transfer_us | total_us |
|---|---|---|---|---|
| w-0 | node-b | 2,658 | 8,342 | 11,000 |
| w-1 | node-c | 643 | 3,357 | 4,000 |
| w-2 | node-b | 587 | 2,413 | 3,000 |
| w-3 | node-c | 2,639 | 6,361 | 9,000 |
| w-4 | node-b | 670 | 2,330 | 3,000 |
| w-5 | node-c | 1,345 | 2,655 | 4,000 |

- `concluidas=6 pendentes=0`.
- Log do worker: `saiu do cluster node_id=node-b aceito=true requeued=0`.
- Log do controller: `nó node-b saiu; 0 unidades em voo reenfileiradas, 0 falharam`.
- `ai-cluster nodes` após a saída lista só `node-c`.

`requeued=0` porque o worker entrega sua unidade antes do próximo tick, então no momento da partida ele não segurava nada. O caminho de requeue em si é coberto pelos testes unitários `requeue_for_node_returns_in_flight_units_to_the_queue` e `leave_node_requeues_in_flight_work_and_drops_the_node`.

## Limites

> [!warning] Teto de dispatch
> O worker pede uma unidade por tick de heartbeat, então a latência de dispatch é limitada pelo intervalo de heartbeat de cinco segundos. Revisitar quando throughput importar mais do que a simplicidade de um único laço.

## Próximos passos

- Nenhum pendente neste slice. O requeue por queda abrupta (worker morto com unidade em voo) é validado por `code/tests/phase-2-failure.sh` e documentado em [[f03-tolerancia-a-falhas]].
- Revisitar o intervalo de dispatch quando throughput importar mais que a simplicidade do laço (ver Limites).
