---
title: "AI Cluster — Arquitetura"
aliases: ["ARCHITECTURE", "AI Cluster — Architecture", "Arquitetura", "docs/ARCHITECTURE"]
tags: [sistema/arquitetura, moc]
type: sistema/arquitetura
---

# AI Cluster — Arquitetura

Voltar para [[system]]. Este é o **documento mestre** da arquitetura. Os subsistemas são detalhados em [[subsistemas]]; as decisões em [[decisoes]]; o estado das decisões em [[estado-das-decisoes]].

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

> [!warning] Limite atual: o gateway executa o modelo
> O gateway (`code/crates/gateway`) roda o modelo dentro do próprio processo, fora do cluster. As projeções podem ser enviadas aos workers, com os nós escolhidos pelas variáveis `AI_CLUSTER_GPT2_SHARDS` e `AI_CLUSTER_SMOLLM2_SHARDS`. No alvo, o gateway apenas traduz a API; a escolha de nós passa pelo Planner e pelo Scheduler. Planejado: separar a engine do gateway ([[inference-engine]]).

## 4. Request Manager

Entre a Inference API e o motor distribuído existe um Request Manager. Ele controla admissão, quotas, fila, orçamento de tokens/recursos, cancelamento e ciclo de vida da Request ([[request-manager]]).

```text
Client -> Inference API -> Request Manager -> Planner -> Scheduler -> Workers
```

O Request Manager decide se a request pode entrar e em quais condições. O Scheduler continua responsável pelo placement e uso dos recursos concretos.

> [!warning] Limite atual
> A admissão está no gateway: chave com tenant, cota de tokens por janela, fila por classe com envelhecimento e cancelamento do grafo ([[adr-015-request-manager-no-gateway]]). Cota, fila e contadores ficam na memória do gateway e não são compartilhados entre gateways. Planejado: Request Manager como serviço próprio, estados de requisição, idempotência, orçamento de recursos e cancelamento de unidades em execução.

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

O OS é imutável ou controlado por imagens versionadas. A instalação inicial apaga o sistema anterior e grava uma imagem limpa ([[adr-002-os-imutavel]]).

> [!warning] Limite atual
> Hoje existe imagem A/B imutável para armv7 (raiz somente leitura, dois slots, boot de rede só para instalação e recuperação; [[adr-017-instalacao-a-b-e-boot-de-rede]]). Atualizações de versão são distribuídas pelo controller em canário, com release assinada e conferida no nó ([[adr-018-release-assinada-e-medida-do-slot]]). Planejado: imagens para x86_64 e ARM64 e instalador próprio para cada uma.

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

PCs e servidores podem utilizar PXE/iPXE. Dispositivos móveis dependem de bootloader/firmware capazes de aceitar a estratégia de reimagem escolhida ([[provisionamento]]).

> Planejado: PXE/iPXE para PCs e servidores; reimage completo disparado pelo controller; provisionamento de dispositivos móveis.

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

> Hoje o controller tem um único grafo: o DAG de dependências das Work Units de um grafo de execução. Planejado: os demais grafos, com projeção e consultas ([[graph-engine]]).

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

> [!warning] Limite atual
> Cada worker classifica o próprio armazenamento a partir de `/sys/block` (NVMe/SSD como WARM, HDD como COLD). Planejado: política de cache e swap em SSD/NVMe com limites de endurance, leitura de SMART e alocação dinâmica entre HOT, WARM e COLD.

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

> [!warning] Limite atual
> Entrada e saída de nós funcionam, e a falha de um worker reenfileira as unidades dele. A recuperação do estado por chave (KV cache, estado de modelo) não existe. Planejado: hot-plug de dispositivos (USB, aceleradores) e recuperação de estado por chave.

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

> Hoje o perfil reporta compute, memória, velocidade do link, saúde térmica e de memória e classe de armazenamento. Planejado: memória sob carga, latência, aceleradores, GPU e NPU, e scores por workload.

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

> Hoje o roll-up compara previsão e execução real, e cada nó aprende a razão entre elas para corrigir as previsões seguintes. Planejado: calibração que vá além dessa razão e planejador aprendido.

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

> Hoje existe só o cache exato de respostas, no gateway. Planejado: as demais etapas da cadeia acima.

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

A pilha inicial segue o [[adr-008-stack-inicial]]. São escolhas do primeiro protótipo, não contratos irreversíveis.

| Camada | Escolha | Decisão |
|---|---|---|
| Linguagem | Rust, edition 2024 | decidido |
| Runtime assíncrono | tokio | decidido |
| RPC do control plane | gRPC sobre HTTP/2 (tonic 0.14) | decidido |
| Schema | Protocol Buffers (prost 0.14, tonic-prost); `protoc` vendorizado no build; mensagens em `code/proto/cluster.proto` | decidido |
| TLS | rustls via tonic; mTLS opcional | decidido ([[adr-016-mtls-opcional-no-plano-de-controle]]) |
| CLI | clap 4 | decidido |
| TUI | dashboard de texto no CLI (`dashboard --watch`); TUI full-screen com ratatui | dashboard decidido; ratatui planejado |
| Base Linux | distribuição a definir | proposto ([[adr-001-base-linux]]) |
| Transporte | TCP/Ethernet, USB, RDMA futuro | gRPC/TCP em uso; abstração em [[camada-de-transporte]] ([[adr-014-abstracao-de-transporte]]) |
| Metadados | banco relacional | proposto; hoje snapshot em arquivo |
| Métricas temporais | time-series | proposto; hoje roll-up em memória no controller |
| Artefatos grandes | object storage | proposto |
| Grafo | projeção, não necessariamente a única base | decidido ([[adr-004-grafo-como-projecao]]); tecnologia aberta |

> [!warning] Limite atual: estado em arquivo
> O controller persiste o estado em um arquivo de snapshot (`AI_CLUSTER_STATE`), não em banco relacional. Os eventos vão para outro arquivo (`AI_CLUSTER_EVENTS`). A escolha definitiva de armazenamento de metadados segue aberta ([[estado-das-decisoes]]).

Arquiteturas de hardware: o protocolo é neutro a arquitetura ([[principios]], P-013). Hoje o código roda em x86_64 e armv7 (32 bits). Planejado: ARM64 e aceleradores.

## Navegação

- [[estado-das-decisoes]] — decisões vigentes, propostas, experimentais e questões em aberto.
- [[visao]] — visão, princípios e requisitos.
- [[subsistemas]], [[protocolo]], [[modelo-de-dados]], [[decisoes]] — os demais MOCs da área de sistema.
