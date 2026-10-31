---
title: "Modelo de dados — relações"
aliases: ["data-model/relationships", "Modelo de dados — relações"]
tags: [sistema/modelo-de-dados]
type: sistema/modelo-de-dados
---

# Modelo de dados — relações

Relações entre as entidades de [[entidades]]. Cada relação diz como é representada: por campo (uma chave), por lista, por estrutura em memória ou só conceitualmente. Índice: [[modelo-de-dados]].

## Relações centrais

```text
Cluster HAS_NODE Node                  implícito: um controller
Node REPRESENTS Device                 1 para 1 (device_id)
Node HAS_CAPABILITY Capability         último perfil; a medida corrente substitui o baseline quando existe
Node HAS_HEALTH Health                 última amostra
Node RUNS Release                      runtime_version e image_sha256 do registro; attested compara com a desejada
Node CONNECTED_TO Node                 não armazenada; o enlace é medido pelo worker contra o controller (ProbeLink)
Model HAS_VERSION ModelVersion         Planejado
ModelVersion HAS_VARIANT ModelVariant  Planejado
ModelVariant HAS_SHARD ModelShard      Planejado; hoje a fatia aponta direto para o modelo (model_subdir)
Request CREATES Execution              implícito: unidades com o mesmo request_id
ExecutionPlan CONTAINS WorkUnit        implícito: pelo conjunto e por depends_on
WorkUnit CONSUMES Tensor               matrix_a / matrix_b, ou matrix_*_from (saída de outra unidade)
WorkUnit PRODUCES Tensor               WorkOutput, guardado até os dependentes lerem
WorkUnit DISPATCHED_TO Node            WorkLease (node_id e dispatched_at)
Execution HAS_STATE StateObject        WorkSnapshot gravado no arquivo de estado
ModelShard HOSTED_ON Node              ShardOp.node_id; a faixa mora no worker, não no controller
StateObject REPLICATED_TO Node         StateReplica, guardada pelo worker
Request EXECUTED_BY Node               implícito: trace por request_id
Reservation HOLDS Node FOR Request     Reservation (node_id → request_id)
Node EXPERIENCED Event                 Event.detail carrega o node_id; sem chave estrangeira
Trace DESCRIBES Execution              GetTrace(request_id)
Sessão do canal BINDS Node             uma sessão de canal por worker
Operação de fatia WAITS_ON Sessão      operação pendente de fatia; cai com a sessão
```

## Cardinalidades e posse

| Relação | Cardinalidade | Quem é dono |
|---|---|---|
| Node — Capability | 1 para 1 atual (mais o baseline) | controller, no registro |
| Node — Health | 1 para 1 (última amostra) | controller |
| WorkUnit — Node | 0 ou 1 (só enquanto em voo) | `WorkLease` |
| Node — Reservation | 0 ou 1 | controller; uma reserva por worker |
| Node — Sessão do canal | 0 ou 1 | controller; uma sessão por worker |
| Request — WorkUnit | 1 para N | `request_id` na unidade |
| WorkUnit — WorkUnit | N para N (`depends_on`) | a unidade dependente |
| ModelShard — Node | N para 1 por faixa | `ShardOp` no pedido; o worker é quem calcula |

## Timestamps e versão

Onde o placement ou o estado mudam ao longo do tempo, a relação carrega instante ou época:

| Relação | Campo |
|---|---|
| Node — registro | `registered_at_unix_ms`, `last_heartbeat_unix_ms` |
| WorkUnit — Node (lease) | `dispatched_at_unix_ms`, `predicted_compute_us` |
| Reservation | `expires_at_unix_ms` |
| StateObject | `epoch_unix_ms` (no arquivo e na réplica) |
| Node — réplica | `replica_epoch_unix_ms` na batida |
| Event | `at_unix_ms` (relógio do controller; o relógio do worker nunca ordena) |

## Réplica de estado

O controller grava o estado em dois arquivos locais. Cada worker também pode guardar uma cópia do estado, que viaja em `RegisterNode` e `Heartbeat`. Um controller que perdeu os arquivos adota a cópia mais nova que receber; a época decide qual é a mais nova. Os workers só recebem uma cópia quando a do controller está mais adiantada que a que eles já têm.

## Limites

> [!warning] Relações que dependem de estado em memória
> Lease, sessão de canal e reserva vivem no processo do controller. Um reinício perde sessões e reservas: a sessão é refeita pelo worker na reconexão, e a reserva não é restaurada. O lease é restaurado junto com o arquivo de estado, para a unidade não ser reoferecida a outro worker.

> Planejado: `Model`, `ModelVersion` e `ModelVariant` com as relações `HAS_VERSION` e `HAS_VARIANT`; execução como entidade, com `HAS_STATE` e `USES`, para reexecutar um plano; relações de enlace (`CONNECTED_TO`) armazenadas com vazão e latência medidas; histórico das relações de placement, para o [[modelo-de-desempenho]].
