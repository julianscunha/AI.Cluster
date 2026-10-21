---
title: "06 — Health and Trust"
type: spec
status: draft
tags: [spec, health-and-trust]
---

# 06 — Saúde e confiança

## Saúde

Health mede condição operacional do hardware/runtime.

Domínios:

- CPU;
- memory;
- thermal;
- storage;
- network;
- accelerator.

## Confiança

Trust mede se o node está autorizado e íntegro.

Estados:

UNKNOWN, UNTRUSTED, BOOTSTRAPPED, VALIDATED, TRUSTED, QUARANTINED, REVOKED.

## Estados de saúde

READY, ACTIVE, DEGRADED, DRAINING, FAILED, OFFLINE.

## Exemplos

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

## Diagnóstico automatizado

O sistema deve registrar a causa provável da degradação e seu impacto estimado.

## Térmico

Comparar temperatura com baseline específico do hardware, não apenas threshold genérico.

## Armazenamento

Detectar health warning, wear, latency anomaly e performance regression quando os dados do dispositivo estiverem disponíveis.
