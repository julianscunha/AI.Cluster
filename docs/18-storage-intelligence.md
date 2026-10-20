---
title: "18 — Storage Intelligence"
type: spec
status: draft
tags: [spec, storage-intelligence]
---

# 18 — Storage Intelligence

## Objetivo

Usar o armazenamento como recurso complementar à memória e à execução, adaptando o comportamento ao tipo e saúde do dispositivo.

## Detection

Identificar:

- HDD;
- SATA SSD;
- NVMe;
- capacity;
- read/write throughput;
- latency;
- temperature;
- health;
- wear/endurance quando disponível.

## Roles

```text
HDD -> cold storage, limited cache, conservative swap
SSD -> swap, cache, staging, checkpoint
NVMe -> aggressive warm cache, staging, checkpoint, tensor cache
```

As políticas são dinâmicas.

## Space allocation

Reservar:

- OS/recovery;
- swap;
- model cache;
- tensor cache;
- checkpoint/state;
- emergency reserve.

Não consumir 100% do espaço.

## SSD policy

Em SSD/NVMe saudáveis e rápidos, aproveitar uma parcela grande da capacidade para swap/cache quando isso melhora o cluster, respeitando espaço livre, endurance e thermal headroom.

## Health integration

Em degradação:

- reduzir escrita pesada;
- evitar checkpoints agressivos;
- reduzir dependência de swap;
- migrar dados quando possível;
- alertar Controller.
