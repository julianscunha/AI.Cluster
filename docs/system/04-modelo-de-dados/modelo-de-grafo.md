---
title: "Modelo de dados — modelo de grafo"
aliases: ["data-model/graph-model", "Modelo de dados — modelo de grafo"]
tags: [sistema/modelo-de-dados]
type: sistema/modelo-de-dados
---

# Modelo de dados — modelo de grafo

Modelo do grafo que o cluster usará para consulta e travessia: topologia, dependências, execução e incidentes. O grafo é uma **projeção** do estado, não a fonte da verdade ([[adr-004-grafo-como-projecao]]). Índice: [[modelo-de-dados]]. Entidades de origem: [[entidades]].

> Planejado: todo este modelo. Hoje não existe projeção de grafo nem engine de grafo. As consultas respondem direto do estado do controller (`GetTrace`, `ListNodes`, `GetMetrics`). Ver [[graph-engine]].

## Tipos de nó do grafo

```text
Cluster
Node
Device
ComputeDevice
Storage
NetworkInterface
Model
ModelShard
Request
Execution
ExecutionPlan
WorkUnit
Tensor
StateObject
Document
Entity
Incident
Metric
Event
Skill
Tool
```

## Tipos de aresta

```text
HOSTS
CONNECTED_TO
DEPENDS_ON
PRODUCES
CONSUMES
EXECUTES
STORES
REPLICATES
RELATED_TO
AFFECTS
SIMILAR_TO
REQUIRES
SUPPORTS
```

## Princípios

- O grafo é otimizado para consulta e travessia.
- Object stores continuam sendo a autoridade para payloads grandes; o grafo guarda referências.
- O grafo deve ser reconstruível a partir do estado de origem.
- Arestas de topologia carregam métricas e saúde quando apropriado.
- Estado temporal é versionado.

## Origem de cada tipo

| Tipo do grafo | Origem em [[entidades]] | Aresta | Origem em [[relacoes]] |
|---|---|---|---|
| Node | `NodeIdentity`, `NodeSnapshot` | HOSTS (Cluster → Node) | implícito |
| Device, ComputeDevice | `device_id`, `accelerators` | — | 1 para 1 com o nó |
| Storage | `storage_classes` | HOSTS (Node → Storage) | campo do perfil |
| NetworkInterface | — | CONNECTED_TO | não armazenada |
| Model, ModelShard | `model_subdir`, `ShardOp` | HOSTS, STORES | faixa no worker |
| Request, Execution, WorkUnit | `request_id`, `WorkUnit`, `WorkLease` | EXECUTES, DEPENDS_ON, PRODUCES, CONSUMES | `depends_on`, `*_from` |
| StateObject | `WorkSnapshot`, `StateReplica` | STORES, REPLICATES | réplica no nó |
| Metric, Event | `MetricsResponse`, `Event` | AFFECTS | — |

Hoje a única visão de grafo é o trace de uma requisição (`GetTrace`), que lista as unidades por estado e, dentro de cada estado, na ordem de execução. Não há travessia nem consulta por aresta.

> [!warning] Grafo como projeção, sem banco próprio
> A projeção será reconstruível a partir do estado de origem, e só se adotará com ganho medido ([[adr-004-grafo-como-projecao]]). Até lá, não há uma segunda fonte de dados para o grafo.

## Limites

- Os tipos `Document`, `Entity`, `Incident`, `Skill` e `Tool` não têm origem no sistema atual. O uso previsto para eles está em [[edge-optimizer]].
- As arestas `SIMILAR_TO`, `RELATED_TO`, `SUPPORTS`, `REQUIRES` e `AFFECTS` não têm fonte de dados hoje.
