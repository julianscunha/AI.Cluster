---
title: "Observabilidade e telemetria"
aliases: ["docs/17", "17 — Observability / Telemetry Engine", "17 — Motor de observabilidade / telemetria"]
tags: [sistema/subsistema/operacao]
type: sistema/subsistema
status: decidido
implementacao: parcial
fases: ["[[fase-05-observabilidade]]"]
---

# Observabilidade e telemetria

O cluster precisa responder três perguntas em qualquer momento: como está cada nó e o cluster
inteiro, o que mudou, e o que aconteceu com uma requisição específica. A telemetria é organizada
em três tipos, cada um com um papel diferente, e um quarto elemento junta previsão e realidade
para o modelo de custo poder ser calibrado.

## Três tipos de telemetria

| Tipo | Responde | Forma |
|---|---|---|
| Métricas | Como está agora, em valores contínuos | Totais, médias, percentis e sinais por nó. |
| Eventos | O que mudou | Linha do tempo de transições de ciclo de vida. |
| Traces | Por onde passou uma requisição | Unidades de uma requisição, com nó e tempos. |

Previsto vs real acompanha as três: mostra a estimativa e a medida lado a lado, para que o erro do
modelo seja visto e não só o ranking do placement.

## Métricas de nó

Alvo: CPU (uso, frequência, carga, throttling), RAM (uso, swap, banda), GPU e NPU (uso, memória,
temperatura), armazenamento (latência, IOPS, throughput, saúde, desgaste), rede (banda, latência,
perda, erros).

## Métricas de inferência

Alvo: tokens de entrada, de saída e em cache, tokens evitados, TTFT, ITL, throughput de prompt e de
geração, tamanho de contexto e uso do cache KV. Dependem do motor de inferência e do Request Manager
([[inference-engine]], [[request-manager]]).

## Eventos

Alvo: mudanças de estado do cluster, como entrada e saída de nó, degradação, perda de nó, alerta de
SSD e início de rebalanceamento.

## Previsto vs real

Estimativas e realidade lado a lado, por unidade, por requisição e no cluster. Serve para calibrar o
modelo de desempenho ([[modelo-de-desempenho]]).

## Amostragem adaptativa

Normal: frequência baixa. Nó degradado ou anômalo: frequência alta por um período, depois volta ao
normal.

## Explicabilidade

Diagnósticos indicam causa provável, impacto e ação recomendada. O que não tem causa conhecida diz
isso, em vez de chutar.

## Formato de saída

- Métricas: `chave=valor` com nomes iguais aos campos do protocolo, para script e scraper.
- Scraper: formato texto do Prometheus, com prefixo `ai_cluster_` e rótulo `node=`.
- Eventos e traces: tabelas legíveis pela CLI ([[cli-tui]]).

> [!info] O que já existe
> - Métricas de cluster (contagens, totais, médias, percentis p50/p95 de compute e latência) e sinais
>   por nó (CPU, temperatura, swap, IOPS, score de rede nominal): [[f05-metricas]]. O exportador em
>   `GET /metrics` na porta 9101 (`code/crates/controller/src/exporter.rs`) exige o token de operador.
> - Eventos de ciclo de vida, em anel de 256 em memória: [[f05-eventos]]. Os nomes reais são
>   `node_registered`, `node_approved`, `node_revoked`, `node_left`, `node_offline`, `work_enqueued`,
>   `graph_enqueued`, `work_completed`, `node_reserved`, `reservation_released` e `shard_channel_opened`
>   (`code/crates/controller/src/service.rs`, `main.rs`).
> - Traces por requisição, com unidades, estado, nó e tempos: [[f05-traces]].
> - Painel de uma tela, lendo as mesmas chamadas da CLI: [[f05-dashboard]].
> - Previsto vs real por unidade e no cluster: [[f05-previsto-vs-real]].
> - Sinais de nó vindos de `/proc` e `/sys` do contêiner, então CPU e disco são do host inteiro.

> [!todo] Pendente:
> - Métricas de nó: frequência, carga, throttling, GPU/NPU, latência e saúde de armazenamento,
>   banda de memória, latência e perda de rede. Hoje `storage_health`, `memory_bandwidth_score` e
>   `network_latency_ms` não são preenchidos.
> - Métricas de inferência: nenhuma, pois o motor de inferência ainda não expõe contagens de token nem
>   tempos de geração.
> - Eventos `NODE_DEGRADED`, `SSD_WARNING` e `REBALANCE_STARTED`: não são emitidos.
> - Amostragem adaptativa: a frequência é fixa.
> - Explicabilidade: não há diagnóstico com causa e ação.
> - Histórico: métricas e eventos são instantâneos ou anel em memória; nada é persistido.
> - Ator nos eventos administrativos, ver [[seguranca]].

> [!warning] Teto do que existe: o anel de eventos guarda 256 entradas e se perde no restart. Os
> totais de compute e latência cobrem só as unidades que o controller ainda retém; quando uma sai do
> anel de conclusões, o número cai. Um contador monotônico exigiria série temporal, que não existe.
