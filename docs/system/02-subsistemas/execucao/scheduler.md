---
title: "11 — Scheduler"
type: spec
status: draft
tags: [spec, scheduler]
---

# 11 — Escalonador

## Objetivo

Atribuir Work Units a nodes/resources maximizando o objetivo da execução.

## Scheduler de dois níveis

Global:
- node selection;
- plan placement;
- reservations;
- priorities;
- rebalancing.

Local:
- cores;
- GPU/NPU queues;
- local memory;
- local order.

## Filtragem de candidatos

Excluir rapidamente nodes sem:

- capability;
- memory;
- trust;
- network reachability;
- precision support.

## Dimensões de pontuação

- compute cost;
- communication cost;
- queue cost;
- locality;
- thermal headroom;
- health;
- state movement cost;
- reliability;
- deadline feasibility.

## Reserva

Recursos podem ser reservados para execução futura.

## Rebalanceamento

Somente migrar quando o ganho esperado superar um threshold configurável e o custo de migração.

## Execução especulativa

Usar apenas para stragglers e quando a economia de latência esperada justificar o consumo de compute.
