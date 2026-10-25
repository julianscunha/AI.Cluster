---
title: "Modelo de dados — modelo de grafo"
aliases: ["data-model/graph-model", "Modelo de dados — modelo de grafo"]
tags: [sistema/modelo-de-dados]
type: sistema/modelo-de-dados
status: decidido
implementacao: nao-iniciado
fases: ["[[fase-13-graph-engine]]"]
---

# Modelo de dados — modelo de grafo

Modelo do grafo que o cluster usa para consulta e travessia: topologia, dependências, execução e incidentes. O grafo é uma **projeção** do estado, não a fonte da verdade ([[adr-004-grafo-como-projecao]]). Índice: [[modelo-de-dados]]. Entidades de origem: [[entidades]].

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

## Mapeamento de origem

Os tipos de nó e de aresta acima são alvo. Cada um tem origem conhecida no modelo de dados atual:

| Tipo do grafo | Origem em [[entidades]] | Aresta | Origem em [[relacoes]] |
|---|---|---|---|
| Node | `NodeRecord` | HOSTS (Cluster → Node) | implícito |
| Device, ComputeDevice | `device_id`, `accelerators` | — | 1 para 1 com o nó |
| Storage | `storage_classes` | HOSTS (Node → Storage) | campo do perfil |
| NetworkInterface | — | CONNECTED_TO | não armazenada |
| Model, ModelShard | `model_subdir`, `ShardOp` | HOSTS, STORES | faixa no worker |
| Request, Execution, WorkUnit | `request_id`, `WorkUnit`, `WorkLease` | EXECUTES, DEPENDS_ON, PRODUCES, CONSUMES | `depends_on`, `*_from` |
| StateObject | `WorkSnapshot`, `StateReplica` | STORES, REPLICATES | réplica no nó |
| Metric, Event | `MetricsResponse`, `Event` | AFFECTS | — |

Hoje a única visão de grafo é o trace de uma requisição (`GetTrace`), que lista unidades por estado e na ordem de execução. Não há travessia nem consulta por aresta.

> [!warning] Grafo como projeção, sem banco próprio
> A decisão ([[adr-004-grafo-como-projecao]]) é que o grafo seja uma projeção reconstruível. Hoje não há projeção: `GetTrace`, `ListNodes` e `GetMetrics` respondem direto do estado do controller. O grafo só se adota com ganho medido ([[fase-13-graph-engine]]).

> [!info] O que já existe
> - Nada de grafo no código. A consulta de caminho é `GetTrace` ([[f05-traces]]), que é uma visão do grafo de dependências de uma requisição ([[f03-grafo-de-dependencias]]).
> - Eventos que alimentariam as arestas: [[f05-eventos]].

> [!todo] O que falta
> - Engine de grafo, projeção e travessia, em [[graph-engine]] e [[fase-13-graph-engine]].
> - Tipos `Document`, `Entity`, `Incident`, `Skill` e `Tool`. Nenhum tem origem no sistema atual; o uso previsto para eles está em [[edge-optimizer]].
> - Arestas `SIMILAR_TO`, `RELATED_TO`, `SUPPORTS`, `REQUIRES` e `AFFECTS`: sem fonte de dados hoje.
