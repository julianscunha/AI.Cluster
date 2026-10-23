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
- [x] `node show <id>` — detalhe de um nó: capability e saúde completas, o que a tabela de
  `nodes` deixa de fora, mais a reserva do nó. Evidência: `bash code/tests/phase-b4-node-show.sh`.
- [x] `events`.

Critério de saída: cluster de 2–4 máquinas Linux funciona sem operação manual contínua.

- [~] validado com containers; 2–4 máquinas físicas pendente.

## 4. Fase C — Capability / Health

### C1 Hardware discovery

CPU, RAM, storage, network, accelerators.

- [~] CPU threads, RAM e compute medido (`capability()` no worker);
- [~] storage por bloco — NVMe/SSD/HDD por nó (`storage-tiers.md`, I2);
- [~] network — velocidade nominal do link sobre a referência de 1 GbE
  (`network-discovery.md`);
- [ ] accelerators — sem caminho de execução no worker, então nada a reportar (fase J).

### C2 Benchmark engine

Benchmarks controlados para compute, memory, storage, network.

- [~] compute medido no worker (`phase-2/capability-benchmark.md`): o worker cronometra o
  matmul no próprio kernel das unidades e reporta a taxa por core em `compute_score`, que o
  modelo de custo já lia;
- [~] rede medida: sonda `ProbeLink` de upload ao controller no registro, em
  `network_achieved_score`; o placement usa o menor entre ela e o nominal
  (`network-discovery.md`). Só upload, uma vez, sem latência;
- [~] storage: escrita sequencial com fsync em `storage_score` (`storage-tiers.md`, K2); só o
  dispositivo do tmp, sem consumidor;
- [ ] memory não é medida sob carga.

### C3 Capability model

Baseline + current + confidence.

- [x] baseline + current + confidence (`capability-current.md`): `compute_score` continua sendo
  o baseline do registro, e `current_compute_score` / `compute_confidence` carregam a última
  remedida — tirada no ramo **ocioso** do laço de trabalho, porque o benchmark de 256³
  disputaria CPU com uma unidade em voo e inflaria o compute que o H5 compara. O perfil passou
  a viajar no `HealthStreamRequest.capability`, então o controller vê o nó mudar sem
  re-registro. O ranking do placement segue no baseline e só `predicted_compute_us` lê o
  current: prever tempo e escolher nó são perguntas diferentes. Zero é "não remedido" (nunca
  "compute zero") e a confiança é concordância entre as execuções da passada (melhor sobre a
  mediana), não histórico. O benchmark reporta também a **distribuição do registro** —
  `compute_median_score` (o valor central, contra o qual a previsão de tempo é calibrada)
  e `compute_deviation` (o quanto as execuções discordaram) — porque calibrar contra
  o melhor caso é ajustar contra um número que a máquina raramente repete
  (`benchmark-stability.md`). Evidência: `bash code/tests/phase-c-current.sh` (baseline 1.07 /
  mediana 0.46 / desvio 0.41 / current 0.72 / confiança 0.71; com o score fixado, current e
  mediana ficam `-`).

### C4 Health model

Thermal, memory pressure, storage health, network degradation.

- [x] liveness (heartbeat/grace) e o score de saúde com pressão de memória, swap e térmica
  (`health-model.md`). O placement já usava o score como penalidade, então o escalonador
  passou a evitar nó quente sem mudança no modelo de custo. Evidência:
  `bash code/tests/phase-c4-health.sh` (dois nós de capability idêntica, só a temperatura
  diferente; a unidade vai para o frio).
- [~] degradação de rede: a sonda é repetida com o nó ocioso (`current_network_score`) e o
  placement lê a atual no lugar do baseline (`network-discovery.md`). Sem E2E que degrade o link;
- [ ] storage health — sem baseline de dispositivo (o análogo do `capability-benchmark.md`).

Critério de saída: adicionar node produz capability profile reproduzível.

- [~] o compute do perfil é reproduzível por benchmark (`phase-2/capability-benchmark.md`);
  memory, storage, network e kernels de acelerador ainda não são medidos.

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

