# 16 — Graph Engine

## Objective

Explorar relações estruturadas para reduzir buscas, localizar dependências e orientar execução/scheduling.

## Graph domains

### Knowledge Graph

Document, chunk, entity, fact, incident, service.

### Hardware Graph

Node, CPU, RAM, GPU, NPU, storage.

### Network Graph

Links e suas métricas.

### Execution Graph

Operations, dependencies, tensors, Work Units.

### Observability Graph

Request, plan, node, event, metric, incident.

## Graphify role

O projeto deve adotar a ideia de graph-based retrieval quando relações e travessals reduzirem custo de busca. A tecnologia específica deve ser validada no benchmark; não presumir que todo search é melhor via graph.

## Projection

A fonte primária de dados permanece nos stores operacionais. O grafo pode ser reconstruído.

## Queries úteis

- nearest eligible node;
- nodes hosting shard;
- related incidents;
- affected requests;
- candidate skills/tools;
- path between nodes;
- state replicas.
