---
title: "AI Cluster — Arquitetura"
aliases: ["ARCHITECTURE", "AI Cluster — Architecture", "Arquitetura", "docs/ARCHITECTURE"]
tags: [sistema/arquitetura, moc]
type: sistema/arquitetura
status: decidido
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-01-nucleo-do-cluster]]", "[[fase-02-capability-e-saude]]", "[[fase-03-execucao-distribuida]]", "[[fase-04-escalonamento]]", "[[fase-05-observabilidade]]", "[[fase-06-estado-e-recuperacao]]", "[[fase-07-seguranca-base]]", "[[fase-08-inferencia]]", "[[fase-09-transporte]]", "[[fase-10-inteligencia-de-storage]]", "[[fase-11-request-manager]]", "[[fase-12-model-runtime]]", "[[fase-13-graph-engine]]", "[[fase-14-edge-optimizer]]", "[[fase-15-ai-cluster-os]]", "[[fase-16-aceleradores]]", "[[fase-17-runtime-adaptativo]]", "[[fase-18-worker-movel]]", "[[fase-19-hardening-de-producao]]"]
---

# AI Cluster — Arquitetura

Voltar para [[system]]. Este é o **documento mestre** da arquitetura-alvo. Os subsistemas são detalhados em [[subsistemas]]; as decisões em [[decisoes]]; o estado de cada decisão em [[estado-das-decisoes]].

Baseline DECIDIDO, v0.1.

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

O AI Cluster é uma infraestrutura de computação. Agentes, aplicações e orquestradores consomem sua capacidade exclusivamente através da Inference API ([[inference-api]], [[adr-007-fronteira-inference-api]]).

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

A interface administrativa permanece separada: CLI/TUI e Management API são destinadas à operação da infraestrutura ([[cli-tui]]).

> [!warning] Desvio do MVP: o gateway executa o modelo
> Hoje o gateway (`code/crates/gateway`) roda o modelo dentro do próprio processo, fora do cluster, e escolhe os nós de execução pelas variáveis de ambiente `AI_CLUSTER_GPT2_SHARDS` e `AI_CLUSTER_SMOLLM2_SHARDS`. No alvo, o gateway apenas traduz a API; a admissão passa pelo Request Manager e a escolha de nós pelo Planner e Scheduler. Migração: [[fase-11-request-manager]] (admissão) e [[fase-12-model-runtime]] (modelo e placement).

## 4. Request Manager

Entre a Inference API e o motor distribuído existe um Request Manager. Ele controla admissão, quotas, fila, orçamento de tokens/recursos, cancelamento e ciclo de vida da Request ([[request-manager]]).

```text
Client -> Inference API -> Request Manager -> Planner -> Scheduler -> Workers
```

O Request Manager decide se a request pode entrar e em quais condições. O Scheduler continua responsável pelo placement e uso dos recursos concretos.

> [!todo] Não implementado
> Não há Request Manager. A admissão é feita diretamente pelo gateway, sem fila, quota ou orçamento. Ver [[fase-11-request-manager]].

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

Cada node deve operar o mesmo runtime lógico em Linux, com imagens específicas por arquitetura/hardware apenas quando necessário ([[ai-cluster-os]]).

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

O OS é imutável ou controlado por imagens versionadas. O ingresso inicial reimaging deve poder apagar o sistema anterior e instalar uma imagem limpa ([[adr-002-os-imutavel]]).

> [!todo] Não implementado
> A imagem imutável, o bootstrap e o reimage ainda não existem. Ver [[fase-15-ai-cluster-os]].

## 8. Provisionamento

Fluxo previsto ([[provisionamento]]):

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

PCs e servidores podem utilizar PXE/iPXE. Dispositivos móveis dependem de bootloader/firmware capazes de aceitar a estratégia de reimagem escolhida ([[fase-18-worker-movel]]).

## 9. Modelo de execução

A unidade abstrata de trabalho é a `WorkUnit` ([[work-unit]]). O fluxo é:

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

- **WHAT** — Execution Graph ([[execution-graph]]).
- **WHERE** — Placement Plan ([[execution-planner]], [[scheduler]]).
- **HOW** — Worker Runtime ([[worker-runtime]]).

## 10. Grafos

O sistema terá pelo menos:

1. Knowledge Graph.
2. Hardware/Network Graph.
3. Model Graph.
4. Execution Graph.
5. Dependency Graph.
6. Observability/Incident Graph.

O grafo é uma projeção operacional e/ou índice relacional de objetos do sistema. A perda do grafo não deve destruir a fonte de verdade primária ([[adr-004-grafo-como-projecao]], [[modelo-de-grafo]], [[graph-engine]]).

## 11. Memória e storage

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

Em SSD/NVMe saudáveis, o sistema deve aproveitar o armazenamento para swap, model cache, tensor cache, checkpoint e staging, respeitando reserva operacional e desgaste. Swap é fallback de memória; cache é recurso de desempenho e deve ser tratado separadamente ([[storage-intelligence]], [[adr-005-camadas-de-storage]]).

## 12. Hot-plug

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

A topologia lógica da execução pode continuar enquanto o placement é substituído ([[hotplug-e-recuperacao]], [[adr-006-hotplug]]).

