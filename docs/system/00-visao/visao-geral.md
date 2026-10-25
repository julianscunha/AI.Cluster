---
title: "Visão geral"
aliases: ["01 — Overview", "docs/01-overview", "docs/01", "Visão"]
tags: [sistema/visao]
type: sistema/visao
status: decidido
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-01-nucleo-do-cluster]]", "[[fase-02-capability-e-saude]]", "[[fase-03-execucao-distribuida]]", "[[fase-04-escalonamento]]", "[[fase-05-observabilidade]]", "[[fase-06-estado-e-recuperacao]]", "[[fase-07-seguranca-base]]", "[[fase-08-inferencia]]", "[[fase-09-transporte]]", "[[fase-10-inteligencia-de-storage]]", "[[fase-11-request-manager]]", "[[fase-12-model-runtime]]", "[[fase-13-graph-engine]]", "[[fase-14-edge-optimizer]]", "[[fase-15-ai-cluster-os]]", "[[fase-16-aceleradores]]", "[[fase-17-runtime-adaptativo]]", "[[fase-18-worker-movel]]", "[[fase-19-hardening-de-producao]]"]
---

# Visão geral

Voltar para [[visao]]. Princípios em [[principios]]; requisitos em [[requisitos]]; arquitetura em [[arquitetura]].

## Propósito

Definir o escopo do AI Cluster como um runtime distribuído e adaptativo para IA: uma plataforma Linux-first que transforma hardware heterogêneo reaproveitado em um pool distribuído de computação para IA.

## Ideia central

Uma coleção heterogênea de equipamentos Linux deve se comportar como um pool lógico de recursos, capaz de executar uma única requisição distribuindo um modelo e suas operações entre diferentes dispositivos.

## O que o sistema deve fazer

- Executar uma única inferência distribuindo um modelo robusto entre múltiplos dispositivos.
- Aceitar hardware heterogêneo: x86_64, ARM64, CPU, GPU, NPU, PCs, servidores, notebooks, SBCs e dispositivos móveis compatíveis.
- Provisionar nós automaticamente e convertê-los para um AI Cluster OS próprio.
- Suportar hot-plug, drain, falha e replanejamento sem derrubar o cluster.
- Medir capacidade real, saúde, temperatura, armazenamento, rede e desempenho.
- Otimizar tokens e computação na borda antes de acionar modelos grandes.
- Usar grafos para topologia, conhecimento, dependências, execução e observabilidade.
- Tratar comunicação, memória e armazenamento como recursos de primeira classe.
- Fornecer TUI/CLI operacional em shell.
- Expor capacidade de inferência somente por uma Inference API.
- Manter Application Plane e Compute Plane desacoplados.
- Aprender com desempenho observado e recalibrar previsões do cluster.

## Cargas de trabalho primárias

- LLM inference;
- embeddings;
- reranking;
- context processing;
- multimodal preprocessing;
- compression;
- deterministic compute;
- custom AI graphs.

## Propriedades operacionais

- onboarding com pouco ou nenhum toque manual (zero/low-touch);
- reimaging;
- hot-plug;
- scheduling consciente de saúde e de desempenho;
- retrieval assistido por grafo;
- otimização adaptativa de tokens;
- TUI detalhada.

## Fora do escopo da v0.1

- suporte a hardware móvel arbitrário;
- suporte universal a NPU;
- migração transparente de memória de processo arbitrário;
- garantia de escala linear;
- substituir kernels de LLM maduros por um compilador de kernel novo no primeiro dia.

> [!info] O que já existe
> - Cluster CPU com controller, workers e CLI: entrada e saída de nós ([[f01-entrada-e-saida-de-nos]]), capability e saúde ([[f02-capability-corrente]], [[f02-modelo-de-saude]]), DAG com retry e recuperação ([[f03-tolerancia-a-falhas]]) e placement por custo ([[f04-placement]]).
> - Observabilidade de eventos, métricas, traces e dashboard de CLI ([[f05-eventos]], [[f05-metricas]], [[f05-traces]], [[f05-dashboard]]).
> - Gateway HTTP com chave de API, `/v1/models`, `/v1/embeddings` e `/v1/chat/completions` com streaming SSE, sobre modelo sintético e sobre GPT-2 small e SmolLM2-135M em CPU ([[f08-gateway-e-api]], [[f08-engine-local]]).
> - Projeções de matmul em workers por canal residente de fatias ([[f08-canal-residente-de-fatias]]).

> [!todo] O que falta
> - Request Manager: admissão, filas, quotas e orçamento ([[request-manager]], [[fase-11-request-manager]]).
> - Model Runtime distribuído: registry, variantes, shards, pipeline/tensor parallelism ([[fase-12-model-runtime]]).
> - Graph engine e Edge optimizer ([[graph-engine]], [[edge-optimizer]]).
> - AI Cluster OS e provisionamento por reimage ([[ai-cluster-os]], [[provisionamento]]).
> - Aceleradores, worker móvel e hardening de produção ([[fase-16-aceleradores]], [[fase-18-worker-movel]], [[fase-19-hardening-de-producao]]).
