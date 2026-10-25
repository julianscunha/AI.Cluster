---
title: "Modelo de dados — relações"
aliases: ["data-model/relationships", "Modelo de dados — relações"]
tags: [sistema/modelo-de-dados]
type: sistema/modelo-de-dados
status: vivo
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-04-escalonamento]]", "[[fase-06-estado-e-recuperacao]]"]
---

# Modelo de dados — relações

Relações entre as entidades de [[entidades]]. Cada relação diz como é representada hoje: por campo (uma chave), por lista, por mapa em memória ou só conceitual. Índice: [[modelo-de-dados]].

## Relações centrais

```text
Cluster HAS_NODE Node                 implícito: um controller
Node REPRESENTS Device                1 para 1 (device_id)
Node HAS_CAPABILITY Capability        último perfil; current_* substitui o baseline quando há remedição
Node HAS_HEALTH Health                última amostra
Node CONNECTED_TO Node                não armazenada; o enlace é medido pelo nó contra o controller (ProbeLink)
Model HAS_VERSION ModelVersion        não existe
ModelVersion HAS_VARIANT ModelVariant não existe
ModelVariant HAS_SHARD ModelShard     não existe; a faixa aponta direto para o modelo (model_subdir)
Request CREATES Execution             implícito: unidades com o mesmo request_id
Execution USES ExecutionPlan          não existe
ExecutionPlan CONTAINS WorkUnit       implícito: pelo conjunto e por depends_on
WorkUnit CONSUMES Tensor              matrix_a / matrix_b, ou matrix_*_from (saída de outra unidade)
WorkUnit PRODUCES Tensor              WorkOutput, guardado até os dependentes lerem
WorkUnit DISPATCHED_TO Node           WorkLease (node_id e dispatched_at)
Execution HAS_STATE StateObject       WorkSnapshot gravado no arquivo de estado
ModelShard HOSTED_ON Node             ShardOp.node_id; a faixa mora no worker, não no controller
StateObject REPLICATED_TO Node        StateReplica, guardada pelo nó (I4)
Request EXECUTED_BY Node              implícito: trace por request_id
Reservation HOLDS Node FOR Request    Reservation (node_id → request_id)
Node EXPERIENCED Event                Event.detail carrega o node_id; sem chave estrangeira
Trace DESCRIBES Execution             GetTrace(request_id)
Session BINDS Node                    uma sessão de canal por nó (ShardHub)
Operation WAITS_ON Session            operação pendente de fatia; cai com a sessão
```

## Cardinalidades e posse

| Relação | Cardinalidade | Quem é dono |
|---|---|---|
| Node — Capability | 1 para 1 atual (mais o baseline) | controller, no registro |
| Node — Health | 1 para 1 (última) | controller |
| WorkUnit — Node | 0 ou 1 (só em voo) | `WorkLease` |
| Node — Reservation | 0 ou 1 | controller; uma reserva por nó |
| Node — Session | 0 ou 1 | controller; uma sessão por nó |
| Request — WorkUnit | 1 para N | `request_id` na unidade |
| WorkUnit — WorkUnit | N para N (`depends_on`) | a unidade dependente |
| ModelShard — Node | N para 1 por faixa | `ShardOp` no pedido; o nó é quem calcula |

## Timestamps e versão

Onde placement ou estado mudam ao longo do tempo, a relação carrega instante ou época:

| Relação | Campo |
|---|---|
| Node — registro | `registered_at_unix_ms`, `last_heartbeat_unix_ms` |
| WorkUnit — Node (lease) | `dispatched_at_unix_ms`, `predicted_compute_us` |
| Reservation | `expires_at_unix_ms` |
| StateObject | `epoch_unix_ms` (no arquivo e na réplica) |
| Node — réplica | `replica_epoch_unix_ms` na batida |
| Event | `at_unix_ms` (relógio do controller; relógio do nó nunca ordena) |

## Desvios

> [!warning] Relações que dependem de estado em memória
> Lease, sessão e reserva vivem só no processo do controller; um restart perde sessões de canal e reservas. O lease é restaurado com o dono do arquivo de estado, para a unidade não ser reoferecida a outro nó; a sessão de canal é refeita pelo worker na reconexão. Ver [[f06-recuperacao]].

> [!info] O que já existe
> - Nó, capacidade e saúde: [[f01-entrada-e-saida-de-nos]], [[f02-capability-corrente]], [[f02-modelo-de-saude]].
> - Leases e dependências: [[f03-grafo-de-dependencias]], [[f03-tolerancia-a-falhas]].
> - Reservas e placement: [[f04-reservas]], [[f04-placement]].
> - Traces: [[f05-traces]]. Persistência: [[f06-persistencia-e-checkpoint]].
> - Fatias por nó: [[f08-canal-residente-de-fatias]].

> [!todo] O que falta
> - Model, ModelVersion e ModelVariant com as relações HAS_VERSION e HAS_VARIANT ([[fase-12-model-runtime]]).
> - Execução como entidade, com HAS_STATE e USES, para reexecutar um plano.
> - Relações de enlace (`CONNECTED_TO`) armazenadas com a medida de vazão e latência ([[f02-descoberta-de-rede]]).
> - Histórico das relações de placement, para o modelo de desempenho ([[modelo-de-desempenho]]).
