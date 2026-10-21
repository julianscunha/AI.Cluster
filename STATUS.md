---
title: "Status da arquitetura"
type: system
status: living
tags: [system, status]
---

# Status da arquitetura

Legenda: `[x]` implementado e validado · `[~]` parcial · `[ ]` não iniciado.

## Decidido

- Linux em todos os nodes. `[x]`
- Mesmo AI Cluster OS lógico com hardware distinto. `[ ]`
- Reimage do sistema original após ingresso; não executar apenas um appliance sobre o Linux antigo. `[ ]`
- Suporte conceitual a PCs, servidores, SBCs e celulares compatíveis. `[ ]`
- Hardware heterogêneo. `[~]` — nós de tamanhos diferentes já são filtrados por capability.
- Hot-plug/hot-remove. `[~]` — join/leave seguro validado; recuperação de estado em aberto.
- Capability scoring multidimensional. `[~]` — threads/RAM/health reportados; scoring multidimensional ainda não.
- Health/Capacity/Trust separados. `[~]` — health e capacity reportados; trust é placeholder.
- Controller + Worker Runtime. `[x]`
- Work Unit como unidade de execução. `[x]`
- Execution Graph + Placement Plan. `[~]` — DAG com dependências e retries validado; modelo de custo de placement implementado (`placement.rs`), sem locality nem térmico.
- Pipeline/Tensor/Hybrid parallelism. `[ ]`
- Edge optimization antes da inferência pesada. `[ ]`
- Graph engine para relações e retrieval. `[ ]`
- Storage intelligence com detecção de HDD/SSD/NVMe. `[ ]`
- SSD/NVMe aproveitados para cache/swap conforme política. `[ ]`
- Telemetria detalhada e TUI via shell. `[~]` — telemetria de eventos pronta; TUI não iniciada.
- Performance Model expected vs actual. `[ ]`
- Application Plane desacoplado via Inference API; acesso direto ao Data Plane não é permitido. `[ ]`
- Request Manager separado do Scheduler para admissão, fila, quotas e orçamento. `[ ]`
- Inference Engine separado do Planner/Scheduler para execução matemática do modelo. `[ ]`

## Proposto

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

## Aberto

- linguagem principal do Controller/Worker;
- base Linux exata;
- formato interno de tensor/shard;
- graph database/projection technology;
- time-series technology;
- modelo de armazenamento distribuído;
- primeira família de LLM para benchmark;
- formato de mobile worker;
- estratégia final de kernel optimization.
