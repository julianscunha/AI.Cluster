# AI Cluster — Master Execution Plan

Status: PROPOSED / v0.1

Este documento transforma a especificação arquitetural em uma sequência executável. A regra é não iniciar uma camada cuja infraestrutura anterior ainda não tenha sido validada.

## 1. Dependency chain

```text
Repository
  ↓
Protocol / IDs / config
  ↓
Controller state
  ↓
Worker agent
  ↓
Discovery + heartbeat
  ↓
Capability benchmark
  ↓
Work Unit
  ↓
Transport
  ↓
Execution Graph
  ↓
Scheduler
  ↓
Telemetry
  ↓
State Manager
  ↓
AI Cluster OS provisioning
  ↓
Storage intelligence
  ↓
Graph engine
  ↓
Edge optimizer
  ↓
Model registry/runtime
  ↓
Distributed LLM
  ↓
Adaptive planner
```

## 2. Phase 0 — Foundation

### A1 Repository

Criar monorepo ou estrutura modular com:

```text
controller/
worker/
common/
protocol/
os/
cli/
benchmarks/
docs/
```

Acceptance:

- builds reproduzíveis;
- versionamento;
- CI;
- unit test harness.

### A2 Common types

Implementar IDs, timestamps, status enums, error model, capability types e serialization.

Acceptance: Controller e Worker compartilham schemas sem duplicação.

### A3 Protocol

Implementar registration, heartbeat, capabilities e health.

Acceptance: dois nodes mantêm sessão autenticada e reportam estado.

## 3. Phase B — Cluster Core

### B1 Controller state

In-memory authoritative runtime state com persistência.

### B2 Node registry

Entrada, atualização, saída e lifecycle.

### B3 Worker agent

Daemon Linux mínimo.

### B4 CLI

Primeiros comandos:

```bash
ai-cluster status
ai-cluster nodes
ai-cluster node show <id>
ai-cluster events
```

Exit criterion: cluster de 2–4 máquinas Linux funciona sem operação manual contínua.

## 4. Phase C — Capability / Health

### C1 Hardware discovery

CPU, RAM, storage, network, accelerators.

### C2 Benchmark engine

Benchmarks controlados para compute, memory, storage, network.

### C3 Capability model

Baseline + current + confidence.

### C4 Health model

Thermal, memory pressure, storage health, network degradation.

Exit criterion: adicionar node produz capability profile reproduzível.

## 5. Phase D — Work Unit Runtime

### D1 Work Unit schema

Criar estado, requirements, dependencies, retries.

### D2 Local worker execution

Executar operações CPU simples.

### D3 Result integrity

Checksum, status e observabilidade.

Exit criterion: uma Work Unit consegue circular entre Controller e Worker e devolver resultado validado.

## 6. Phase E — Transport

### E1 Transport abstraction

API única:

```text
send
receive
stream
probe
```

### E2 TCP transport

Baseline.

### E3 Shared memory

Otimizar comunicação intra-host.

### E4 USB transport

Implementar somente após o protocolo TCP estar estável.

### E5 Adaptive transport metrics

Medir bandwidth, latency, loss, congestion.

Exit criterion: mesma Work Unit pode usar transportes diferentes sem alterar a lógica da operação.

## 7. Phase F — Execution Graph

### F1 Graph schema

Nodes, edges, dependencies, barriers.

### F2 DAG executor

Executar branches em paralelo.

### F3 Placement abstraction

Separar graph de placement.

### F4 Retry / recovery

Reatribuir Work Unit falha.

Exit criterion: perder um worker não corrompe o DAG.

## 8. Phase G — Scheduler

### G1 Candidate filtering

Capabilities, memory, trust, health.

### G2 Cost model

Compute + communication + queue + locality.

### G3 Global scheduler

Placement.

### G4 Local scheduler

Resource queues.

### G5 Rebalancing

Threshold + hysteresis.

### G6 Reservations

Recursos futuros.

Exit criterion: o scheduler prova, por benchmark, que pode escolher placement melhor que round-robin para workloads testados.

## 9. Phase H — Observability

### H1 Metrics

Node, runtime e cluster.

### H2 Events

Lifecycle e incidentes.

### H3 Traces

Request → Work Unit → transfer → compute.

### H4 TUI

Dashboard em shell.

### H5 Expected vs actual

Expor prediction error.

