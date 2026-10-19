---
title: "17 — Observability / Telemetry Engine"
type: spec
status: draft
tags: [spec, observability]
---

# 17 — Observability / Telemetry Engine

## Three telemetry types

### Metrics

Valores contínuos: CPU, RAM, temperature, tokens/s, latency.

### Events

Mudanças: NODE_JOINED, NODE_DEGRADED, NODE_LOST, SSD_WARNING, REBALANCE_STARTED.

### Traces

Caminho completo da execução.

## Node metrics

- CPU utilization/frequency/load/throttling;
- RAM usage/swap/bandwidth;
- GPU/NPU utilization/memory/temperature;
- storage latency/IOPS/throughput/health/wear;
- network bandwidth/latency/loss/errors.

## LLM metrics

- input tokens;
- output tokens;
- cached tokens;
- tokens avoided;
- TTFT;
- ITL;
- prompt throughput;
- generation throughput;
- context size;
- KV cache usage.

## Expected vs actual

Mostrar estimativas e realidade lado a lado.

## Adaptive sampling

Normal: baixa frequência. Degraded/anomaly: aumentar frequência temporariamente.

## CLI/TUI requirements

```text
ai-cluster status
ai-cluster nodes
ai-cluster node show <id>
ai-cluster node history <id>
ai-cluster metrics
ai-cluster events
ai-cluster request show <id>
ai-cluster request trace <id>
ai-cluster topology
ai-cluster diagnose
```

## Explainability

Diagnósticos devem indicar causa provável, impacto e ação recomendada.
