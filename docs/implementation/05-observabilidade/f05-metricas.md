---
title: "F05 — Métricas: roll-up do cluster e sinais de nó"
aliases: ["metrics", "H1"]
tags: [implementacao/fase/05, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-05-observabilidade]]"
codigos-antigos: ["H1"]
sistema: ["[[observabilidade]]"]
evidencia: ["code/tests/phase-h-metrics.sh"]
---
# F05 — Métricas: roll-up do cluster e sinais de nó

- [ ] Superado

## Objetivo

Uma leitura só que responda "como está o cluster?". O controller já sabia tudo (nós no registry, fila e conclusões no `WorkStore`), mas responder exigia cruzar `ai-cluster nodes` com `ai-cluster events` de cabeça. Fecha a pendência de "métricas agregadas" do M0.

## O que foi feito

- `MetricsResponse` e `GetMetrics` no proto: uma chamada devolve o roll-up, com **percentis** p50/p95 de compute e de latência ao lado das médias.
- `MetricsResponse.total_predicted_compute_us`: previsão do modelo de custo somada sobre as unidades concluídas, ao lado do compute real ([[f05-previsto-vs-real]]).
- `HealthReport` ganhou `swap_utilization` e `disk_io_ops`; o worker passou a preencher `cpu_utilization` e `temperature_c`, que existiam no wire e ficavam sempre em zero.
- `Registry::counts`: total de nós e quantos estão disponíveis, num só lock, para os dois números não virem de estados diferentes.
- `metrics::summarize` (`code/crates/controller/src/metrics.rs`): a aritmética, pura, fora dos locks. Testável sem subir registry nem work store.
- `exporter::render` e `exporter::serve` (`code/crates/controller/src/exporter.rs`): o mesmo roll-up, mais os sinais de cada nó, em texto Prometheus no `GET /metrics`. Sinais de compute incluem a distribuição do benchmark (`ai_cluster_node_compute_median_score`, `ai_cluster_node_compute_deviation`, ver [[f02-estabilidade-do-benchmark]]) ao lado do melhor caso (`ai_cluster_node_compute_score`).
- CLI: `ai-cluster metrics` (com percentis) e `ai-cluster nodes` (com colunas de nó).

## Como ler

```text
nodes_total=1 nodes_available=1
units_completed=2 units_pending=0 units_blocked=0 units_failed=0
total_compute_time_us=1217 avg_compute_time_us=608 total_predicted_compute_us=36
total_latency_us=3000 avg_latency_us=1500
p50_compute_time_us=538 p95_compute_time_us=679
p50_latency_us=1000 p95_latency_us=2000
```

A saída é `chave=valor` com chaves iguais aos nomes de campo do proto, não tabela de colunas: `metrics` é o comando que um script ou scraper lê, e parsing posicional quebra quando um rótulo muda.

O que explica a latência é o **contraste** entre as duas linhas de tempo. No exemplo, as duas unidades levaram 3.000us de ponta a ponta, mas só 1.217us de compute: os ~1.800us restantes são transferência, fila e espera de poll.

- `units_pending` é fila **mais** o que está em voo; `units_blocked` é o subconjunto que espera dependência. `pending` inclui `blocked`.
- `units_failed` são as que nunca vão completar: orçamento de retry esgotado.
- `nodes_available` é o conjunto que o placement enxerga: registrado e não drenando, offline ou em quarentena.
- `total_predicted_compute_us` é o que o modelo esperava somar; a distância para `total_compute_time_us` é o erro do modelo no cluster. Zero quando nenhuma unidade concluída teve previsão: "não previsto", não "instantâneo".

## Percentis

Média e percentil respondem perguntas diferentes. A média diz o custo típico; o p95 diz o que a cauda sente, e a cauda é o que um cliente que espera por **uma** requisição experimenta. No exemplo, a média de latência é 1.500us e o p95 é 2.000us: metade das unidades terminou em até 1.000us, e uma em cada vinte passou de 2.000us.

O cálculo é **nearest-rank**: ordena os tempos e pega o menor valor cuja posição cobre `ceil(p * n)`. A resposta é sempre um tempo que aconteceu de verdade, nunca interpolação. Com uma unidade só, os quatro percentis são o mesmo número, e isso é a resposta certa.

## Sinais de nó

