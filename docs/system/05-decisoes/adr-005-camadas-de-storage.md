---
title: "ADR-005 — Storage Tiering"
type: adr
status: decided
tags: [adr, storage-tiering]
---

# ADR-005 — Storage Tiering

Status: DECIDED

## Decisão

Tratar RAM/VRAM como HOT, SSD/NVMe como WARM e storage persistente/HDD como COLD.

## Additional policy

SSD/NVMe saudáveis podem receber uso amplo de cache e swap, mas mantendo reserva operacional e limites de endurance.
