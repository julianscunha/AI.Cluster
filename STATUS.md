---
title: "Architecture Status"
type: system
status: living
tags: [system, status]
---

# Architecture Status

## Decided

- Linux em todos os nodes.
- Mesmo AI Cluster OS lógico com hardware distinto.
- Reimage do sistema original após ingresso; não executar apenas um appliance sobre o Linux antigo.
- Suporte conceitual a PCs, servidores, SBCs e celulares compatíveis.
- Hardware heterogêneo.
- Hot-plug/hot-remove.
- Capability scoring multidimensional.
- Health/Capacity/Trust separados.
- Controller + Worker Runtime.
- Work Unit como unidade de execução.
- Execution Graph + Placement Plan.
- Pipeline/Tensor/Hybrid parallelism.
- Edge optimization antes da inferência pesada.
- Graph engine para relações e retrieval.
- Storage intelligence com detecção de HDD/SSD/NVMe.
- SSD/NVMe aproveitados para cache/swap conforme política.
- Telemetria detalhada e TUI via shell.
- Performance Model expected vs actual.
- Application Plane desacoplado via Inference API; acesso direto ao Data Plane não é permitido.
- Request Manager separado do Scheduler para admissão, fila, quotas e orçamento.
- Inference Engine separado do Planner/Scheduler para execução matemática do modelo.

## Proposed

- Base Linux específica.
- gRPC + Protobuf.
- PXE/iPXE.
- PostgreSQL + time-series + object storage + graph projection.
- mTLS.

## Experimental

- Mobile boot/reimage universal.
- NPU backends.
- RDMA.
- zero-copy cross-device.
- speculative execution thresholds.
- learned planner.

## Open

- linguagem principal do Controller/Worker;
- base Linux exata;
- formato interno de tensor/shard;
- graph database/projection technology;
- time-series technology;
- modelo de armazenamento distribuído;
- primeira família de LLM para benchmark;
- formato de mobile worker;
- estratégia final de kernel optimization.
