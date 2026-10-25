---
title: "Modelo de dados — entidades"
aliases: ["data-model/entities", "Modelo de dados — entidades"]
tags: [sistema/modelo-de-dados]
type: sistema/modelo-de-dados
status: vivo
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-06-estado-e-recuperacao]]"]
---

# Modelo de dados — entidades

Entidades do sistema e onde cada uma vive hoje. A coluna "No código" aponta o tipo real; "—" significa que a entidade é só conceitual e ainda não tem representação. Índice: [[modelo-de-dados]]. Relações: [[relacoes]]. Grafo: [[modelo-de-grafo]].

## Entidades centrais

| Entidade | No código | Identificador | Estado de vida | Observação |
|---|---|---|---|---|
| Cluster | — (um controller = um cluster) | `cluster_id` devolvido no registro | implícito | não há tabela de clusters |
| Node | `NodeRecord` (`controller/registry.rs`), `NodeIdentity`, `NodeSnapshot` | `node_id` (texto) | `NodeState` (ver abaixo) | o registro guarda identidade, perfil, saúde, `trusted` e a credencial |
| Device | campo `device_id` em `NodeIdentity` | `device_id` (texto) | — | 1 para 1 com o nó hoje |
| Capability | `CapabilityReport` | — (anexo do nó) | atualizado no registro e na remedição | perfil de baseline e corrente |
| Health | `HealthReport` | — (anexo do nó) | última amostra | separado de capacidade e de confiança |
| Transport | `TransportKind` (`transport` crate: `Tcp`, `SharedMemory`, `Usb`) | — | só `Tcp` (gRPC) é usado | `SharedMemory` e `Usb` são previsões |
| Model | — no controller; gateway tem `LLAMAS` e o GPT-2 | nome de subdiretório (`model_subdir`) | fixo | o diretório de modelos é o identificador hoje |
| ModelVersion | — | — | — | sem versão nem checksum dos pesos |
| ModelVariant | — | — | — | |
| ModelShard | `ShardOp` (protocolo) e `Slice` (`engine/shard.rs`) | `(node_id, model_subdir, tensor, row_start, row_end, by_columns)` | residente no worker, por LRU | ver [[canal-de-fatias]] |
| Request | `request_id` (campo em `WorkUnit`, `EnqueueGraphRequest`) | `request_id` | não é persistida | agrupa unidades; o gateway não guarda o pedido |
| Execution | — | — | — | o grafo de uma requisição é o conjunto de unidades com o mesmo `request_id` |
| ExecutionPlan | — | — | — | existe só implicitamente, pelas dependências `depends_on` |
| WorkUnit | `WorkUnit` | `work_id` | `pending`, `dispatched` (com `WorkLease`), `completed`, `failed` | ver a tabela de estados |
| Tensor | `Matrix` (`common/matrix.rs`), row-major `f64` | — | em memória | sem `tensor://` nem tipos de precisão |
| StateObject | `WorkSnapshot`, `StateReplica` | arquivo de estado (`AI_CLUSTER_STATE`) | gravado a cada mudança, com cópia redundante | ver [[f06-persistencia-e-checkpoint]] |
| Reservation | `Reservation` (`controller/reservations.rs`) | `node_id` (uma reserva por nó) | `expires_at` | TTL padrão de 60 s |
| Metric | `MetricsResponse` | — | snapshot, não série | exportado em texto pelo exportador (`ai_cluster_*`) |
| Event | `Event` | — | anel de 256 | só fatos do controller |
| Trace | `TraceEntry` (por unidade) | `request_id` | derivado de `WorkStore` | sem persistência própria |
| GraphEntity | — | — | — | [[modelo-de-grafo]] |
| GraphRelation | — | — | — | [[modelo-de-grafo]] |

Entidades de runtime que não aparecem na lista acima, mas carregam estado:

| Entidade | No código | Vida |
|---|---|---|
| Sessão de canal | `Session` (`controller/shards.rs`) | uma por nó; substituída na reabertura |
| Operação de fatia em voo | `Pending` (`controller/shards.rs`) | até resposta, erro ou fim da chamada |
| Castigo de nó (gateway) | `Roster` (`gateway/main.rs`) | 30 s por nó que falhou |
| Segredos | `AI_CLUSTER_JOIN_TOKEN`, `AI_CLUSTER_OPERATOR_TOKEN`, `AI_CLUSTER_INFERENCE_TOKEN`, credencial do nó | ver [[protocolo-controller-worker]] |

