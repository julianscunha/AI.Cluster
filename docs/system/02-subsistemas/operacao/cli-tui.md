---
title: "22 — CLI / TUI"
type: spec
status: draft
tags: [spec, cli-tui]
---

# 22 — CLI / TUI

## Objetivo

Operação primariamente via shell, sem dependência de GUI.

## Comandos centrais

```bash
ai-cluster status
ai-cluster dashboard
ai-cluster nodes
ai-cluster node show <id>
ai-cluster node drain <id>
ai-cluster node reprovision <id>
ai-cluster node revoke <id>
ai-cluster model list
ai-cluster model show <id>
ai-cluster request list
ai-cluster request show <id>
ai-cluster request trace <id>
ai-cluster metrics
ai-cluster events
ai-cluster topology
ai-cluster graph query <query>
ai-cluster diagnose
ai-cluster benchmark
ai-cluster rebalance
```

## Dashboard

Deve mostrar em tempo real:

- node count/state;
- health;
- capacity;
- CPU/RAM/GPU/NPU;
- temperature;
- storage health;
- network;
- requests;
- TTFT;
- ITL;
- tokens/s;
- tokens avoided;
- KV cache;
- expected vs actual;
- current plan.

## Semântica de cores

```text
GREEN  = normal
YELLOW = degraded
ORANGE = warning/draining
RED    = failed/quarantined
BLUE   = provisioning/transition
GRAY   = offline
```

Cores são acompanhadas de texto e motivo para acessibilidade e operação remota.
