---
title: "F05 — Observabilidade"
aliases: ["Fase H — Observability", "Observabilidade (Fase H do EP)"]
tags: [implementacao/fase/05, implementacao/estado/implementado]
type: implementacao/fase
status: implementado
codigos-antigos: ["H1", "H2", "H3", "H4", "H5"]
sistema: ["[[observabilidade]]", "[[cli-tui]]", "[[modelo-de-desempenho]]"]
---

# F05 — Observabilidade

## Objetivo

Responder, para qualquer requisição de teste, por que ela demorou: eventos de ciclo de vida, métricas agregadas, traces por requisição, painel de uma tela e o erro entre o previsto pelo modelo de custo e o medido.

## Depende de

- [[fase-03-execucao-distribuida]] (unidades e grafo que geram os dados)
- [[fase-04-escalonamento]] (decisões e previsão de custo que são expostas)

## Entrega

| Item | Status | Onde |
|---|---|---|
| Eventos de ciclo de vida (`ai-cluster events`) | [x] | [[f05-eventos]] |
| Métricas do cluster, percentis e sinais de nó (`metrics`, `nodes`, `/metrics`) | [x] | [[f05-metricas]] |
| Traces por requisição (`request trace`) | [x] | [[f05-traces]] |
| Painel de uma tela (`dashboard`) | [x] | [[f05-dashboard]] |
| Previsto vs real (erro do modelo de custo) | [x] | [[f05-previsto-vs-real]] |
| Métricas de inferência: TTFT, ITL, tokens/s, KV cache (`GET /metrics` no gateway) | [x] | [[f05-metricas-de-inferencia]] |

## Checklist

Fonte: EP §9 (H1–H5), roadmap seção Observabilidade e EP §8/§9 "critério de saída".

- [x] H1 Metrics: nós totais e disponíveis, fila, tempos de compute e latência com médias e percentis p50/p95, sinais de nó (CPU, swap, temperatura, IOPS) e exportador Prometheus. Rede nominal; storage score não. Métricas de LLM no gateway ([[f05-metricas-de-inferencia]]).
- [x] H2 Events: lifecycle e incidentes, ring buffer de 256 eventos.
- [x] H3 Traces: request → Work Unit → transferência → compute, com `request_id` ligando as unidades. Persistir a correlação além de restart não.
- [x] H4 TUI / dashboard: painel em shell sobre as chamadas existentes. TTFT, ITL, tokens/s e KV cache existem no `/metrics` do gateway, mas o painel ainda não os lê.
- [x] H5 Previsto vs real: o modelo de custo prevê tempo; trace e roll-up mostram a previsão ao lado da medição.
- [x] Critério de saída: toda requisição de teste tem trace completo e métricas suficientes para explicar sua latência.

## Gate

Gate padrão (EP §19): código, benchmark, telemetria, teste de falha, doc e ADR.

- [x] Código: `code/crates/controller/src/{events,metrics,exporter}.rs`, CLI `ai-cluster`.
- [x] Benchmark: `phase-h-predictions.sh` fixa a aritmética do modelo; `phase-h-traces.sh` mostra o erro em requisição real.
- [x] Telemetria: esta fase é a telemetria.
- [x] Teste de falha: `code/tests/phase-2-failure.sh` mata um worker em voo e agora verifica o evento `node_offline` em `events`.
- [x] Doc: os cinco slices desta pasta.
- [x] ADR: [[adr-012-metricas-de-inferencia-no-gateway]].

> [!warning] Tetos transversais
> Nada de série temporal: métricas são instantâneos sobre o que o controller ainda retém. As métricas de inferência vivem no gateway, sem histórico além das últimas 1024 amostras. Rede e storage score são nominais ou ausentes.

## Slices

- [[f05-eventos]]: ring buffer de 256 eventos, lifecycle registrado pelo serviço e pelo sweep de offline.
- [[f05-metricas]]: roll-up do cluster, percentis nearest-rank, sinais de nó do `/proc` e exportador Prometheus.
- [[f05-traces]]: `request_id` no work unit, `GetTrace` e `ai-cluster request trace`.
- [[f05-dashboard]]: painel `ai-cluster dashboard [--watch N]` sobre `ListNodes`, `GetMetrics` e `ListReservations`.
- [[f05-metricas-de-inferencia]]: `Sampler` por geração, p50/p95 de TTFT e ITL, tokens/s e KV cache no `GET /metrics` do gateway.
- [[f05-previsto-vs-real]]: previsão de tempo por unidade, correção por taxa típica e setup, erro percentual por requisição e no cluster.

## Códigos antigos

- H1 (Metrics), H2 (Events), H3 (Traces), H4 (TUI), H5 (Previsto vs real), do EP §9
- roadmap, seção Observabilidade (mesmos itens)

## Specs do sistema

- [[observabilidade]]
- [[cli-tui]]
- [[modelo-de-desempenho]]
