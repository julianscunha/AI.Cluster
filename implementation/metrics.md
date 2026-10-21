---
title: "Métricas — roll-up do cluster"
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

- `MetricsResponse` e `GetMetrics` no proto: uma chamada devolve o roll-up.
- `Registry::counts`: total de nós e quantos estão disponíveis, **num só lock**, para os dois
  números não virem de estados diferentes do mundo.
- `metrics::summarize` (`code/crates/controller/src/metrics.rs`): a aritmética, pura, fora dos
  locks — recebe contagens e os registros concluídos e devolve a mensagem. Testável sem
  subir registry nem work store.
- CLI: `ai-cluster metrics`.

## Como ler

```text
nodes_total=1 nodes_available=1
units_completed=2 units_pending=0 units_blocked=0 units_failed=0
total_compute_time_us=1159 avg_compute_time_us=579
total_latency_us=5000 avg_latency_us=2500
```

A saída é `chave=valor` com chaves ASCII iguais aos nomes de campo do proto, não uma tabela
de colunas: `metrics` é o comando que um script ou um scraper lê, e parsing posicional quebra
sempre que um rótulo muda.

O que explica a latência é o **contraste** entre as duas últimas linhas. No exemplo, as duas
unidades levaram 5.000us de ponta a ponta, mas só 1.159us de compute: os ~3.800us restantes
são transferência, fila e espera de poll. É exatamente a pergunta "por que esta requisição
demorou" que o critério de saída da fase H pede.

- `units_pending` é fila **mais** o que está em voo; `units_blocked` é o subconjunto que
  espera dependência do grafo. `pending` inclui `blocked`.
- `units_failed` são as que nunca vão completar: orçamento de retry esgotado.
- `nodes_available` é o conjunto que o placement enxerga — registrado e não drenando,
  offline ou em quarentena.

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

## Evidência

`bash code/tests/phase-h-metrics.sh`.

```text
=== A) cluster ocioso ===
nodes_total=1 nodes_available=1
units_completed=0 units_pending=0 units_blocked=0 units_failed=0
total_compute_time_us=0 avg_compute_time_us=0
total_latency_us=0 avg_latency_us=0
OK: 1 nó, nada concluído, média em 0 (sem divisão por zero)
=== B) duas unidades executadas ===
nodes_total=1 nodes_available=1
units_completed=2 units_pending=0 units_blocked=0 units_failed=0
total_compute_time_us=1159 avg_compute_time_us=579
total_latency_us=5000 avg_latency_us=2500
OK: as duas unidades aparecem como concluídas e o compute foi somado
```

Coberto por `sums_and_averages_over_completed_units`,
`an_empty_cluster_reports_zero_instead_of_dividing_by_zero`,
`averages_round_down_rather_than_up`, `a_nonsensical_timing_saturates_instead_of_wrapping`
em `metrics.rs`, e `metrics_roll_up_nodes_and_completed_work` no serviço.

## Teto conhecido

**Só o que o controller já tem.** São contagens e tempos do registry e do work store. As
métricas de nó do `docs/17` — utilização de CPU, temperatura, swap, IOPS, banda — dependem do
worker passar a medi-las e reportá-las; hoje o worker reporta `HealthReport` mas a maioria
dos campos fica no default. Esta fatia não inventa esses sinais.

**Sem histórico, sem percentis.** Só totais e médias do instante. p50/p95, taxa por segundo e
tendência precisam de uma série temporal, que não existe.

**Sem exportador.** O número sai por gRPC e pela CLI. Um endpoint Prometheus/statsd é uma
fatia separada, e é onde `chave=valor` já ajuda.

**Métricas de LLM não.** Tokens, TTFT, ITL, KV cache — nada disso existe porque não há motor
de inferência ainda. É a fase J.
