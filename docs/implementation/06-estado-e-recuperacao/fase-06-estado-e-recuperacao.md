---
title: "F06 — Estado e recuperação"
aliases: ["Fase I", "Fase I — State Manager", "roadmap-4", "Storage/State", "fase-i"]
tags: [implementacao/fase/06, implementacao/estado/implementado]
type: implementacao/fase
status: implementado
codigos-antigos: ["I1", "I2", "I3", "I4", "roadmap-4"]
sistema: ["[[state-manager]]", "[[hotplug-e-recuperacao]]"]
---
# F06 — Estado e recuperação

Esta é a nota da fase e o MOC de `06-estado-e-recuperacao/`.

## Objetivo

Estado de execução que sobrevive a restart do controller e recuperação de execução após falha de
nó e interrupção abrupta do controller, sem perder trabalho recuperável. Critério de saída do
roadmap (Fase 4): perda de nó não apaga estado recuperável de execução.

## Depende de

- [[fase-03-execucao-distribuida]] (Work Unit, DAG, retry e recovery por nó)
- [[fase-05-observabilidade]] (eventos e métricas da recuperação)

## Entrega

- Snapshot do controller (`AI_CLUSTER_STATE`) com fila, posse das unidades em voo e resultados
  ainda necessários.
- Cópia do snapshot no volume e, opcionalmente, num nó (`AI_CLUSTER_STATE_REPLICA`).
- Checkpoint podado: só o que uma unidade não terminada ainda lê.
- Recuperação por dois relógios (grace de heartbeat e grace desde o start) e recomputação.

## Checklist

| Estado | Item | Slice |
| --- | --- | --- |
| [x] | I1 state directory: fila, posse e cópia sobrevivem a restart | [[f06-persistencia-e-checkpoint]] |
| [~] | I1: diretório de referências lógicas `state://…` e réplica fora do volume | pendente, [[fase-12-model-runtime]] |
| [x] | I3 checkpoint incremental (poda por leitor vivo) | [[f06-persistencia-e-checkpoint]] |
| [x] | I4 recovery: nó perdido e interrupção abrupta com unidade em voo | [[f06-recuperacao]] |
| [x] | I4 promoção de réplica: cópia num nó recuperada quando o volume morre | [[f06-recuperacao]] |
| [x] | Roadmap Fase 4: recovery por reload no startup do controller | [[f06-persistencia-e-checkpoint]] |
| [ ] | Replicação de estado por chave (KV cache, execution state, model state) | pendente, [[fase-12-model-runtime]] |
| [ ] | Journal de deltas ou snapshot periódico (hoje o conjunto vivo é reescrito inteiro) | pendente, sem fase |
| [x] | Registry persistido: decidido não persistir o estado vivo; credencial, aprovação e quarentena já vão a disco | [[adr-013-estado-do-controller-e-recuperacao]] |
| [x] | Eventos persistidos (`AI_CLUSTER_EVENTS`, recarga dos últimos 256) | [[f06-eventos-persistidos]] |
| [~] | I2 HOT/WARM/COLD: classes detectadas por nó; alocação por camada não existe | ver [[fase-10-inteligencia-de-storage]] |
| [ ] | Roadmap Fase 4: swap/cache policies | ver [[fase-10-inteligencia-de-storage]] |

Legenda: `[x]` implementado e validado · `[~]` parcial · `[ ]` não iniciado.

## Gate

Critério de saída (EP §10 e roadmap Fase 4): execução distribuída recupera-se de falha planejada e
de interrupção abrupta em ambiente controlado. **Atendido**, com a ressalva do estado por chave.

| Item do gate padrão (EP §19) | Situação |
| --- | --- |
| Código | feito (`controller/src/work.rs`, `main.rs`, `registry.rs`, worker) |
| Benchmark | feito: tamanho do checkpoint (4,19 MB → 2 bytes) |
| Telemetria | feita: eventos de recuperação e log persistido (`phase-i-events.sh`) |
| Teste de falha | feito: `phase-2-failure.sh`, `phase-i-state.sh`, `phase-i4-recovery.sh`, `phase-i4-replica.sh` |
| Doc | feito (este conjunto) |
| ADR | feito: [[adr-013-estado-do-controller-e-recuperacao]] |

## Slices

- [[f06-persistencia-e-checkpoint]]: fila, posse, cópia no volume e poda do checkpoint (I1, I3).
- [[f06-eventos-persistidos]]: linha do tempo em arquivo que atravessa o restart.
- [[f06-recuperacao]]: dois relógios, `requeue_for_node`, cópia num nó e época (I4).

## Códigos antigos

- I1 (state directory), I2 (HOT/WARM/COLD, vai para F10), I3 (checkpoint), I4 (recovery).
- Roadmap Fase 4 (storage/state).

## Specs do sistema

- [[state-manager]]: sequência de recuperação e diretório de estado (alvo).
- [[hotplug-e-recuperacao]]: falha de nó e recuperação de execução.
- [[storage-intelligence]]: classes de armazenamento (I2 no alvo).

> [!info] O que já existe
> Snapshot de fila, posse e cópia de estado, checkpoint podado e recuperação por dois relógios,
> todos em `code/crates/controller` e no worker (`AI_CLUSTER_STATE`, `AI_CLUSTER_STATE_REPLICA`).

> [!todo] O que falta
> Estado por chave, diretório `state://`, réplica fora do volume, journal de deltas e registry
> persistido (decidido não persistir o estado vivo, ver [[adr-013-estado-do-controller-e-recuperacao]]). Ver os itens pendentes no checklist.
