---
title: "AI Cluster — Plano Mestre de Execução"
type: system
status: living
tags: [system, execution-plan]
---

# AI Cluster — Plano Mestre de Execução

Status: EM EXECUÇÃO / v0.1 — itens executados marcados.

Legenda: `[x]` executado e validado · `[~]` parcial · `[ ]` não iniciado.

Este documento transforma a especificação arquitetural em uma sequência executável. A regra é não iniciar uma camada cuja infraestrutura anterior ainda não tenha sido validada.

## 1. Cadeia de dependências

```text
Repositório
  ↓
Protocolo / IDs / config
  ↓
Estado do Controller
  ↓
Worker agent
  ↓
Discovery + heartbeat
  ↓
Benchmark de capability
  ↓
Work Unit
  ↓
Transporte
  ↓
Execution Graph
  ↓
Scheduler
  ↓
Telemetria
  ↓
State Manager
  ↓
Provisionamento do AI Cluster OS
  ↓
Inteligência de storage
  ↓
Graph engine
  ↓
Edge optimizer
  ↓
Model registry/runtime
  ↓
LLM distribuído
  ↓
Planner adaptativo
```

## 2. Fase 0 — Fundação

### A1 Repositório

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

Aceite:

- [x] builds reproduzíveis;
- [x] versionamento;
- [x] CI;
- [x] unit test harness.

### A2 Tipos comuns

Implementar IDs, timestamps, status enums, error model, capability types e serialization.

Aceite: Controller e Worker compartilham schemas sem duplicação. — [x]

### A3 Protocolo

Implementar registration, heartbeat, capabilities e health.

Aceite: dois nodes mantêm sessão autenticada e reportam estado. — [x]

## 3. Fase B — Cluster Core

### B1 Estado do Controller

Estado autoritativo em memória com persistência.

- [x] estado em memória;
- [~] persistência — snapshot da fila e dos outputs (`persistence.md`); registry e eventos
  não sobrevivem a restart.

### B2 Node registry

Entrada, atualização, saída e lifecycle. — [x]

### B3 Worker agent

Daemon Linux mínimo. — [x]

### B4 CLI

Primeiros comandos:

```bash
ai-cluster status
ai-cluster nodes
ai-cluster node show <id>
ai-cluster events
```

- [x] `status`;
- [x] `nodes`;
- [ ] `node show <id>`;
- [x] `events`.

Critério de saída: cluster de 2–4 máquinas Linux funciona sem operação manual contínua.

- [~] validado com containers; 2–4 máquinas físicas pendente.

## 4. Fase C — Capability / Health

### C1 Hardware discovery

CPU, RAM, storage, network, accelerators.

- [~] CPU threads e RAM reportados (`capability()` no worker);
- [ ] storage, network e accelerators.

### C2 Benchmark engine

Benchmarks controlados para compute, memory, storage, network.

- [~] benchmark de matriz existe; benchmarks controlados por recurso, não.

### C3 Capability model

Baseline + current + confidence.

- [~] baseline apenas; current e confidence não.

### C4 Health model

Thermal, memory pressure, storage health, network degradation.

- [~] liveness (heartbeat/grace); os demais sinais não.

Critério de saída: adicionar node produz capability profile reproduzível.

- [~] perfil é reportado e visível, mas não é reproduzível por benchmark.

## 5. Fase D — Work Unit Runtime

### D1 Work Unit schema

Criar estado, requirements, dependencies, retries. — [x]

### D2 Local worker execution

Executar operações CPU simples. — [x]

### D3 Result integrity

Checksum, status e observabilidade. — [x]

Critério de saída: uma Work Unit consegue circular entre Controller e Worker e devolver resultado validado. — [x]

## 6. Fase E — Transport

### E1 Transport abstraction

API única:

```text
send
receive
stream
probe
```

- [ ] não existe; o cliente gRPC é usado direto.

### E2 TCP transport

Baseline. — [x] gRPC sobre TCP.

### E3 Shared memory

Otimizar comunicação intra-host. — [ ] não iniciado.

### E4 USB transport

Implementar somente após o protocolo TCP estar estável. — [ ] não iniciado.

### E5 Adaptive transport metrics

Medir bandwidth, latency, loss, congestion. — [ ] não iniciado.

Critério de saída: mesma Work Unit pode usar transportes diferentes sem alterar a lógica da operação. — [ ] não atendido.

