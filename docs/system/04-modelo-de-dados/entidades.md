---
title: "Modelo de dados — entidades"
aliases: ["data-model/entities", "Modelo de dados — entidades"]
tags: [sistema/modelo-de-dados]
type: sistema/modelo-de-dados
---

# Modelo de dados — entidades

Entidades do sistema, onde cada uma vive e como é identificada. A coluna "Representação" dá a mensagem do protocolo ou o armazenamento que a carrega; "—" significa que a entidade é só conceitual. Índice: [[modelo-de-dados]]. Relações: [[relacoes]]. Grafo: [[modelo-de-grafo]].

## Entidades centrais

| Entidade | Representação | Identificador | Estado de vida | Observação |
|---|---|---|---|---|
| Cluster | um controller = um cluster | `cluster_id`, devolvido no registro | implícito | não há tabela de clusters |
| Node | `NodeIdentity`, `NodeSnapshot` | `node_id` (texto) | `NodeState` (ver abaixo) | guarda identidade, perfil, saúde, confiança e a credencial; a credencial não sai na leitura |
| Device | `NodeIdentity.device_id` | `device_id` (texto) | — | hoje 1 para 1 com o nó |
| Capability | `CapabilityReport` | anexo do nó | baseline no registro; corrente depois de cada remedição | |
| Health | `HealthReport` | anexo do nó | última amostra | separada de capacidade e de confiança |
| Attestation | `NodeSnapshot.attested` | — | calculada na leitura | verdadeira quando versão e medida do slot batem com a release desejada |
| Release | `Release` | `version` | desejada até ser trocada ou cancelada | uma por cluster; guardada em arquivo se `AI_CLUSTER_RELEASE_STATE` estiver definido |
| Transport | gRPC sobre TCP | — | única forma em uso | outros transportes são previstos, não existem |
| Model | nome do subdiretório (`model_subdir`) | `model_subdir` | fixo | o diretório é o modelo; não há versão nem checksum dos pesos |
| ModelShard | `ShardOp`, `ShardCompute` | `(model_subdir, tensor, row_start, row_end, by_columns)` | residente no worker, despejada por LRU | ver [[canal-de-fatias]] |
| Request | `request_id` em `WorkUnit` e nos pedidos de enfileiramento | `request_id` | não é persistida como entidade | agrupa unidades e pode ser cancelada |
| Execução | — (implícita) | — | — | o grafo de uma requisição é o conjunto de unidades com o mesmo `request_id`, ligadas por `depends_on` |
| WorkUnit | `WorkUnit` | `work_id` | `queued`, `dispatched`, `completed`, `failed` | carrega prioridade e dependências |
| WorkLease | `WorkLease` | `work_id` | enquanto a unidade estiver em voo | a posse: quem segura a unidade e desde quando |
| Tensor | matriz `f64`, row-major | — | em memória | nenhum identificador de tensor nem tipo de precisão |
| StateObject | `WorkSnapshot` (arquivo), `StateReplica` (cópia num nó) | `epoch_unix_ms` | gravado a cada mudança, com cópia redundante | ver [[modelo-de-dados]] e [[state-manager]] |
| Reservation | `Reservation` | `node_id` (uma por nó) | `expires_at_unix_ms` | TTL padrão de 60 s; não sobrevive a reinício |
| Metric | `MetricsResponse` | — | snapshot, não série | exportado em texto com prefixo `ai_cluster_` |
| Event | `Event` | — | anel de 256 | só fatos do controller; `AI_CLUSTER_EVENTS` guarda em arquivo |
| Trace | `TraceEntry`, uma por unidade | `request_id` | derivado do estado de trabalho | sem persistência própria |

Runtime que carrega estado e não é entidade do protocolo:

| Entidade | Vida |
|---|---|
| Sessão do canal de fatias | uma por nó; substituída quando o nó reabre o canal |
| Operação de fatia em voo | até resposta, erro ou fim da chamada |
| Castigo de nó no gateway | 30 s por nó que falhou |
| Segredos | tokens de join, de operador e de inferência, e a credencial do nó ([[protocolo-controller-worker]]) |