## 13. Capacidade

Capacidade não deve ser um único score absoluto. O perfil é multidimensional ([[capability-engine]]):

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

## 14. Modelo de desempenho

A previsão deve considerar, no mínimo ([[modelo-de-desempenho]]):

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

## 15. Inteligência de edge

Antes de inferência pesada, o sistema deve tentar ([[edge-optimizer]]):

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

## 16. Observabilidade

Métricas, eventos e traces são separados ([[observabilidade]]).

O node terá estados de saúde, capacidade e confiança independentes ([[saude-e-confianca]]).

Exemplo:

```text
Health: DEGRADED
Capacity: HIGH
Trust: TRUSTED
State: ACTIVE
```

## 17. Pilha tecnológica

A pilha inicial segue o [[adr-008-stack-inicial]]. São escolhas de implementação do primeiro protótipo, não contratos irreversíveis.

| Camada | Escolha | Estado |
|---|---|---|
| Linguagem | Rust, edition 2024 | Decidido; implementado em `code/` |
| Runtime assíncrono | tokio | Decidido; implementado |
| RPC do control plane | gRPC sobre HTTP/2 (tonic 0.14) | Decidido; implementado |
| Schema | Protocol Buffers (prost 0.14, tonic-prost); `protoc` vendorizado no build (`protoc-bin-vendored`) | Decidido; implementado (`code/proto/cluster.proto`) |
| TLS | rustls via tonic (`tls-ring`) | Implementado em [[fase-07-seguranca-base]]; mTLS em [[fase-19-hardening-de-producao]] |
| CLI | clap 4 | Decidido; implementado |
| TUI | Dashboard de texto no CLI (`dashboard --watch`); TUI full-screen com ratatui | Dashboard implementado; ratatui planejado |
| Base Linux | Distribuição a definir | Proposto ([[adr-001-base-linux]]) |
| Transporte | TCP/Ethernet, USB, RDMA futuro | gRPC/TCP implementado; abstração em [[fase-09-transporte]] |
| Metadados | banco relacional | Proposto; hoje snapshot em arquivo (`AI_CLUSTER_STATE`) |
| Métricas temporais | time-series | Proposto; hoje roll-up em memória no controller |
| Artefatos grandes | object storage | Proposto; não implementado |
| Grafo | projeção, não necessariamente a única base | Decidido ([[adr-004-grafo-como-projecao]]); tecnologia aberta |

> [!warning] Desvio do MVP: estado em arquivo
> O controller persiste o estado em um arquivo de snapshot (`AI_CLUSTER_STATE`) e não em banco relacional. A escolha definitiva de storage de metadados segue aberta ([[estado-das-decisoes]]).

Arquiteturas-alvo: x86_64 e ARM64 ([[fase-16-aceleradores]]). O núcleo do protocolo é neutro a arquitetura ([[principios]], P-013).

> [!info] O que já existe
> Resumo verificado em `code/`; detalhes em cada subsistema.
> - Controller com registry, placement por custo (`code/crates/controller/src/placement.rs`), reservas, eventos, métricas e exporter: [[f04-placement]], [[f04-reservas]], [[f05-eventos]], [[f05-metricas]].
> - Worker com execução de Work Units de matriz, checksum do resultado e classe de armazenamento por dispositivo (HDD como COLD, SSD/NVMe como WARM): [[f02-capability-corrente]], [[f10-classes-de-armazenamento]].
> - Protocolo gRPC com `ShardChannel`, `ShardMatmul`, `ProbeLink`, `GetTrace` e `Reserve`: [[protocolo-controller-worker]], [[canal-de-fatias]].
> - Gateway com chave de API e rotas `/v1/models`, `/v1/embeddings` e `/v1/chat/completions`: [[f08-gateway-e-api]].
> - Inference Engine em CPU para GPT-2 small e SmolLM2-135M, com paridade contra `transformers` e KV cache: [[f08-engine-local]], [[f08-chat-completions]].
> - CLI com comandos operacionais e dashboard: [[cli-tui]], [[f05-dashboard]].

> [!todo] O que falta
> - Request Manager ([[fase-11-request-manager]]).
> - Model Runtime distribuído: registry, shards por variante, pipeline/tensor/híbrido ([[fase-12-model-runtime]]).
> - Graph engine e Edge optimizer ([[fase-13-graph-engine]], [[fase-14-edge-optimizer]]).
> - AI Cluster OS, provisionamento e reimage ([[fase-15-ai-cluster-os]]).
> - Aceleradores, worker móvel, runtime adaptativo e hardening ([[fase-16-aceleradores]], [[fase-17-runtime-adaptativo]], [[fase-18-worker-movel]], [[fase-19-hardening-de-producao]]).
> - Storage inteligente completo: alocação dinâmica de HOT/WARM/COLD ([[fase-10-inteligencia-de-storage]]).

## Navegação

- [[estado-das-decisoes]] — decidido, proposto, experimental e aberto.
- [[visao]] — visão, princípios e requisitos.
- [[subsistemas]], [[protocolo]], [[modelo-de-dados]], [[decisoes]] — os demais MOCs da área de sistema.