## 7. Fase F — Execution Graph

### F1 Graph schema

Nodes, edges, dependencies, barriers. — [x]

### F2 DAG executor

Executar branches em paralelo. — [x]

### F3 Placement abstraction

Separar graph de placement.

- [~] o filtro `min_cpu_threads` é uma decisão de placement, mas está dentro do `claim`
  do store; a abstração não existe.

### F4 Retry / recovery

Reatribuir Work Unit falha. — [x]

Critério de saída: perder um worker não corrompe o DAG. — [x]
(`phase-2/fault-tolerance.md`)

## 8. Fase G — Scheduler

### G1 Candidate filtering

Capabilities, memory, trust, health.

- [x] capabilities (cpu_threads) e health (heartbeat);
- [x] memory: footprint da unidade (`placement::unit_footprint_bytes`) contra a memória que o
  nó reporta, decidido em `placement::fits`. Evidência: `bash code/tests/phase-g-memory.sh`.
- [~] trust: hoje é a exclusão por estado (`registry::eligible` recusa QUARANTINED/FAILED/
  OFFLINE); os níveis de trust do docs/06 dependem do fluxo de enrollment do docs/19, que ainda
  não existe.

### G2 Cost model

Compute + communication + queue + locality. — [x] compute, transferência, fila e saúde em
`code/crates/controller/src/placement.rs`; locality ainda não modelada.

### G3 Global scheduler

Placement. — [x] `WorkStore::claim_placed` rankeia os candidatos ociosos pelo custo; o nó
que não é o melhor cede e a unidade espera o que encaixa melhor, em
`code/crates/controller/src/work.rs` e `service.rs`. `AI_CLUSTER_PLACEMENT=off` volta ao
primeiro-que-pergunta. Evidência: `bash code/tests/phase-g-placement.sh`.

### G4 Local scheduler

Resource queues. — [ ] não iniciado.

### G5 Rebalancing

Threshold + hysteresis. — [ ] não iniciado.

### G6 Reservations

Recursos futuros. — [ ] não iniciado.

Critério de saída: o scheduler prova, por benchmark, que pode escolher placement melhor que round-robin para workloads testados. — [x] atendido: em `bash code/tests/phase-g-placement.sh` a mesma unidade roda no nó fraco sob primeiro-que-pergunta (2.020.000us) e no nó capaz sob placement (18.000us).

## 9. Fase H — Observability

### H1 Metrics

Node, runtime e cluster. — [ ] não iniciado.

### H2 Events

Lifecycle e incidentes. — [x] (`telemetry.md`, `ai-cluster events`)

### H3 Traces

Request → Work Unit → transfer → compute. — [ ] não iniciado.

### H4 TUI

Dashboard em shell. — [ ] não iniciado.

### H5 Expected vs actual

Expor prediction error. — [ ] não iniciado.

Critério de saída: qualquer requisição de teste possui trace completo e métricas suficientes para explicar sua latência. — [ ] não atendido.

## 10. Fase I — State Manager

### I1 State directory

IDs, ownership, replicas.

- [~] IDs e um arquivo de estado (`AI_CLUSTER_STATE`); ownership e replicas não.

### I2 HOT/WARM/COLD

RAM/VRAM, SSD/NVMe, persistent storage. — [ ] não iniciado.

### I3 Checkpoint

Incremental. — [~] snapshot completo da fila, não incremental.

### I4 Recovery

Node loss, replica promotion, recomputation.

- [~] restart do controller recarrega a fila; promoção de réplica não existe.

Critério de saída: execução distribuída recupera-se de falha planejada e de interrupção abrupta em um ambiente controlado. — [~] (`persistence.md` cobre interrupção do controller; falha de nó é requeue)

## 11. Fase J — AI Cluster OS

### J1 Base image

Selecionar base Linux. — [ ] não iniciado.

### J2 Minimal services

Somente componentes necessários. — [ ] não iniciado.

### J3 Bootstrap

Linux existente → enrollment. — [ ] não iniciado.

### J4 Reimage

Instalação limpa. — [ ] não iniciado.

### J5 Boot/recovery

A/B update. — [ ] não iniciado.

### J6 Multi-arch

x86_64 primeiro; ARM64 em seguida. — [ ] não iniciado.

Critério de saída: máquina de teste é reprovisionada automaticamente e retorna como READY. — [ ] não atendido.

