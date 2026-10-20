---
title: "15 — Edge Optimizer"
type: spec
status: draft
tags: [spec, edge-optimizer]
---

# 15 — Edge Optimizer

## Objetivo

Reduzir trabalho, tokens, contexto, latência e comunicação antes da inferência pesada.

## Pipeline

```text
Request
 -> classify
 -> exact cache
 -> semantic cache
 -> graph retrieval
 -> context selection
 -> dedup
 -> compression
 -> skill/tool resolution
 -> prompt compilation
 -> token budget
 -> model routing
 -> LLM
```

## Caminho rápido

Quando uma requisição for determinística ou cacheável, evitar LLM.

Exemplos:

- cálculo simples;
- consulta de estado do cluster;
- cache hit.

## Retrieval assistido por grafo

Usar entidades e relações para navegar rapidamente para fatos relevantes, incidentes, dependências e contexto.

## Compression

A compressão deve ser orientada pela pergunta, preservando informação relevante e indicando confiança.

## Resolução de Skill / Tool

Resolver fora do contexto principal quais capabilities/tools realmente são necessárias e enviar apenas schemas relevantes.

## Compilador de prompt

Transformar contexto disperso em uma representação mínima adequada à tarefa.

## Token Budgeter

Estimar input, output, maximum, contexto disponível e objetivo de qualidade.

## Decisão de otimização

Cada otimização possui custo próprio. O sistema deve aplicá-la somente quando o saving esperado superar esse custo.

## Métricas

- original tokens;
- optimized tokens;
- tokens avoided;
- reduction ratio;
- LLM calls avoided;
- compute avoided;
- cache hit rate;
- graph hit rate.
