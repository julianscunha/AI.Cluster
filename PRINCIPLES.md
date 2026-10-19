---
title: "Principles"
type: system
status: decided
tags: [system, principles]
---

# Principles

Status: DECIDED baseline / v0.1

## P-001 — Compute is a pool, hardware is an implementation

O Controller trabalha com capacidade e capabilities, não com modelos específicos de máquina.

## P-002 — Optimize before scaling

Primeiro evitar trabalho; depois escolher modelo menor; só depois usar capacidade distribuída pesada.

## P-003 — WHAT / WHERE / HOW separation

Execution Graph define o que fazer. Placement define onde. Worker Runtime define como.

## P-004 — Health is not capacity

Um node pode ter alta capacidade nominal e baixa saúde atual.

## P-005 — Trust is independent from health

Um node saudável pode estar em quarantine.

## P-006 — Caches are disposable

Model cache, tensor cache e caches derivados não podem ser fontes únicas de verdade.

## P-007 — Storage is a compute-adjacent resource

SSD/NVMe podem reduzir pressão de memória, acelerar staging e preservar estado quente/morno, mas não substituem RAM.

## P-008 — Communication is part of execution cost

O melhor node computacional pode perder para um node um pouco mais lento se o custo de comunicação for muito maior.

## P-009 — Replanning is event-driven

Entradas relevantes: node join/leave, thermal degradation, network degradation, model/state change, resource pressure.

## P-010 — Hot-plug must be planned around recoverable state

Para uma execução em andamento, não se presume migração arbitrária de memória viva; usam-se checkpoints, replicação e recomputação seletiva.

## P-011 — Evidence over assumptions

Capacidade e eficiência devem ser inferidas de benchmark e telemetria observados, não de nomes de hardware.

## P-012 — Explain decisions

Scheduler e Optimizer devem registrar por que escolheram determinado node, transporte, modelo ou otimização.

## P-013 — Multi-architecture from the start

O núcleo do protocolo deve ser neutro a x86_64/ARM64.

## P-014 — Model version is immutable within an execution

Uma execução referencia uma variante/version exata do modelo.

## P-015 — Prefer locality

Reutilizar estados, modelos e tensores próximos é preferível a transportar dados desnecessariamente.