## 12. Fase K — Storage Intelligence

### K1 Detection

HDD/SATA SSD/NVMe. — [ ] não iniciado.

### K2 Benchmark

Throughput/latency. — [ ] não iniciado.

### K3 Health

SMART/NVMe health onde suportado. — [ ] não iniciado.

### K4 Dynamic allocation

Swap/model cache/tensor cache/checkpoint/reserve. — [ ] não iniciado.

### K5 Policy adaptation

Reduzir escrita quando health/wear piorarem. — [ ] não iniciado.

Critério de saída: pelo menos HDD e NVMe recebem políticas distintas e mensuráveis. — [ ] não atendido.

## 13. Fase L — Graph Engine

### L1 Graph data model

Hardware, network, knowledge, execution, observability. — [ ] não iniciado.

### L2 Projection

Sincronizar objetos operacionais. — [ ] não iniciado.

### L3 Traversal

Queries de topology/locality/relation. — [ ] não iniciado.

### L4 Retrieval benchmark

Comparar graph-assisted retrieval com baseline para casos relevantes. — [ ] não iniciado.

Critério de saída: só adotar traversal onde houver ganho mensurável. — [ ] não atendido.

## 14. Fase M — Edge Optimizer

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

— [ ] nenhum iniciado.

Critério de saída: demonstrar redução de tokens e/ou latência que supere o custo do próprio optimizer. — [ ] não atendido.

## 15. Fase N — Model Runtime

### N1 Model Registry
### N2 Manifest
### N3 Variant/precision
### N4 Sharding
### N5 Model cache
### N6 CPU inference
### N7 Pipeline parallelism
### N8 Tensor parallelism
### N9 Hybrid planner

— [ ] nenhum iniciado.

Critério de saída: modelo robusto executa em múltiplos nodes CPU e todo overhead é mensurado. — [ ] não atendido.

## 16. Fase O — Accelerators

Ordem sugerida:

```text
CPU x86_64
 → CPU ARM64
 → NVIDIA GPU
 → AMD/Intel GPU
 → NPU abstraction
 → mobile NPU
```

Cada backend precisa passar por capability discovery e benchmark próprio. — [ ] não iniciado.

## 17. Fase P — Adaptive Runtime

### P1 Performance learning
### P2 Predictive placement
### P3 Prefetch
### P4 Speculative execution
### P5 Capacity inference
### P6 Automated rebalancing

— [ ] nenhum iniciado.

Critério de saída: previsão melhora estatisticamente em workload repetido e o sistema consegue provar expected vs actual. — [ ] não atendido.

## 18. Fase Q — Mobile Worker

### Q1 Supported hardware matrix
### Q2 Bootloader/reimage path
### Q3 ARM64 image
### Q4 USB transport
### Q5 accelerator backend
### Q6 thermal/battery constraints

— [ ] nenhum iniciado.

Um mobile node deve ser considerado apenas se seu valor computacional líquido, após comunicação e limitações térmicas, for positivo.

## 19. Critérios de gate entre fases

Cada fase deve produzir:

```text
código
benchmarks
telemetria
testes de falha
atualização de documentação
atualização de ADR quando a arquitetura mudar
```

Não avançar uma fase simplesmente porque o código compila.

## 20. Primeiro alvo experimental

O primeiro experimento não deve ser um LLM.

Usar:

```text
2–4 nós Linux
CPU-only
1 GbE
workload de matriz/DAG
```

Medir:

```text
nó único
2 nós
3 nós
4 nós
```

E comparar:

```text
tempo de compute
tempo de comunicação
latência total
throughput
previsto
real
```

A primeira pergunta experimental é:

> Quando a comunicação é adicionada, em quais tamanhos de Work Unit o hardware adicional produz ganho líquido?

Esse resultado orienta todo o restante da plataforma.

- [x] executado — `phase-0/benchmark-sweep.md`: 1/2/3 nós × tamanhos 32–256. Dentro do teto
  de mensagem do gRPC, a transferência ainda empata ou supera o compute; o crossover fica
  acima de 256 e exige teto maior ou outro transporte. A varredura também achou o gargalo
  real daquele momento: o poll do worker compartilhava o ticker do heartbeat (5s), o que
  fixava a parede em ~36s para 6 units. Corrigido em `3caa75d`.
