---
title: "06 — Health and Trust"
type: spec
status: draft
tags: [spec, health-and-trust]
---

# 06 — Health and Trust

## Health

Health mede condição operacional do hardware/runtime.

Domínios:

- CPU;
- memory;
- thermal;
- storage;
- network;
- accelerator.

## Trust

Trust mede se o node está autorizado e íntegro.

Estados:

UNKNOWN, UNTRUSTED, BOOTSTRAPPED, VALIDATED, TRUSTED, QUARANTINED, REVOKED.

## Health states

READY, ACTIVE, DEGRADED, DRAINING, FAILED, OFFLINE.

## Examples

Node pode ser:

```text
Health: DEGRADED
Trust: TRUSTED
Capacity: HIGH
State: ACTIVE
```

ou:

```text
Health: GREEN
Trust: QUARANTINED
Capacity: HIGH
State: BLOCKED
```

## Automated diagnostics

O sistema deve registrar a causa provável da degradação e seu impacto estimado.

## Thermal

Comparar temperatura com baseline específico do hardware, não apenas threshold genérico.

## Storage

Detectar health warning, wear, latency anomaly e performance regression quando os dados do dispositivo estiverem disponíveis.
