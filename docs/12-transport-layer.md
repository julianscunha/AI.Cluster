---
title: "12 — Transport Layer"
type: spec
status: draft
tags: [spec, transport-layer]
---

# 12 — Transport Layer

## Objetivo

Transportar Work Units, tensors, state e artefatos usando o meio mais adequado.

## Abstração de transporte

```text
Transport Manager
 |- shared memory
 |- Ethernet/TCP
 |- RDMA (future)
 |- USB
 `- future transports
```

## Control vs data plane

Control plane usa mensagens pequenas, versionadas e autenticadas.

Data plane é otimizado para buffers/tensors.

## Métricas de link

- bandwidth;
- latency;
- jitter;
- packet loss;
- retries;
- current utilization;
- health.

## Decisões

Antes de transferir:

1. tensor already local?
2. cache hit?
3. recompute cheaper?
4. compress/quantize beneficial?
5. best source replica?
6. best path?

## Chunking

Objetos grandes devem suportar chunks, retry, resume e checksum.

## Compression

Pode ser aplicada quando o ganho de bandwidth superar custo de compressão/decompressão.

## USB

USB é um transporte de primeira classe para mobile workers, desde que o caminho efetivo ofereça desempenho e estabilidade suficientes.