```text
NÓ        ESTADO   ARQ      THREADS  RAM_GB  COMPUTE  SAÚDE  CPU_%  TEMP_C  SWAP_%
node-m    READY    x86_64         4    3.77     1.18   0.73   7.51    0.00    1.14
```

O worker mede no próprio `/proc` a cada heartbeat:

- **CPU (`/proc/stat`)**: diferença entre duas leituras, com iowait contado como ocioso.
- **Swap (`/proc/meminfo`)**: `1 - SwapFree/SwapTotal`.
- **Temperatura (`/sys/class/thermal/thermal_zoneN/temp`)**: milésimos de grau divididos por mil.
- **IOPS (`/proc/diskstats`)**: leituras e escritas concluídas por segundo, por diferença entre leituras.

**Zero significa "não medido", não "zero".** A primeira leitura reporta CPU e IOPS em zero; um contêiner sem `/sys/class/thermal` reporta temperatura zero. Mesma convenção de `compute_score` ou `health_score` não reportado.

`health_score` continua refletindo **só pressão de memória**. Os sinais de CPU, térmico e swap são reportados crus; dobrá-los no score é decisão do motor de saúde ([[saude-e-confianca]]), e mudaria o placement, que usa o score como penalidade.

## Exportador

```text
GET http://<controller>:9101/metrics     # AI_CLUSTER_METRICS_ADDR, padrão 0.0.0.0:9101
```

Nomes prefixados `ai_cluster_`, e no resto iguais aos campos do proto: um número lido no `/metrics` e o mesmo em `ai-cluster metrics` se reconhecem. Cada nó vira uma série com rótulo `node=`. `HELP`/`TYPE` saem uma vez por métrica, não por amostra, porque um parser Prometheus estrito recusa tipo declarado duas vezes.

Falhar ao subir o exportador é reportado e **não** é fatal: perder o exportador não pode derrubar o control plane.

## Confiança dos números

`total_compute_time_us` vem do worker (tempo de compute auto-reportado); `total_latency_us` vem do relógio do controller (despacho → resultado). São domínios diferentes: numa unidade que termina em menos de um milissegundo, o auto-reporte do worker pode **exceder** a parede observada pelo controller. Em produção a unidade leva milissegundos e o compute é fração da latência. A média é divisão inteira (trunca) e é 0 quando nada concluiu.

Os sinais de nó vêm do `/proc` do contêiner, que num contêiner comum é o do host. CPU e disco são do host inteiro: bom para "como está o nó", não é medidor de consumo do processo.

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

Cobertura: `sums_and_averages_over_completed_units`, `sums_the_prediction_beside_the_measurement`, `an_empty_cluster_reports_zero_instead_of_dividing_by_zero`, `averages_round_down_rather_than_up`, `a_nonsensical_timing_saturates_instead_of_wrapping`, `percentiles_use_the_nearest_rank` e `summarize_reports_percentiles_beside_the_average` em `metrics.rs`; `renders_the_cluster_roll_up`, `renders_one_sample_per_node_with_the_node_label` e `a_node_without_identity_does_not_break_the_sample` em `exporter.rs`; `parses_meminfo_values` e testes de `/proc` no worker; `metrics_roll_up_nodes_and_completed_work` no serviço.

## Limites

> [!warning] Percentis, não histórico
> Só totais, médias e percentis do instante, sobre o que o controller ainda retém. Quando uma unidade sai do ring de conclusões, o número cai. Taxa por segundo e tendência precisam de série temporal, que não existe.

> [!warning] IOPS conta partição duas vezes
> `/proc/diskstats` lista o disco e as partições, e a soma inclui as duas. O número é teto, não valor exato. Filtrar dispositivos por tipo resolve quando importar.

> [!warning] Rede é nominal
> `network_score` é a velocidade negociada do link sobre a referência de 1 GbE ([[f02-descoberta-de-rede]]), um teto; a taxa alcançada sob carga não é. `network_latency_ms`, `storage_health` e `memory_bandwidth_score` continuam sem preenchimento: exigem medição ativa, fatia própria.

> [!todo] Métricas de LLM
> Tokens, TTFT, ITL e KV cache não existem porque não há motor de inferência. Pertencem à [[fase-08-inferencia]].

## Próximos passos

- Medição ativa de rede, latência e banda de memória.
- Séries temporais, se taxa e tendência virarem requisito.
