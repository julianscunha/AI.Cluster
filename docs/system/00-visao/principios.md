---
title: "Princípios"
type: system
status: decided
tags: [system, principles]
---

# Princípios

Status: baseline DECIDIDO / v0.1

## P-001 — Compute é um pool, hardware é uma implementação

O Controller trabalha com capacidade e capabilities, não com modelos específicos de máquina.

## P-002 — Otimizar antes de escalar

Primeiro evitar trabalho; depois escolher modelo menor; só depois usar capacidade distribuída pesada.

## P-003 — Separação WHAT / WHERE / HOW

Execution Graph define o que fazer. Placement define onde. Worker Runtime define como.

## P-004 — Health não é capacidade

Um node pode ter alta capacidade nominal e baixa saúde atual.

## P-005 — Trust é independente de health

Um node saudável pode estar em quarantine.

## P-006 — Caches são descartáveis

Model cache, tensor cache e caches derivados não podem ser fontes únicas de verdade.

## P-007 — Storage é um recurso adjacente ao compute

SSD/NVMe podem reduzir pressão de memória, acelerar staging e preservar estado quente/morno, mas não substituem RAM.

## P-008 — Comunicação faz parte do custo de execução

O melhor node computacional pode perder para um node um pouco mais lento se o custo de comunicação for muito maior.

## P-009 — Replanejamento é orientado a eventos

Entradas relevantes: node join/leave, thermal degradation, network degradation, model/state change, resource pressure.

## P-010 — Hot-plug deve ser planejado em torno de estado recuperável

Para uma execução em andamento, não se presume migração arbitrária de memória viva; usam-se checkpoints, replicação e recomputação seletiva.

## P-011 — Evidência acima de suposições

Capacidade e eficiência devem ser inferidas de benchmark e telemetria observados, não de nomes de hardware.

## P-012 — Explique as decisões

Scheduler e Optimizer devem registrar por que escolheram determinado node, transporte, modelo ou otimização.

## P-013 — Multi-arquitetura desde o início

O núcleo do protocolo deve ser neutro a x86_64/ARM64.

## P-014 — A versão do modelo é imutável dentro de uma execução

Uma execução referencia uma variante/version exata do modelo.

## P-015 — Prefira localidade

Reutilizar estados, modelos e tensores próximos é preferível a transportar dados desnecessariamente.
