---
title: "MVP — First Build"
type: implementation
status: proposed
tags: [implementation, mvp]
---

# MVP — First Build

## Objective

Provar que hardware heterogêneo pode colaborar na execução de uma única carga distribuída sem depender inicialmente de um LLM.

## Hardware

Primeiro MVP recomendado:

- 2–4 máquinas Linux x86_64;
- 4–16 GB RAM cada;
- Ethernet;
- SSD quando disponível.

## Software

- Controller;
- Worker Agent;
- Work Unit protocol;
- CPU tensor/matrix backend;
- telemetry;
- CLI.

## Workload

Multiplicação de matrizes, operações elementwise e DAG de dependências.

## Demonstration

```text
Request
 -> Work Unit A
 -> Work Unit B + Work Unit C in parallel
 -> Work Unit D
 -> response
```

## Measurements

- execution time;
- network transfer;
- queue;
- serialization;
- CPU utilization;
- memory;
- predicted vs actual;
- gain/loss from adding node.

## Success criteria

O MVP só é considerado bem-sucedido se conseguir demonstrar casos em que adicionar um node melhora o tempo ou throughput da execução, e também detectar casos em que adicionar um node piora o resultado devido a comunicação.
