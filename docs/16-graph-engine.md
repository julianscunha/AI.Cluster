---
title: "16 — Graph Engine"
type: spec
status: draft
tags: [spec, graph-engine]
---

# 16 — Motor de grafo

## Objetivo

Explorar relações estruturadas para reduzir buscas, localizar dependências e orientar execução/scheduling.

## Domínios de grafo

### Grafo de conhecimento

Document, chunk, entity, fact, incident, service.

### Grafo de hardware

Node, CPU, RAM, GPU, NPU, storage.

### Grafo de rede

Links e suas métricas.

### Grafo de execução

Operations, dependencies, tensors, Work Units.

### Grafo de observabilidade

Request, plan, node, event, metric, incident.

## Papel do Graphify

O projeto deve adotar a ideia de graph-based retrieval quando relações e travessals reduzirem custo de busca. A tecnologia específica deve ser validada no benchmark; não presumir que todo search é melhor via graph.

## Projeção

A fonte primária de dados permanece nos stores operacionais. O grafo pode ser reconstruído.

## Queries úteis

- nearest eligible node;
- nodes hosting shard;
- related incidents;
- affected requests;
- candidate skills/tools;
- path between nodes;
- state replicas.
