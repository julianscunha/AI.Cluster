---
title: "IA"
aliases: ["Camada de IA", "Grupo ia", "Inferência"]
tags: [sistema/moc]
type: sistema/moc
status: vivo
implementacao: parcial
---

# IA

Subsistemas do caminho de uma requisição de IA, da aplicação até a execução do modelo. Voltar para [[subsistemas]].

## Caminho da requisição

```text
Aplicação
  -> Inference API          fronteira: autenticação, validação, streaming
  -> Request Manager        admissão, fila, quotas, orçamento
  -> Edge Optimizer         reduz contexto e tokens antes do LLM
  -> Model Manager          resolve modelo, variante e shards
  -> Execution Planner
  -> Scheduler              placement e recursos
  -> Inference Engine       execução matemática do modelo
```

## Notas

- [[inference-api]]: fronteira única para aplicações e agentes; API compatível com o padrão de chat e embeddings.
- [[request-manager]]: decide se e quando uma request executa; separado do scheduler. Não implementado.
- [[edge-optimizer]]: reduz trabalho antes da inferência pesada (cache, retrieval, compressão, roteamento). Não implementado.
- [[model-manager]]: registry de modelos, versões, variantes e shards. Não implementado; os pesos são lidos de caminhos fixos.
- [[inference-engine]]: executa o grafo do modelo (prefill, decode, KV cache, paralelismo). Implementado em CPU para GPT-2 e SmolLM2, com projeções em workers.

Decisões e protocolo: [[adr-007-fronteira-inference-api]], [[adr-009-canal-residente-de-fatias]], [[canal-de-fatias]].

> [!warning] Desvio geral: o MVP concentra a cadeia no gateway
> Hoje o `gateway` faz API, autenticação, validação e admissão por semáforo, e roda o modelo dentro do próprio processo (crate `engine`). Só as projeções lineares vão a workers, e a escolha dos nós é por variável de ambiente. Não há Request Manager, Edge Optimizer nem Model Manager. Os pesos são lidos do disco de cada worker sem versão nem checksum.
> Migração: [[fase-11-request-manager]] (admissão e fila entre a API e o Scheduler) e [[fase-12-model-runtime]] (registry, manifest, variantes, shards, motor fora da API).

> [!note] Códigos J
> Os códigos J1 a J5 nos slices de inferência se referem à fase de inferência. No plano de execução antigo, J é o AI Cluster OS. Ver [[equivalencia-de-fases]].

> [!info] O que já existe
> - Gateway HTTP com `/v1/models`, `/v1/embeddings` (sintético) e `/v1/chat/completions` (GPT-2 small e SmolLM2-135M, com streaming SSE e instruct em ChatML). Slices: [[f08-gateway-e-api]], [[f08-chat-completions]].
> - Motor em CPU com paridade contra `transformers` e KV cache. Slice: [[f08-engine-local]].
> - Canal residente de fatias entre gateway, controller e workers, com reatribuição de faixas quando um worker cai e papel de inferência separado do operador. Slice: [[f08-canal-residente-de-fatias]].
