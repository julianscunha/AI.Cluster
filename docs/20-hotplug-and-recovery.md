---
title: "20 — Hot-Plug and Recovery"
type: spec
status: draft
tags: [spec, hotplug-and-recovery]
---

# 20 — Hot-plug e recuperação

## Remoção planejada

```text
ACTIVE
 -> DRAINING
 -> no new work
 -> finish/checkpoint active work
 -> release model/state
 -> OFFLINE
```

## Perda inesperada

```text
heartbeat lost
 -> mark FAILED
 -> identify impacted Work Units
 -> recover state
 -> choose replacements
 -> new plan
 -> resume
```

## Entrada

```text
DISCOVERED
 -> TRUST
 -> PROVISION
 -> BENCHMARK
 -> warm model/state
 -> READY
```

## Atualização de capacidade

After join/leave:

1. update Cluster State;
2. update graph;
3. update performance capacity;
4. generate candidate plan;
5. compare expected improvement;
6. replan if worthwhile.

## Não-objetivo

Não é requisito inicial migrar qualquer memória viva arbitrária byte-a-byte entre hosts. A estratégia deve privilegiar state objects, checkpoints e recomputação seletiva.
