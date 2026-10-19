---
title: "AI Cluster — Architecture"
type: system
status: decided
tags: [system, architecture]
---

# AI Cluster — Architecture

Status: DECIDED baseline / v0.1

## 1. Objetivo

Construir um **Distributed AI Runtime** que abstraia hardware heterogêneo como uma capacidade computacional lógica, mantendo execução, estado, modelos, comunicação e observabilidade desacoplados do tipo físico do equipamento.

O sistema não deve ser desenhado apenas como “LLM cluster”. LLM é um workload. O runtime deve suportar, no mesmo modelo de execução, workloads como embeddings, reranking, classificação, compressão, visão, áudio e tarefas computacionais auxiliares.

## 2. Arquitetura de alto nível

```text
                         AI CLUSTER
                              |
                         CONTROLLER
                              |
       +----------------------+----------------------+
       |                      |                      |
       v                      v                      v
 Edge Intelligence      Execution Planner       State Manager
       |                      |                      |
       +----------------------+----------------------+
                              |
                          Scheduler
                              |
               +--------------+--------------+
               |              |              |
               v              v              v
            Worker         Worker         Worker
               |              |              |
             CPU            GPU            NPU
               +--------------+--------------+
                              |
                         Transport
                              |
                         Telemetry
                              |
                    Performance Model
                              |
                              +----> Planner
```

## 3. Application Plane e Inference API

O AI Cluster é uma infraestrutura de computação. Agentes, aplicações e orquestradores consomem sua capacidade exclusivamente através da Inference API.

```text
Application Plane
  |
  v
Inference API
  |
  v
AI Cluster
```

Nenhum consumidor externo acessa diretamente Controller, Worker, shards, tensors ou estado interno. A API abstrai completamente a topologia interna.

A interface administrativa permanece separada: CLI/TUI e Management API são destinadas à operação da infraestrutura.

## 4. Request Manager

Entre a Inference API e o motor distribuído existe um Request Manager. Ele controla admissão, quotas, fila, orçamento de tokens/recursos, cancelamento e ciclo de vida da Request.

```text
Client -> Inference API -> Request Manager -> Planner -> Scheduler -> Workers
```

O Request Manager decide se a request pode entrar e em quais condições. O Scheduler continua responsável pelo placement e uso dos recursos concretos.

## 5. Control Plane

Responsável por:

- cluster membership;
- identidade e confiança;
- provisioning;
- capabilities;
- scheduling;
- modelos e versões;
- estado lógico das execuções;
- health;
- observabilidade;
- políticas.

## 6. Data Plane

Responsável por:

- execução de Work Units;
- operações de tensor;
- movimentação de buffers;
- cache local;
- acesso a RAM/VRAM/NPU memory;
- execução de kernels;
- checkpoint e restore.

## 7. AI Cluster OS

Cada node deve operar o mesmo runtime lógico em Linux, com imagens específicas por arquitetura/hardware apenas quando necessário.

Estrutura conceitual:

```text
AI Cluster OS
|- Linux kernel
|- boot/recovery
|- Hardware Abstraction Layer
|- Cluster Agent
|- Worker Runtime
|- Telemetry Agent
|- Health Monitor
|- Storage Intelligence
`- Security / Identity
```

O OS é imutável ou controlado por imagens versionadas. O ingresso inicial reimaging deve poder apagar o sistema anterior e instalar uma imagem limpa.

## 8. Provisionamento

Fluxo previsto:

```text
Existing Linux / bootstrap
 -> discovery
 -> enrollment
 -> trust validation
 -> hardware benchmark
 -> select OS image
 -> full reimage
 -> reboot
 -> AI Cluster OS
 -> validation
 -> runtime benchmark
 -> READY
```

PCs e servidores podem utilizar PXE/iPXE. Dispositivos móveis dependem de bootloader/firmware capazes de aceitar a estratégia de reimagem escolhida.

## 7. Modelo de execução

A unidade abstrata de trabalho é a `WorkUnit`. O fluxo é:

```text
Request
 -> Edge Optimization
 -> Model Graph
 -> Execution Graph
 -> Execution Planner
 -> Placement Plan
 -> Scheduler
 -> WorkUnits
 -> Worker Runtime
 -> State / Transport
 -> Telemetry
 -> Performance Model
 -> Planner
