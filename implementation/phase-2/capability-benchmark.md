---
title: "Fase 2 — Benchmark de capability"
type: implementation
status: partial
tags: [implementation, phase-2, capability, benchmark, worker]
---

# Fase 2 — Benchmark de capability

O worker anunciava `cpu_threads` e mais nada: o modelo de custo do placement (G2) tinha um
`compute_score` sempre em 0 e caía na contagem de threads. Esta fatia mede o compute no
próprio hardware do nó e passa a reportá-lo, então o número que o scheduler usa para
escolher o nó vem de uma medição e não de um proxy.

## O que mudou

- `worker::compute_score`: mede o matmul deste nó no **mesmo kernel** que executa as
  unidades (`Matrix::multiply`, o caminho serial de `multiply_with_threads`) e normaliza a
  taxa por core. O valor vai no `compute_score` do `CapabilityReport` registrado.
- `worker::measure_compute_score`: cronometra N multiplicações de uma matriz 256³ e reporta
  a **melhor** das execuções, não a média. Contenção só torna uma execução mais lenta, então
  a mais rápida é a menos perturbada e a mais reproduzível; a média arrastaria a taxa para
  baixo sempre que o host estivesse compartilhado. A primeira execução fria é descartada
  pelo mesmo mecanismo, então não há passe de aquecimento separado.
- `worker::normalize_compute_score`: um core a `REFERENCE_CORE_GFLOPS` pontua 1.0 — o valor
  que o fallback de threads já assumia — então ligar o benchmark não reordena um cluster
  homogêneo. Uma taxa zero ou absurda pontua 0.0, que o modelo lê como "não medido" e
  substitui pelo fallback: um benchmark quebrado não pode parecer um nó rápido.
- CLI: coluna `COMPUTE` na tabela `nodes`, para o perfil medido ser visível ao operador.

## Por que por core

O modelo de custo multiplica o score pelo número de threads reportado
(`compute_rate = threads × compute_score`). Medir multithread contaria os cores duas vezes:
um nó de 8 threads que medisse o próprio paralelismo apareceria como 64 unidades de compute.

## Referência

`REFERENCE_CORE_GFLOPS = 6.0`: um core de ~6 GFLOP/s neste kernel pontua 1.0. É um **botão de
calibração**, não uma verdade física — o que importa é a ordenação entre nós. Num host de
referência mediu-se 6,15 GFLOP/s single-thread; dentro do contêiner, sob carga, a medição
varia. Por isso o melhor-de-N: reduz o arrasto da contenção sem mudar o que o número significa.

`AI_CLUSTER_COMPUTE_SCORE` fixa o score em vez de medir. Serve para um teste de placement não
depender da velocidade do host. Ausente ou não-parseável significa medir.

## Evidência

`bash code/tests/phase-c-benchmark.sh`.

```text
=== A) benchmark medido: o nó reporta o compute que mediu ===
NÓS 1
NÓ                   ESTADO     ARQ        THREADS    RAM_GB COMPUTE    SAÚDE
node-bench           READY      x86_64           4      3.77    1.19     0.78
compute_score medido = 1.19
compute_score relido  = 1.19
OK: o worker reportou um compute medido, não o fallback 0
OK: o perfil é reproduzível entre leituras
=== B) score fixado: AI_CLUSTER_COMPUTE_SCORE=2.0 é o número reportado ===
compute_score fixado = 2.00
OK: o score fixado foi reportado como veio
```

O score 1.19 é por core e cai perto da referência 1.0, como esperado para um core comum. Com
a média em vez do melhor-de-N a mesma medição deu 0.60: o arrasto vinha de disputa de CPU
entre contêineres no mesmo host, não do hardware.

Coberto por `a_reference_speed_core_scores_one`, `a_nonsensical_rate_falls_back_instead_of_scoring_high`
e `the_compute_benchmark_reports_a_positive_rate` no worker.

## Teto conhecido

**Só compute.** Memória, storage, rede e kernels de acelerador não são medidos — a fatia
entrega o score de compute. O modelo de custo já lê `network_score` com fallback 1.0
enquanto não há medição.

**Só um score.** docs/05 pede um perfil (`general_compute`, `matmul_int4`, `memory_bandwidth`,
`thermal_headroom`, ...); o wire carrega um `compute_score` só. O perfil completo é o resto
da fase C2/C3.

**Escala linear com threads.** O modelo assume que N threads dão N× o core; turbo e SMT
fazem uma execução real de 4 threads ficar um pouco abaixo de 4× a de 1 thread. A estimativa
é otimista em nós grandes. Revisitar com uma varredura multithread se o placement pender para eles.

**Confiança não modelada.** docs/05 pede uma medida de confiança derivada do histórico;
não existe — o score é uma leitura instantânea.
