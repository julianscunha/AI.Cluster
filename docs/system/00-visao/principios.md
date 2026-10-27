---
title: "Princípios"
aliases: ["PRINCIPLES", "Princípios de projeto"]
tags: [sistema/visao, principios]
type: sistema/visao
---

# Princípios

Voltar para [[visao]]. Os princípios valem para toda a arquitetura em [[arquitetura]].

## P-001 — Compute é um pool, hardware é uma implementação

O Controller trabalha com capacidade e capabilities, não com modelos específicos de máquina.

## P-002 — Otimizar antes de escalar

Primeiro evitar trabalho; depois escolher modelo menor; só depois usar capacidade distribuída pesada.

## P-003 — Separação WHAT / WHERE / HOW

Execution Graph define o que fazer. Placement define onde. Worker Runtime define como. Ver [[execution-graph]], [[execution-planner]] e [[worker-runtime]].

## P-004 — Health não é capacidade

Um node pode ter alta capacidade nominal e baixa saúde atual. Ver [[saude-e-confianca]].

## P-005 — Trust é independente de health

Um node saudável pode estar em quarantine. Ver [[seguranca]].

## P-006 — Caches são descartáveis

Model cache, tensor cache e caches derivados não podem ser fontes únicas de verdade.

## P-007 — Storage é um recurso adjacente ao compute

SSD/NVMe podem reduzir pressão de memória, acelerar staging e preservar estado quente/morno, mas não substituem RAM. Ver [[storage-intelligence]].

## P-008 — Comunicação faz parte do custo de execução

O melhor node computacional pode perder para um node um pouco mais lento se o custo de comunicação for muito maior. Ver [[camada-de-transporte]].

## P-009 — Replanejamento é orientado a eventos

Entradas relevantes: node join/leave, thermal degradation, network degradation, model/state change, resource pressure.

## P-010 — Hot-plug deve ser planejado em torno de estado recuperável

Para uma execução em andamento, não se presume migração arbitrária de memória viva; usam-se checkpoints, replicação e recomputação seletiva. Ver [[hotplug-e-recuperacao]] e [[adr-006-hotplug]].

## P-011 — Evidência acima de suposições

Capacidade e eficiência devem ser inferidas de benchmark e telemetria observados, não de nomes de hardware. Ver [[capability-engine]].

## P-012 — Explique as decisões

Scheduler e Optimizer devem registrar por que escolheram determinado node, transporte, modelo ou otimização. Ver [[scheduler]] e [[observabilidade]].

## P-013 — Multi-arquitetura desde o início

O núcleo do protocolo deve ser neutro a x86_64/ARM64. Ver [[protocolo-controller-worker]].

## P-014 — A versão do modelo é imutável dentro de uma execução

Uma execução referencia uma variante/version exata do modelo. Ver [[model-manager]].

> [!warning] Limite atual
> O gateway e os workers conferem o `manifest.json` do modelo (versão e SHA-256) ao subir e expõem a versão. A execução ainda não referencia uma variante de um registry. Planejado: registry de variantes e placement por versão ([[model-manager]]).

## P-015 — Prefira localidade

Reutilizar estados, modelos e tensores próximos é preferível a transportar dados desnecessariamente. Ver [[state-manager]].