```

A separação fundamental é:

- **WHAT** — Execution Graph.
- **WHERE** — Placement Plan.
- **HOW** — Worker Runtime.

## 8. Grafos

O sistema terá pelo menos:

1. Knowledge Graph.
2. Hardware/Network Graph.
3. Model Graph.
4. Execution Graph.
5. Dependency Graph.
6. Observability/Incident Graph.

O grafo é uma projeção operacional e/ou índice relacional de objetos do sistema. A perda do grafo não deve destruir a fonte de verdade primária.

## 9. Memória e storage

A hierarquia padrão é:

```text
HOT  = RAM / VRAM / accelerator memory
WARM = NVMe / SSD
COLD = storage remoto / HDD / objeto persistente
```

O AI Cluster OS deve detectar automaticamente:

- HDD;
- SATA SSD;
- NVMe;
- capacidade;
- desempenho;
- latência;
- saúde;
- temperatura;
- desgaste/endurance.

Em SSD/NVMe saudáveis, o sistema deve aproveitar o armazenamento para swap, model cache, tensor cache, checkpoint e staging, respeitando reserva operacional e desgaste. Swap é fallback de memória; cache é recurso de desempenho e deve ser tratado separadamente.

## 10. Hot-plug

Entrada:

```text
DISCOVERED
 -> PROVISIONING
 -> VALIDATING
 -> BENCHMARKING
 -> READY
 -> ACTIVE
```

Saída planejada:

```text
ACTIVE
 -> DRAINING
 -> STATE/MODEL MIGRATION
 -> OFFLINE
```

Falha:

```text
ACTIVE
 -> heartbeat lost
 -> FAILED
 -> state recovery
 -> replan
```

A topologia lógica da execução pode continuar enquanto o placement é substituído.

## 11. Capacidade

Capacidade não deve ser um único score absoluto. O perfil é multidimensional:

```text
compute
memory
memory bandwidth
gpu
npu
network
latency
storage
thermal
health
```

Scores derivados podem ser específicos do workload:

```text
LLM_INT4
LLM_INT8
FP16
MATMUL
EMBEDDING
COMPRESSION
```

O desempenho real corrige o benchmark inicial.

## 12. Performance Model

A previsão deve considerar, no mínimo:

```text
Ttotal = Tcompute + Tcommunication + Tserialization + Tmemory + Tqueue
```

O modelo aprende com execuções reais.

O cluster deve expor:

- estimated capacity;
- actual capacity;
- prediction error;
- expected latency;
- actual latency;
- expected throughput;
- actual throughput.

## 13. Edge Intelligence

Antes de inferência pesada, o sistema deve tentar:

```text
classify
 -> exact cache
 -> semantic cache
 -> graph retrieval
 -> context selection
 -> deduplication
 -> compression
 -> skill/tool resolution
 -> prompt compilation
 -> token budgeting
 -> model routing
```

O princípio é maximizar **useful work**, não simplesmente tokens/second.

## 14. Observabilidade

Métricas, eventos e traces são separados.

O node terá estados de saúde, capacidade e confiança independentes.

Exemplo:

```text
Health: DEGRADED
Capacity: HIGH
Trust: TRUSTED
State: ACTIVE
```

## 15. Dependências tecnológicas iniciais

PROPOSED:

- Linux como base do AI Cluster OS.
- x86_64 e ARM64 como primeiras arquiteturas.
- gRPC + Protobuf para control plane.
- transporte abstrato com TCP/Ethernet, USB e possibilidade futura de RDMA.
- TUI baseada em shell.
- armazenamento de metadata em banco relacional.
- armazenamento temporal para métricas.
- object storage para artefatos grandes.
- camada de grafo como projeção, não necessariamente como única base.

Essas escolhas devem ser validadas durante o MVP.

## 16. Estratégia de desenvolvimento

O projeto deve começar com um cluster mínimo CPU-only, validando:

1. discovery;
2. provisioning;
3. identity;
4. heartbeat;
5. benchmark;
6. Work Unit;
7. transport;
8. scheduler;
9. telemetry;
10. hot-plug;
11. execução matemática distribuída.

Somente depois devem entrar LLM, quantização, GPU/NPU e dispositivos móveis.