## Estados

**Nó (`NodeState`, texto no protocolo).** O contrato tem 11 valores: `DISCOVERED`, `PROVISIONING`, `VALIDATING`, `BENCHMARKING`, `READY`, `ACTIVE`, `DEGRADED`, `DRAINING`, `QUARANTINED`, `FAILED`, `OFFLINE`. No código hoje só têm transição:

| Transição | Gatilho |
|---|---|
| (novo) → `READY` | `RegisterNode` |
| `READY` → `OFFLINE` | sem batida por `AI_CLUSTER_HEARTBEAT_GRACE_MS` (15 s) |
| `OFFLINE` → `READY` | nova batida |
| `READY` → `DRAINING` → removido | `LeaveNode` (reenfileira o que estava em voo) |
| qualquer → não confiável, credencial trocada | `RevokeNode`: o registro continua, com `trusted = false`; o que estava em voo volta à fila |

`DISCOVERED`, `PROVISIONING`, `VALIDATING`, `BENCHMARKING`, `ACTIVE`, `DEGRADED`, `QUARANTINED` e `FAILED` são reservados pelo contrato. `QUARANTINED` e `FAILED` já são tratados como não elegíveis.

**Unidade de trabalho.** `queued` (pendente), `dispatched` (com `WorkLease`), `completed`, `failed`. Uma unidade volta a `queued` quando o nó sai ou é revogado, e vai a `failed` depois de `MAX_ATTEMPTS` = 3 despachos. Os dependentes de uma unidade `failed` também falham.

## Regras de identidade

- Identificadores são strings estáveis: `node_id` (escolhido pelo nó), `work_id` e `request_id` (gerados pelo controller quando vazios), `op_id` (só dentro do canal, gerado pelo controller).
- `op_id` não é persistido e não tem significado fora da sessão que o criou.
- Identificador de fatia é a chave residente, não um id: duas réplicas da mesma faixa seriam a mesma chave.

## Separação

| Regra | Como o código se comporta |
|---|---|
| Node ≠ Device | o nó é o processo; `device_id` é o hardware. Hoje são 1:1 e `device_id` é só texto |
| Model ≠ ModelVariant | sem variantes; o diretório é o modelo |
| Request ≠ Execution | a requisição é o `request_id`; a execução é implícita nas unidades |
| ExecutionGraph ≠ PlacementPlan | o grafo existe nas dependências; o placement é decidido unidade a unidade no despacho |
| Health ≠ Capacity ≠ Trust | três campos separados: `HealthReport`, `CapabilityReport` e `trusted`. `eligible` lê os três |

> [!warning] Desvio: estado por nó sem tabela própria
> O controller guarda tudo em memória e em dois arquivos (`AI_CLUSTER_STATE`): estado de trabalho e registro de credenciais. A arquitetura-alvo prevê um estado versionado por entidade. O caminho de migração passa por [[state-manager]] e [[fase-06-estado-e-recuperacao]].

> [!info] O que já existe
> - Nós, saúde, capacidade e confiança: [[f01-entrada-e-saida-de-nos]], [[f02-capability-corrente]], [[f02-modelo-de-saude]], [[f07-aprovacao-de-no]].
> - Unidades, dependências e estados: [[f03-grafo-de-dependencias]], [[f03-tolerancia-a-falhas]], [[work-unit]].
> - Reservas: [[f04-reservas]]. Eventos, métricas e traces: [[f05-eventos]], [[f05-metricas]], [[f05-traces]].
> - Estado persistido e réplica: [[f06-persistencia-e-checkpoint]], [[f06-recuperacao]].
> - Fatias residentes: [[f08-canal-residente-de-fatias]].

> [!todo] O que falta
> - Modelo, versão e variante de modelo, com checksum dos pesos ([[fase-12-model-runtime]]).
> - Execução e plano como entidades próprias, para reexecução e comparação de planos.
> - Tabela de estado com histórico temporal, em vez de snapshot.
> - Identificadores de tensor e precisão (`tensor://`, `precisions`).
> - Entidades de grafo (ver [[modelo-de-grafo]]).