- [x] a política de ranking saiu do `WorkStore::claim_placed` para `placement::decide`, uma
  função pura (candidatos ociosos, reserva que supera custo, banda de histerese). O store
  mantém o lock, a fila e o despacho e passa duas consultas (`in_flight`, `held`), então a
  decisão atômica não mudou. `fits` (incl. `min_cpu_threads`) já vivia em `placement.rs`.
  Sem trait de política: há uma só. Coberto por 3 testes unitários de `decide`; os de
  `work.rs` e `phase-g-placement`/`phase-g-reservations` passam sem alteração.

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

Resource queues. — [~] o scheduler local concede os cores (`min` das threads reportadas e
do host, nunca zero) e o worker executa a unidade nesse número de threads, em
`worker::core_budget` e `Matrix::multiply_with_threads`. Filas de recursos, prioridades e
GPU/NPU não. Evidência: `bash code/tests/phase-g-cores.sh` (~7x menos compute com 4 cores
que com 1, mesmo checksum).

### G5 Rebalancing

Threshold + hysteresis. — [x] o nó só cede a unidade quando outro candidato ocioso é mais
barato por mais que a banda de histerese (`AI_CLUSTER_PLACEMENT_TOLERANCE`, padrão 0.10),
em `service::placement_tolerance` e `WorkStore::claim_placed`. Ceder custa até um intervalo
de poll, então recusar ceder por diferença de arredondamento troca ociosidade certa por
economia incerta; a banda também substitui o antigo desempate por id. Coberto por
`a_near_equal_node_takes_the_unit_instead_of_yielding`,
`a_clearly_worse_node_still_yields_with_the_band_on`,
`a_zero_band_means_strictly_cheapest`, `a_wide_band_lets_a_worse_node_take_the_unit` e
`clamps_the_placement_tolerance_band`. Rebalanceamento de trabalho já em voo (mover uma
unidade de um nó sobrecarregado) não existe — as unidades são curtas e voltam à fila.

### G6 Reservations

Recursos futuros. — [x] (`reservations.md`): `Reserve`/`ListReservations`/`ReleaseReservation`
retêm um nó para uma requisição futura. A unidade da requisição dona roda no nó retido mesmo
que ele seja o nó pior pelo cost model, e o nó retido recusa unidades de outras requisições
— nos dois caminhos de despacho, inclusive com `AI_CLUSTER_PLACEMENT=off`, porque a retenção
é propriedade do nó e não da política. Um nó, uma reserva; TTL vencendo libera, e o
vencimento é aplicado na leitura, não por timer. Não persiste além de um restart e retém o
nó inteiro, não uma fração da capacidade. Na CLI: `ai-cluster reserve|reservations|release` e
`ai-cluster benchmark --request-id` para enfileirar sob o id reservado. Evidência:
`bash code/tests/phase-g-reservations.sh` (nó fraco retido roda o trabalho do dono; nó forte
retido derruba a unidade de outra requisição para o fraco).

Critério de saída: o scheduler prova, por benchmark, que pode escolher placement melhor que round-robin para workloads testados. — [x] atendido: em `bash code/tests/phase-g-placement.sh` a mesma unidade roda no nó fraco sob primeiro-que-pergunta (2.020.000us) e no nó capaz sob placement (18.000us).

## 9. Fase H — Observability

### H1 Metrics

Node, runtime e cluster. — [x] (`metrics.md`): `GetMetrics` devolve nós totais/disponíveis,
fila (pendentes/bloqueadas/falhas), tempos de compute e latência somados e médios **e** os
percentis p50/p95 dos dois, com `ai-cluster metrics`. O worker passou a reportar CPU, swap,
temperatura e IOPS no `HealthReport` (antes só pressão de memória) e `ai-cluster nodes` mostra
as colunas — zero ali é "não medido", não "zero". O exportador Prometheus serve o mesmo roll-up
mais uma série por nó em `GET /metrics` (`AI_CLUSTER_METRICS_ADDR`, padrão `0.0.0.0:9101`), com
o tipo declarado uma vez por métrica. Rede, storage score e métricas de LLM não. Evidência:
`bash code/tests/phase-h-metrics.sh` (ocioso → 2 unidades → percentis → nós → `/metrics`).

### H2 Events

Lifecycle e incidentes. — [x] (`telemetry.md`, `ai-cluster events`)

### H3 Traces

