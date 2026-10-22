---
title: "Métricas — roll-up do cluster e sinais de nó"
type: implementation
status: implemented
tags: [implementation, metrics, observability]
---

# Métricas — roll-up do cluster

O controller já sabia tudo o que estas métricas mostram: contagem de nós no registry, fila
e conclusões no `WorkStore`. Faltava uma leitura só que juntasse isso. Antes, responder
"como está o cluster?" exigia cruzar `ai-cluster nodes` com `ai-cluster events` na cabeça de
quem lê. Isto é o H1 da fase de Observability, e é o que fecha a pendência de "métricas
agregadas" do M0.

## O que mudou

- `MetricsResponse` e `GetMetrics` no proto: uma chamada devolve o roll-up, agora com os
  **percentis** p50/p95 de compute e de latência ao lado das médias.
- `MetricsResponse.total_predicted_compute_us`: a previsão do modelo de custo somada sobre as
  unidades concluídas, ao lado do compute real. É o "previsto vs real" no nível do cluster
  (`predictions.md`, H5) — o trace dá o mesmo número por requisição.
- `HealthReport` ganhou `swap_utilization` e `disk_io_ops`, e o worker passou a preencher
  `cpu_utilization` e `temperature_c`, que existiam no wire e ficavam sempre em zero.
- `Registry::counts`: total de nós e quantos estão disponíveis, **num só lock**, para os dois
  números não virem de estados diferentes do mundo.
- `metrics::summarize` (`code/crates/controller/src/metrics.rs`): a aritmética, pura, fora dos
  locks — recebe contagens e os registros concluídos e devolve a mensagem. Testável sem
  subir registry nem work store.
- `exporter::render` e `exporter::serve` (`code/crates/controller/src/exporter.rs`): o mesmo
  roll-up, mais os sinais de cada nó, em formato texto do Prometheus num `GET /metrics`. Os
  sinais de compute agora incluem a distribuição do benchmark —
  `ai_cluster_node_compute_median_score` e `ai_cluster_node_compute_deviation`
  (`benchmark-stability.md`), ao lado do melhor caso em `ai_cluster_node_compute_score`.
- CLI: `ai-cluster metrics` (com percentis) e `ai-cluster nodes` (com as colunas de nó).

## Como ler

```text
nodes_total=1 nodes_available=1
units_completed=2 units_pending=0 units_blocked=0 units_failed=0
total_compute_time_us=1217 avg_compute_time_us=608 total_predicted_compute_us=36
total_latency_us=3000 avg_latency_us=1500
p50_compute_time_us=538 p95_compute_time_us=679
p50_latency_us=1000 p95_latency_us=2000
```

A saída é `chave=valor` com chaves ASCII iguais aos nomes de campo do proto, não uma tabela
de colunas: `metrics` é o comando que um script ou um scraper lê, e parsing posicional quebra
sempre que um rótulo muda.

O que explica a latência é o **contraste** entre as duas linhas de tempo. No exemplo, as duas
unidades levaram 3.000us de ponta a ponta, mas só 1.217us de compute: os ~1.800us restantes
são transferência, fila e espera de poll. É exatamente a pergunta "por que esta requisição
demorou" que o critério de saída da fase H pede.

- `units_pending` é fila **mais** o que está em voo; `units_blocked` é o subconjunto que
  espera dependência do grafo. `pending` inclui `blocked`.
- `units_failed` são as que nunca vão completar: orçamento de retry esgotado.
- `nodes_available` é o conjunto que o placement enxerga — registrado e não drenando,
  offline ou em quarentena.
- `total_predicted_compute_us` é o que o modelo de custo esperava somar nessas unidades; a
  distância para `total_compute_time_us` é o erro do modelo no cluster inteiro
  (`predictions.md`, H5). Zero quando nenhuma unidade concluída teve previsão — "não previsto",
  não "instantâneo".

## Percentis

Média e percentil respondem perguntas diferentes. A média diz o custo típico; o p95 diz o que
a cauda sente — e a cauda é o que um cliente que espera por **uma** requisição experimenta.
No exemplo, a média de latência é 1.500us e o p95 é 2.000us: metade das unidades terminou em
até 1.000us, e uma em cada vinte passou de 2.000us.

O cálculo é **nearest-rank**: ordena os tempos e pega o menor valor cuja posição cobre
`ceil(p * n)`. A resposta é sempre um tempo que aconteceu de verdade, nunca uma interpolação
entre dois. Com uma unidade só, os quatro percentis são o mesmo número — e isso é a resposta
certa, não um defeito.

## Sinais de nó

```text
NÓ        ESTADO   ARQ      THREADS  RAM_GB  COMPUTE  SAÚDE  CPU_%  TEMP_C  SWAP_%
node-m    READY    x86_64         4    3.77     1.18   0.73   7.51    0.00    1.14
```

O worker mede no próprio `/proc` a cada heartbeat:

- **CPU (`/proc/stat`)**: diferença entre duas leituras, com iowait contado como ocioso — um
  core bloqueado em disco não está fazendo trabalho de CPU.
- **Swap (`/proc/meminfo`)**: `1 - SwapFree/SwapTotal`.
- **Temperatura (`/sys/class/thermal/thermal_zoneN/temp`)**: milésimos de grau divididos por
  mil.
