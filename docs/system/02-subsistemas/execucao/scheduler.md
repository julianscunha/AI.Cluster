---
title: "Escalonador"
aliases: ["Scheduler", "docs/11", "11 — Scheduler"]
tags: [sistema/subsistema/execucao, subsistema/scheduler]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-04-escalonamento]]"]
---

# Escalonador

Voltar para [[execucao]].

## Objetivo

Atribuir Work Units a nós e recursos, maximizando o objetivo da execução: latência, vazão ou o perfil da requisição.

## Dois níveis

**Global** (controller):

- seleção de nó;
- placement do plano ([[execution-planner]]);
- reservas;
- prioridades;
- rebalanceamento.

**Local** (worker):

- cores;
- filas de GPU/NPU;
- memória local;
- ordem local.

## Modelo de despacho

O despacho é por demanda: cada worker pergunta ao controller por trabalho (`RequestWork`), e o controller decide, naquele momento, se entrega ou não a unidade àquele nó. Por isso a decisão de placement é um filtro e um ranking sobre o nó que pergunta, não uma escolha de empurrar trabalho para um nó.

## Filtragem de candidatos

Excluir rapidamente os nós sem:

- capability: threads mínimas declaradas pela unidade;
- memória: footprint da unidade contra a memória reportada;
- confiança: nó aprovado e não em estado de saída, quarentena ou falha;
- alcance de rede;
- suporte à precisão.

## Dimensões de pontuação

- custo de compute;
- custo de comunicação;
- custo de fila;
- localidade;
- folga térmica;
- saúde;
- custo de mover estado;
- confiabilidade;
- viabilidade do prazo.

## Reserva

Um nó pode ser reservado para uma requisição antes de o trabalho existir. Enquanto a reserva vale, o nó só aceita unidades dessa requisição, e as unidades dela só rodam nele, mesmo que não seja o mais barato.

## Rebalanceamento

Migrar trabalho só quando o ganho esperado supera um limiar configurável e o custo da migração.

## Execução especulativa

Usar apenas para stragglers e quando a economia de latência esperada justifica o compute gasto.

> [!info] O que já existe
> - **Filtro duro** (`placement::fits`, `code/crates/controller/src/placement.rs`): `min_cpu_threads` da unidade e footprint (A e B lidos, C escrito, 8 bytes por elemento) contra threads e memória reportadas. Memória zero significa "não reportado" e não filtra. Ver [[f04-placement]].
> - **Elegibilidade** (`registry::eligible`): nó confiável e fora de `DRAINING`, `QUARANTINED`, `FAILED` e `OFFLINE`. Confiança é um flag binário (aprovação e credencial, ver [[f07-aprovacao-de-no]]).
> - **Custo** (`placement_cost`): compute, transferência, fila (50 por unidade em voo) e saúde. Pesos são placeholders; só a ordem entre nós tem significado.
> - **Ranking só entre ociosos** e **histerese** de 10% (`AI_CLUSTER_PLACEMENT_TOLERANCE`). `AI_CLUSTER_PLACEMENT=off` volta ao primeiro que pergunta. Ver [[f04-placement]].
> - **Reservas** (`code/crates/controller/src/reservations.rs`): por nó, com TTL padrão de 60 s, vencimento lido na consulta. RPCs `Reserve`, `ListReservations`, `ReleaseReservation`. Ver [[f04-reservas]].
> - **Local**: `worker::core_budget` concede `min(threads reportadas, cores do host)`, e o produto é dividido por linhas. Ver [[f04-scheduler-local]].
> - Evidências: `bash code/tests/phase-g-placement.sh`, `bash code/tests/phase-g-memory.sh`, `bash code/tests/phase-g-reservations.sh`, `bash code/tests/phase-g-cores.sh`.

> [!todo] Não implementado
> - Prioridades entre unidades ou requisições (classes CRITICAL a CACHE, ver [[worker-runtime]]). A fila é FIFO com filtros.
> - Dimensões de pontuação ausentes: localidade, folga térmica, custo de mover estado, confiabilidade e prazo. O registro não carrega esses dados.
> - Filtros de precisão, acelerador e armazenamento. O worker reporta, mas nenhuma unidade pode exigir.
> - Confiança em níveis (UNKNOWN → TRUSTED). Hoje é binária.
> - Rebalanceamento: trabalho só muda de nó quando o nó sai ou falha.
> - Execução especulativa.
> - Fila local por recurso no worker, com ordem e admissão locais. O worker roda uma unidade por vez.
> - Escolha da unidade de melhor encaixe para o nó que pergunta (a G4). Hoje só a primeira unidade elegível é avaliada.
> - Reserva de fração de nó (cores ou memória). A reserva retém o nó inteiro.
> - Reserva que sobrevive a restart. Hoje é só em memória.

> [!warning] Teto do MVP
> Um nó ocioso que para de perguntar pode segurar uma unidade. Ele continua mandando heartbeat, então não é marcado offline, e o controller infere "ocioso" só pela ausência de unidade em voo. Revisitar com idade do último poll ou contador de recusas. Mesma nota em [[hotplug-e-recuperacao]].
