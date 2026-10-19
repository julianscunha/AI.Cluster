# 11 — Scheduler

## Objective

Atribuir Work Units a nodes/resources maximizando o objetivo da execução.

## Two-level scheduler

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

## Candidate filtering

Excluir rapidamente nodes sem:

- capability;
- memory;
- trust;
- network reachability;
- precision support.

## Scoring dimensions

- compute cost;
- communication cost;
- queue cost;
- locality;
- thermal headroom;
- health;
- state movement cost;
- reliability;
- deadline feasibility.

## Reservation

Recursos podem ser reservados para execução futura.

## Rebalancing

Somente migrar quando o ganho esperado superar um threshold configurável e o custo de migração.

## Speculative execution

Usar apenas para stragglers e quando a economia de latência esperada justificar o consumo de compute.