Exit criterion: qualquer requisição de teste possui trace completo e métricas suficientes para explicar sua latência.

## 10. Phase I — State Manager

### I1 State directory

IDs, ownership, replicas.

### I2 HOT/WARM/COLD

RAM/VRAM, SSD/NVMe, persistent storage.

### I3 Checkpoint

Incremental.

### I4 Recovery

Node loss, replica promotion, recomputation.

Exit criterion: execução distribuída recupera-se de falha planejada e de interrupção abrupta em um ambiente controlado.

## 11. Phase J — AI Cluster OS

### J1 Base image

Selecionar base Linux.

### J2 Minimal services

Somente componentes necessários.

### J3 Bootstrap

Existing Linux → enrollment.

### J4 Reimage

Instalação limpa.

### J5 Boot/recovery

A/B update.

### J6 Multi-arch

x86_64 primeiro; ARM64 em seguida.

Exit criterion: máquina de teste é reprovisionada automaticamente e retorna como READY.

## 12. Phase K — Storage Intelligence

### K1 Detection

HDD/SATA SSD/NVMe.

### K2 Benchmark

Throughput/latency.

### K3 Health

SMART/NVMe health onde suportado.

### K4 Dynamic allocation

Swap/model cache/tensor cache/checkpoint/reserve.

### K5 Policy adaptation

Reduzir escrita quando health/wear piorarem.

Exit criterion: pelo menos HDD e NVMe recebem políticas distintas e mensuráveis.

## 13. Phase L — Graph Engine

### L1 Graph data model

Hardware, network, knowledge, execution, observability.

### L2 Projection

Sincronizar objetos operacionais.

### L3 Traversal

Queries de topology/locality/relation.

### L4 Retrieval benchmark

Comparar graph-assisted retrieval com baseline para casos relevantes.

Exit criterion: só adotar traversal onde houver ganho mensurável.

## 14. Phase M — Edge Optimizer

### M1 Exact cache
### M2 Semantic cache
### M3 Graph retrieval
### M4 Context selection
### M5 Deduplication
### M6 Compression
### M7 Skill/tool resolution
### M8 Prompt compiler
### M9 Token budgeter
### M10 Fast path

Exit criterion: demonstrar redução de tokens e/ou latência que supere o custo do próprio optimizer.

## 15. Phase N — Model Runtime

### N1 Model Registry
### N2 Manifest
### N3 Variant/precision
### N4 Sharding
### N5 Model cache
### N6 CPU inference
### N7 Pipeline parallelism
### N8 Tensor parallelism
### N9 Hybrid planner

Exit criterion: modelo robusto executa em múltiplos nodes CPU e todo overhead é mensurado.

## 16. Phase O — Accelerators

Ordem sugerida:

```text
CPU x86_64
 → CPU ARM64
 → NVIDIA GPU
 → AMD/Intel GPU
 → NPU abstraction
 → mobile NPU
```

Cada backend precisa passar por capability discovery e benchmark próprio.

## 17. Phase P — Adaptive Runtime

### P1 Performance learning
### P2 Predictive placement
### P3 Prefetch
### P4 Speculative execution
### P5 Capacity inference
### P6 Automated rebalancing

Exit criterion: previsão melhora estatisticamente em workload repetido e o sistema consegue provar expected vs actual.

## 18. Phase Q — Mobile Worker

### Q1 Supported hardware matrix
### Q2 Bootloader/reimage path
### Q3 ARM64 image
### Q4 USB transport
### Q5 accelerator backend
### Q6 thermal/battery constraints

Um mobile node deve ser considerado apenas se seu valor computacional líquido, após comunicação e limitações térmicas, for positivo.

## 19. Gate criteria between phases

Cada fase deve produzir:

```text
code
benchmarks
telemetry
failure tests
documentation update
ADR update when architecture changes
```

Não avançar uma fase simplesmente porque o código compila.

## 20. First experimental target

O primeiro experimento não deve ser um LLM.

Usar:

```text
2–4 Linux nodes
CPU-only
1 GbE
matrix/DAG workload
```

Medir:

```text
single node
2 nodes
3 nodes
4 nodes
```

E comparar:

```text
compute time
communication time
total latency
throughput
prediction
actual
```

A primeira pergunta experimental é:

> Quando a comunicação é adicionada, em quais tamanhos de Work Unit o hardware adicional produz ganho líquido?

Esse resultado orienta todo o restante da plataforma.
