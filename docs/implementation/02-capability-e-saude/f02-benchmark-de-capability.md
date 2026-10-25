---
title: "F02 — Benchmark de capability"
aliases: ["phase-2/capability-benchmark", "C2", "Benchmark de compute"]
tags: [implementacao/fase/02, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-02-capability-e-saude]]"
codigos-antigos: ["C2", "phase-2/capability-benchmark"]
sistema: ["[[capability-engine]]"]
evidencia: ["code/tests/phase-c-benchmark.sh"]
---
# F02 — Benchmark de capability

- [ ] Superado

## Objetivo

O worker anunciava `cpu_threads` e mais nada: o modelo de custo do placement (G2) tinha um
`compute_score` sempre em 0 e caía na contagem de threads. Esta fatia mede o compute no
próprio hardware do nó e passa a reportá-lo, então o número que o scheduler usa para
escolher o nó vem de uma medição e não de um proxy.

## O que foi feito

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

## Banda de memória

`worker::memory_bandwidth_score` copia 32 MiB (além de qualquer cache) e fica com a melhor de 3
corridas, como o compute. Normaliza sobre `REFERENCE_MEMORY_MBPS` = 10 GB/s (1.0 ≈ um core DDR4
comum) e vai em `memory_bandwidth_score`; a CLI mostra `mem_bw=` no detalhe do nó. Zero é "não
medido". Teste unitário `memory_probe_reports_a_positive_rate`; sem E2E e sem consumidor no
placement ainda.

> [!todo] Limite do atalho
> Single-thread: subestima nós multicanal. Medir com várias threads e sob carga concorrente quando
> o placement precisar da banda agregada de uma unidade limitada por memória.

## Limites

**Compute e banda de memória.** Storage e rede têm sonda própria; kernels de acelerador não são medidos. O modelo de custo já lê `network_score` com fallback 1.0
enquanto não há medição.

**Só um score.** [[capability-engine]] pede um perfil (`general_compute`, `matmul_int4`, `memory_bandwidth`,
`thermal_headroom`, ...); o wire carrega um `compute_score` só. O perfil completo é o resto
da fase C2/C3 ([[f02-capability-corrente]], [[f02-descoberta-de-rede]]).

**Escala linear com threads.** O modelo assume que N threads dão N× o core; turbo e SMT
fazem uma execução real de 4 threads ficar um pouco abaixo de 4× a de 1 thread. A estimativa
é otimista em nós grandes. Revisitar com uma varredura multithread se o placement pender para eles.

**Confiança não modelada no baseline.** O score do registro é uma leitura instantânea e viaja
sem medida de confiança própria: não dá para dizer se o baseline do nó A foi mais limpo que o
do nó B. A confiança existe para a remedida ([[f02-capability-corrente]], C3), não para o baseline.

## Próximos passos

- Medir memória sob carga e storage na mesma forma do compute (ver [[fase-02-capability-e-saude]]).
- Perfil multidimensional em vez de um `compute_score` só, quando o placement precisar distinguir cargas.
- Varredura multithread se o placement pender para nós grandes.