Request → Work Unit → transfer → compute. — [x] (`traces.md`): o `request_id` liga a
requisição do cliente às unidades que ela gerou, `GetTrace` devolve estado, nó e tempos de
cada uma, e `ai-cluster request trace <id>` mostra a divisão de cada unidade entre compute e
transferência. Cobre a execução atual; persistir a correlação além de um restart fica junto da
persistência do work store.

### H4 TUI

Dashboard em shell. — [x] (`dashboard.md`): `ai-cluster dashboard` lê `ListNodes`, `GetMetrics`
e `ListReservations` e imprime numa vista só os nós (estado, threads, RAM, compute, saúde), a
fila de trabalho (concluídas/pendentes/bloqueadas/falhas e tempos) e as reservas ativas. Sem
`--watch` imprime uma vez e sai, para um script poder ler; com `--watch N` repinta a cada N
segundos até Ctrl-C. Os campos do painel da spec sem fonte ainda — TTFT, ITL, tokens/s, tokens
evitados e KV cache — não aparecem: zerá-los diria que valem zero. "Previsto vs real" passou a
ter fonte: o `GetMetrics` soma a previsão (H1) e a linha `TEMPO` mostra `previsto_total_us` ao
lado do compute real. Evidência: `bash code/tests/phase-h-dashboard.sh` (ocioso → 2 unidades →
reserva ativa).

### H5 Previsto vs real

Expor prediction error. — [x] (`predictions.md`): `predicted_compute_us` no `TraceEntry` e
`placement::predicted_compute_us` fazem o modelo de custo prever **tempo**, não só ordenar —
`REFERENCE_CORE_GFLOPS` passou a viver em `ai_cluster_common::matrix` para o controlador
inverter a normalização do worker sem duas cópias do mesmo número. O trace mostra
`PREDITO_US` por unidade e uma linha `PREVISTO somado_us=... erro_pct=...`; zero é lido como
"não previsto", nunca como previsão de zero µs. O erro medido era grande e honesto: ~32× de
subestimação em unidades pequenas (64³), porque o score é calibrado single-core num kernel de
256³ — e a fatia de estabilidade mostrou que parte do erro era o número que alimentava a
previsão, não o modelo. Duas correções vieram daí: a previsão usa a **taxa típica**
(`compute_median_score / compute_score`, guardada por `compute_deviation`) em vez do melhor caso,
e soma o **setup por unidade** (`UNIT_SETUP_US`, ajustável por `AI_CLUSTER_UNIT_SETUP_US`). O
`erro_pct` do trace caiu de ~3200% para poucos por cento (`erro_pct=-3.8` numa requisição real de
64³); bandas por faixa de tamanho ficam para quando o desvio mostrar que a medição estabilizou.
Evidência: `bash code/tests/phase-h-predictions.sh` (622us previstos por unidade de 64³, 775us
para uma de 128³ — score fixado) e `bash code/tests/phase-h-traces.sh` (a coluna no trace de uma
requisição real). O roll-up do cluster agora soma a previsão:
`MetricsResponse.total_predicted_compute_us` (H1) e a linha `TEMPO ... previsto_total_us=` do
painel (H4) leem o mesmo número — o mesmo script confere `total_predicted_compute_us=2019`
(1244 + 775). O caminho "nó sem score" é coberto por teste unitário: não há ambiente para forçar
score zero no worker.

Critério de saída: qualquer requisição de teste possui trace completo e métricas suficientes para explicar sua latência. — [x] as métricas de latência existem (`metrics.md`: latência vs compute por cluster, com percentis e sinais de nó) e o trace por requisição existe (`traces.md`, H3), que é a resposta a "por que essa requisição demorou?"; e o trace carrega o previsto ao lado do real (`predictions.md`, H5), que é a resposta a "quanto do que demorou o modelo já sabia" — agora também no roll-up do cluster (`total_predicted_compute_us`).

## 10. Fase I — State Manager

### I1 State directory