- **IOPS (`/proc/diskstats`)**: leituras e escritas concluídas por segundo, também por
  diferença entre leituras.

**Zero significa "não medido", não "zero".** A primeira leitura não tem com o que comparar e
reporta CPU e IOPS em zero; um contêiner sem `/sys/class/thermal` reporta temperatura zero.
É a mesma convenção de um `compute_score` ou `health_score` não reportado.

`health_score` continua refletindo **só pressão de memória**. Os sinais de CPU, térmico e swap
são reportados crus; dobrá-los dentro do score é decisão do motor de saúde (`docs/06`), não de
uma fatia de métricas — e mexer nisso mudaria o placement, que usa o score como penalidade.

## Exportador

```text
GET http://<controller>:9101/metrics     # AI_CLUSTER_METRICS_ADDR, padrão 0.0.0.0:9101
```

Os nomes são prefixados `ai_cluster_` e no resto iguais aos campos do proto, então um número
lido no `/metrics` e o mesmo número lido em `ai-cluster metrics` se reconhecem. Cada nó vira
uma série com rótulo `node=`. O `HELP`/`TYPE` sai uma vez por métrica, não por amostra: um
parser Prometheus estrito recusa uma métrica com o tipo declarado duas vezes.

Falhar ao subir o exportador é reportado e **não** é fatal — perder o exportador não pode
derrubar o control plane com ele.

## Por que não é uma série temporal

Cada leitura é um instantâneo do estado atual, não um acumulado histórico. Duas leituras
com um segundo de diferença podem divergir, e os totais de compute/latência são só das
unidades ainda **retidas** pelo controller — quando uma unidade sai do ring de conclusões, o
número cai. Um contador monotônico exigiria guardar histórico, que é outra decisão (e outra
fatia). Aqui, "total" significa "total do que o controller ainda lembra".

## Confiança dos números

`total_compute_time_us` vem do worker (tempo de compute auto-reportado); `total_latency_us`
vem do relógio do controller (despacho → resultado). São domínios diferentes: numa unidade
que termina dentro de um milissegundo, o auto-reporte do worker pode **exceder** a parede
observada pelo controller. Em produção a unidade leva milissegundos e o compute é uma fração
da latência, então a diferença é overhead de verdade. A média é divisão inteira (trunca para
baixo) e é 0 quando nada concluiu — um cluster ocioso é um estado válido, não um erro.

Os sinais de nó vêm do `/proc` **do contêiner**, que num contêiner comum é o `/proc` do host.
CPU e disco, portanto, são do host inteiro, não do que o worker conseguiu usar — para "como
está o nó" é o sinal certo, mas não é um medidor de consumo do processo.

## Evidência

`bash code/tests/phase-h-metrics.sh`.

```text
=== A) cluster ocioso ===
nodes_total=1 nodes_available=1
units_completed=0 ... p50_latency_us=0 p95_latency_us=0
OK: 1 nó, nada concluído, média em 0 (sem divisão por zero)
=== B) duas unidades executadas ===
p50_compute_time_us=538 p95_compute_time_us=679
p50_latency_us=1000 p95_latency_us=2000
OK: as duas unidades aparecem como concluídas e o compute foi somado
=== C) percentis ===
OK: p50_latency_us=1000 <= p95_latency_us=2000
=== D) métricas de nó ===
node-m  READY  x86_64  4  3.77  1.18  0.73  7.51  0.00  1.14
OK: a tabela de nós traz as colunas de métricas de nó
=== E) exportador Prometheus ===
ai_cluster_nodes_total 1
OK: ai_cluster_nodes_total presente
OK: ai_cluster_node_cpu_utilization presente
```

Coberto por `sums_and_averages_over_completed_units`,
`sums_the_prediction_beside_the_measurement`,
`an_empty_cluster_reports_zero_instead_of_dividing_by_zero`,
`averages_round_down_rather_than_up`, `a_nonsensical_timing_saturates_instead_of_wrapping`,
`percentiles_use_the_nearest_rank` e `summarize_reports_percentiles_beside_the_average` em
`metrics.rs`; `renders_the_cluster_roll_up`,
`renders_one_sample_per_node_with_the_node_label` e
`a_node_without_identity_does_not_break_the_sample` em `exporter.rs`; `parses_meminfo_values`
e os testes de `/proc` no worker; e `metrics_roll_up_nodes_and_completed_work` no serviço.

## Teto conhecido

**Percentis, não histórico.** Só totais, médias e percentis do instante, sobre o que o
controller ainda retém. Taxa por segundo e tendência precisam de uma série temporal, que não
existe.

**IOPS conta partição duas vezes.** `/proc/diskstats` lista o disco e suas partições, e a soma
inclui as duas — o número é um teto, não um valor exato. Filtrar dispositivos por tipo é o
caminho quando isso importar.

**Rede não é medida.** `network_latency_ms`, `storage_health`, `network_score` e
`memory_bandwidth_score` continuam sem preenchimento: exigem medição ativa, que é uma fatia
própria.

**Métricas de LLM não.** Tokens, TTFT, ITL, KV cache — nada disso existe porque não há motor
de inferência ainda. É a fase J.