> Planejado: `ModelVersion` e `ModelVariant`, com checksum dos pesos; identificadores e tipos de precisão de tensor; execução e plano como entidades próprias, para reexecutar e comparar planos.

## Estados

**Nó (`NodeState`, texto no protocolo).** O contrato define 11 valores: `DISCOVERED`, `PROVISIONING`, `VALIDATING`, `BENCHMARKING`, `READY`, `ACTIVE`, `DEGRADED`, `DRAINING`, `QUARANTINED`, `FAILED`, `OFFLINE`. Hoje o controller produz `READY`, `OFFLINE`, `DRAINING` e `QUARANTINED`. Os demais são reservados: o contrato os prevê, mas nenhum caminho os produz.

| Transição | Gatilho |
|---|---|
| (novo) → `READY` | `RegisterNode`. O nó nasce **não confiável** (`trusted = false`) e não recebe trabalho |
| não confiável → confiável | `ApproveNode` |
| `READY` → `OFFLINE` | sem batida por `AI_CLUSTER_HEARTBEAT_GRACE_MS` (padrão 15 s) |
| `OFFLINE` → `READY` | nova batida |
| `READY` → `DRAINING` → removido | `LeaveNode`; as unidades em voo voltam à fila |
| qualquer → `QUARANTINED` | três resultados inválidos seguidos. O nó continua visível, mas não é candidato a trabalho |
| `QUARANTINED` → candidato | `ApproveNode` de novo |
| qualquer → não confiável, credencial invalidada | `RevokeNode`: o registro continua, com `trusted = false`; o que estava em voo volta à fila |

`FAILED` é tratado como não elegível, mas nenhum caminho o produz hoje.

**Unidade de trabalho.** `queued` (na fila), `dispatched` (com `WorkLease`), `completed` e `failed`. Uma unidade volta a `queued` quando o nó que a segura sai ou é revogado. Vai a `failed` depois de 3 despachos (`MAX_ATTEMPTS`). Os dependentes de uma unidade `failed` também falham. A prioridade vai de 0 a 9: a classe mais alta é despachada antes, a fila é FIFO dentro da classe, e o envelhecimento impede que uma classe alta deixe a baixa esperando para sempre.

## Regras de identidade

- Identificadores são strings estáveis: `node_id` (escolhido pelo nó), `work_id` e `request_id` (gerados pelo controller quando vazios), `op_id` (gerado pelo controller, só dentro do canal).
- `op_id` não é persistido e não tem significado fora da sessão que o criou.
- Identificador de fatia é a chave residente, não um id: duas réplicas da mesma faixa seriam a mesma chave.

## Separação

| Regra | Comportamento |
|---|---|
| Node ≠ Device | o nó é o processo; `device_id` é o hardware. Hoje são 1 para 1, e `device_id` é só texto |
| Model ≠ ModelVariant | sem variantes: o diretório é o modelo |
| Request ≠ Execução | a requisição é o `request_id`; a execução é implícita nas unidades |
| Grafo de execução ≠ plano de placement | o grafo existe nas dependências; o placement é decidido unidade a unidade no despacho |
| Saúde ≠ Capacidade ≠ Confiança | três campos separados: `HealthReport`, `CapabilityReport` e `trusted`. A elegibilidade lê os três, mais a quarentena |

## Limites

- **Estado em memória com arquivos opcionais.** O controller guarda tudo em memória. O estado de trabalho (`AI_CLUSTER_STATE`) e o registro de nós e credenciais (`AI_CLUSTER_NODES`) são arquivos; sem eles, um reinício perde esse estado, a menos que um nó guarde a cópia dele (ver [[relacoes]]).
- **Sem histórico.** O estado é um snapshot: não há histórico temporal de nós nem de unidades.

> Planejado: tabela de estado com histórico temporal, em vez de snapshot.