IDs, ownership, replicas. — [x] (`persistence.md`): `AI_CLUSTER_STATE` nomeia o arquivo de
snapshot; a fila e seus outputs sobrevivem a um restart, e agora também o **dono** de cada
unidade em voo. As unidades despachadas vão em `WorkSnapshot.dispatched` e voltam ao reload
com o mesmo nó, então o resultado que aquele worker já estava calculando é aceito em vez de
descartado; quem não volta é reoferecido quando o grace passa (`WorkStore::unknown_owners`).
Cada gravação escreve dois arquivos, `path` e `path.replica`, e um primário ilegível ou de
zero bytes leva à cópia. Essa cópia é **local**, no mesmo volume: as duas morrem juntas quando
o volume morre. A cópia que sobrevive a isso fica num nó (`AI_CLUSTER_STATE_REPLICA`), entregue
na batida e oferecida de volta no registro — ver a fatia I4. O diretório de referências lógicas
(`state://…`) não existe: não tem consumidor enquanto não houver estado por chave. Evidência:
`bash code/tests/phase-i-state.sh` e `bash code/tests/phase-i4-replica.sh`.

### I2 HOT/WARM/COLD

RAM/VRAM, SSD/NVMe, persistent storage. — [x] (`storage-tiers.md`): cada nó classifica suas
classes de armazenamento em bloco lendo `/sys/block` — NVMe/SSD viram WARM, HDD vira COLD —
e reporta em `CapabilityReport.storage_classes`; HOT é a RAM, já em `memory_bytes`. A coluna
`ARMAZ` em `ai-cluster nodes` mostra o resultado e `-` é "não medido". Não há medida de
latência nem de capacidade, então NVMe e SATA SSD ficam juntos em WARM. Evidência:
`bash code/tests/phase-i-storage.sh`.

### I3 Checkpoint

Incremental. — [x] (`persistence.md`): o checkpoint deixou de carregar toda matriz já
concluída. A poda roda dentro de `save_locked`, o único ponto por onde toda mutação passa, e
segura só o resultado que uma unidade não terminada ainda pode ler — pelos dois caminhos de
leitura, `ready` e `take_locked`. Medido em 8 unidades de 256³ sem dependências: o snapshot
final caiu de 4.194.410 bytes (8 matrizes de saída, uma por unidade) para 2 bytes. O custo
passou a ser proporcional ao trabalho que falta, não ao trabalho já feito. Ainda não é um
journal de deltas — o conjunto vivo continua sendo reescrito inteiro, e duas vezes por causa
da cópia. Evidência: `bash code/tests/phase-i-checkpoint.sh`.

### I4 Recovery

Node loss, replica promotion, recomputation.

- [x] recuperação de nó perdido e de interrupção abrupta **com unidade em voo** (`recovery.md`):
  dois relógios — grace de heartbeat para o nó que ficou mudo com o controller de pé, e grace
  desde o start para o dono que não voltou depois do restart — e os dois desembocam em
  `requeue_for_node`. A posse restaurada do disco é o que permite esperar o grace antes de
  reoferecer, em vez de a unidade rodar duas vezes enquanto o dono ainda pode estar voltando.
  Evidência: `bash code/tests/phase-i4-recovery.sh` (o restart traz `1 em voo`; passado o
  grace, `node-old` é dado como ausente e `node-new` conclui a unidade recuperada).
- [x] promoção de réplica (`recovery.md`, "A cópia num nó"): o controller entrega o snapshot
  numa batida quando a época dele avançou além da que o nó reporta, o nó guarda em
  `AI_CLUSTER_STATE_REPLICA` e oferece de volta no (re-)registro. Um controller que perdeu os
  dois arquivos locais adota a cópia mais nova e a grava de volta. Evidência:
  `bash code/tests/phase-i4-replica.sh` (o volume do controller morre e a fila volta da cópia
  de um nó; o roteiro falha se algum arquivo local tiver sido carregado).
- [ ] o que ainda não é replicado: estado por chave — KV cache, execution state, model state.
  Não existe no cluster, então não há o que promover além da fila e do que ela lê. É o degrau
  seguinte.

Critério de saída: execução distribuída recupera-se de falha planejada e de interrupção abrupta em um ambiente controlado. — [x] falha planejada de worker (`phase-2-failure.sh`, controller vivo), interrupção abrupta do controller com unidade em voo (`phase-i4-recovery.sh`) e perda dos dois arquivos de estado com recuperação pela cópia de um nó (`phase-i4-replica.sh`).

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

Throughput/latency. — [~] throughput de escrita (`storage-tiers.md`); latência não.

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
