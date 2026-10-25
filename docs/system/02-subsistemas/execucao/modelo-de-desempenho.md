---
title: "Modelo de desempenho"
aliases: ["Performance Model", "docs/21", "21 — Performance Model"]
tags: [sistema/subsistema/execucao, subsistema/modelo-de-desempenho]
type: sistema/subsistema
status: proposto
implementacao: parcial
fases: ["[[fase-02-capability-e-saude]]", "[[fase-05-observabilidade]]"]
---

# Modelo de desempenho

Voltar para [[execucao]].

## Objetivo

Prever custo e capacidade, e comparar a previsão com o resultado observado. O modelo alimenta o custo do [[scheduler]] e o plano do [[execution-planner]].

## Entradas

- benchmark de hardware ([[capability-engine]]);
- métricas históricas de carga real ([[observabilidade]]);
- carga atual do nó;
- topologia de rede;
- desempenho de armazenamento ([[storage-intelligence]]);
- modelo e variante;
- precisão;
- tamanho de contexto.

## Saídas

- previsão de latência;
- previsão de vazão;
- custo por nó;
- capacidade do cluster;
- confiança da previsão;
- variância esperada.

## Laço de aprendizado

```text
benchmark
 -> carga real
 -> observar
 -> comparar previsto e realizado
 -> atualizar o modelo
 -> melhorar o próximo plano
```

## Capacidade do cluster

Manter separadamente:

- capacidade de latência;
- capacidade de vazão;
- capacidade de memória;
- capacidade de armazenamento;
- capacidade de rede.

Não tratar a soma de RAM e CPU como se fosse um único dispositivo físico.

> [!info] O que já existe
> - **Benchmark por nó**: a capability reporta `compute_score` (melhor execução), `compute_median_score`, `compute_deviation`, `network_score` e `network_achieved_score`. Ver [[f02-benchmark-de-capability]] e [[f02-estabilidade-do-benchmark]].
> - **Remedição ociosa** (C3): o worker remede o compute quando está ocioso, no máximo a cada 60 s (`RECOMPUTE_INTERVAL_SECS`), e envia `current_compute_score`. A rede usa `current_network_score` quando existe. Ver [[f02-capability-corrente]].
> - **Previsão de tempo de compute por unidade** (`predicted_compute_us` em `code/crates/controller/src/placement.rs`): `flops / (threads × taxa por thread × GFLOP de referência × razão típico/melhor)` mais um custo fixo de setup de 600 µs (`AI_CLUSTER_UNIT_SETUP_US`). Sem medição, a previsão é zero, que significa "não previsto" e não "instantâneo".
> - **Ranking e previsão usam medidas diferentes**: o ranking usa a linha de base de registro; a previsão usa a medição atual. Assim a previsão acompanha um nó que ficou mais lento depois de entrar.
> - **Registro previsto × real**: cada `CompletedWork` guarda `predicted_compute_us`, e as métricas e o trace expõem os dois valores. Ver [[f05-previsto-vs-real]] e [[f05-traces]].
> - Evidência: `bash code/tests/phase-h-predictions.sh` e `bash code/tests/phase-c-current.sh`.

> [!todo] Não implementado
> - Previsão de transferência e de latência total. Só o tempo de compute é previsto.
> - Capacidade do cluster por dimensão. Não há agregado de latência, vazão, memória, armazenamento ou rede.
> - Confiança e variância como saída. A previsão é um número só.
> - Entradas de modelo, variante, precisão e tamanho de contexto. As unidades não carregam esses campos.
> - Laço de aprendizado que atualiza o modelo a partir do histórico. Hoje a correção é a razão típico/melhor e a remedição ociosa.
> - Carga atual do nó e topologia de rede como entradas da previsão.

> [!warning] Teto do MVP
> A previsão assume que o compute escala linearmente com o número de threads, a mesma premissa do benchmark. Turbo e SMT fazem um nó largo ser um pouco mais lento do que a estimativa linear, então a previsão sai otimista em nós com muitas threads. A pontuação de placement é relativa: só a ordem entre nós é significativa, não o tempo absoluto.
