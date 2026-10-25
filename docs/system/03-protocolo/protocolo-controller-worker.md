---
title: "Protocolo Controller ↔ Worker"
aliases: ["protocol/controller-worker", "Protocolo controller-worker"]
tags: [sistema/protocolo]
type: sistema/protocolo
status: vivo
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-01-nucleo-do-cluster]]", "[[fase-07-seguranca-base]]"]
---

# Protocolo Controller ↔ Worker

Como o controller e os workers conversam: transporte, quem inicia cada conversa, identidade, metadados e limites. O catálogo de cada RPC e mensagem está em [[mensagens]]; o canal residente de fatias está em [[canal-de-fatias]]. Índice: [[protocolo]].

## Transporte

- gRPC sobre HTTP/2, payload em Protobuf (ADR em [[adr-003-controller-worker]]). O schema é `code/proto/cluster.proto`, pacote `ai.cluster.v1`, com dois serviços no mesmo servidor do controller: `ClusterControl` e `LinkProbe`.
- **TLS**: o controller sobe com `AI_CLUSTER_TLS_CERT` e `AI_CLUSTER_TLS_KEY`; sem eles não sobe, a menos de `AI_CLUSTER_INSECURE=1` (rede fechada). O cliente usa `https://` com `AI_CLUSTER_TLS_CA` (e `AI_CLUSTER_TLS_DOMAIN` quando o endpoint é um IP ou alias). `http://` é recusado sem `AI_CLUSTER_INSECURE`, para que um token nunca vá em claro por padrão.
- **Keepalive HTTP/2 de 10 s** nos dois lados, com keepalive também ocioso: detecta conexão meio-aberta (Wi-Fi caído, worker suspenso) em segundos.
- **Tamanho**: 64 MiB por mensagem nas duas direções (`MAX_MESSAGE_BYTES`). `ProbeLink` tem teto próprio de 4 MiB (`MAX_PROBE_BYTES`), para que a sonda não herde o limite de dados.
- **Quem inicia**: sempre o worker. O controller nunca abre conexão para um worker, e o worker não expõe porta. O trabalho é puxado (`RequestWork`); o canal de fatias é um stream que o worker abre e o controller só usa.

## Fluxo de interações

| Interação | Iniciador | RPC | Padrão |
|---|---|---|---|
| Registro | worker | `RegisterNode` | unário; devolve `node_credential` |
| Aprovação | operador | `ApproveNode` | unário; nó novo espera aprovação (S2) |
| Batida de vida | worker | `Heartbeat` | unário a cada 5 s; sem batida por 15 s (`AI_CLUSTER_HEARTBEAT_GRACE_MS`) vira `OFFLINE` |
| Saúde e perfil | worker | `ReportHealth` | unário; leva o perfil remedido quando houver |
| Trabalho | worker | `RequestWork` → `SubmitWorkResult` | pull; `WORK_POLL_IDLE_MS` = 200 ms ocioso |
| Sonda de enlace | worker | `ProbeLink` | upload de 2 MiB cronometrado (C2) |
| Canal de fatias | worker abre | `ShardChannel` | stream bidirecional residente |
| Inferência fatiada | gateway (via operador ou token de inferência) | `ShardMatmul` | unário; o controller repassa pelo canal |
| Saída do nó | worker | `LeaveNode` | unário; reenfileira o que estava em voo |
| Enfileirar | operador | `EnqueueWork`, `EnqueueGraph` | unário |
| Observação | operador | `ListNodes`, `ListEvents`, `GetMetrics`, `GetTrace`, `WorkReport` | unário, leitura |

## Mensagens de ciclo de vida: alvo e implementado

A lista de mensagens do protocolo proposto foi substituída pelos RPCs reais. Esta tabela mostra para onde cada item foi.

| Mensagem proposta | Estado | Implementado como |
|---|---|---|
| `HELLO` | parcial | não há handshake separado: a versão vai no cabeçalho e a identidade em `RegisterNode` |
| `REGISTER` | implementado | `RegisterNode` |
| `CAPABILITIES` | implementado | `RegisterNodeRequest.capability` e, remedido, `HealthStreamRequest.capability` |
| `BENCHMARK_REQUEST` / `BENCHMARK_RESULT` | parcial | o benchmark roda no worker no registro; não há RPC de pedido |
| `HEARTBEAT` | implementado | `Heartbeat` |
| `HEALTH_REPORT` | implementado | `ReportHealth` |
| `TELEMETRY` | não iniciado | observação por pull (`ListEvents`, `GetMetrics`); não há stream de telemetria |
| `LOAD_MODEL` / `UNLOAD_MODEL` | não iniciado | o modelo é um subdiretório de `AI_CLUSTER_MODEL_DIR`, nomeado em cada `ShardOp` |
| `LOAD_SHARD` / `UNLOAD_SHARD` | parcial | implícito: a fatia carrega no primeiro uso e sai por LRU ([[canal-de-fatias]]) |
| `ASSIGN_WORK` | parcial | invertido: o worker puxa com `RequestWork` |
| `CANCEL_WORK` | não iniciado | não há RPC; só o `ShardMatmul` cancela (abandono da chamada) |
| `CHECKPOINT` / `RESTORE` | parcial | `StateReplica` trafega dentro de `Heartbeat` e `RegisterNode`; sem mensagem explícita |
| `DRAIN` | parcial | sem RPC próprio; `LeaveNode` marca o nó `DRAINING` e o remove |
| `LEAVE` | implementado | `LeaveNode` |

## Identidade e autenticação

| Segredo | Variável | Quem apresenta | Vale para |
|---|---|---|---|
| Token de join | `AI_CLUSTER_JOIN_TOKEN` | todo nó e cliente | todos os RPCs de nó e leitura |
| Token de operador | `AI_CLUSTER_OPERATOR_TOKEN` | CLI do operador | tudo que o join não cobre (aprovar, revogar, enfileirar, reservar, `GetOutput`) |
| Token de inferência | `AI_CLUSTER_INFERENCE_TOKEN` | gateway | **só** `ShardMatmul` ([[canal-de-fatias]]) |
| Credencial do nó | emitida em `RegisterNode` | o próprio nó, no cabeçalho `x-ai-cluster-node-auth` | chamadas de nó (`Heartbeat`, `RequestWork`, `ShardChannel`…) |

- Os três tokens de servidor precisam ser distintos, ou o controller não sobe.
- Chamadas de nó aceitam a credencial do nó ou o token de operador. O canal de fatias **não** aceita o operador: exige a credencial do nó (`channel_allowed`).
- O nó precisa estar registrado, aprovado e em estado elegível; revogação e saída derrubam a sessão.

Detalhes de quem chama o quê estão em [[mensagens]], na coluna de autorização.

## Metadados de requisição

Um interceptor cliente põe os metadados em toda chamada; nenhum ponto de chamada os repete.

| Campo | Cabeçalho | Estado |
|---|---|---|
| versão do protocolo | `x-ai-cluster-protocol-version` | emitido `v1`; o controller rejeita qualquer outra com `FailedPrecondition` |
| correlação | `x-ai-cluster-correlation-id` | valor novo a cada chamada |
| autenticação | `x-ai-cluster-auth` | join, operador ou inferência |
| credencial do nó | `x-ai-cluster-node-auth` | credencial emitida no registro |
| nó do canal | `x-ai-cluster-node-id` | só na abertura de `ShardChannel` |
| nó culpado | `x-ai-cluster-failed-node` | só em resposta de erro de `ShardMatmul` |
| idempotência | — | estrutural: a unidade sai da tabela de despacho ao completar, então uma submissão duplicada não tem o que completar |

O `node_id` também viaja no payload protobuf, onde o controller precisa dele.

## Streaming e backpressure

- **Canal de fatias**: stream bidirecional com fila de 64 quadros por sentido; fila cheia falha na hora, sem esperar.
- **Telemetria**: não há stream. O operador lê por pull.
- **Transferências longas**: `WorkUnit` vai inteiro numa mensagem (limite de 64 MiB); não há chunking nem streaming de operandos.

## Compatibilidade

O controller conhece as capacidades de cada nó por `CapabilityReport` no registro e a cada remedição. Negociação de features ainda não existe; ver [[versionamento]].

> [!warning] Desvios da arquitetura-alvo
> - **Fila de puxar em vez de despacho**: `RequestWork` com espera de 200 ms ocioso põe um piso de dezenas de ms em cada operação dependente. Por isso o canal de fatias empurra. A migração prevista é generalizar o canal residente para trabalho (ver [[execution-planner]] e [[scheduler]]).
> - **Um segredo de join para todos os nós**: a identidade por nó vem da credencial do registro (S3b). mTLS e attestation ficam para [[fase-19-hardening-de-producao]].
> - **Rejeição por chamada, não por nó**: versão incompatível não registra nenhum nó, mas o controller não põe nó em quarentena por versão.
> - **Sem idempotency key explícita**: a garantia é estrutural, pela posse da unidade em [[work-unit]].

> [!info] O que já existe
> - Registro, batida, saúde e saída: [[f01-entrada-e-saida-de-nos]], [[f02-modelo-de-saude]].
> - Trabalho puxado e tolerância a falha: [[f03-tolerancia-a-falhas]], [[f00-mvp]].
> - Segurança: [[f07-token-de-join]], [[f07-aprovacao-de-no]], [[f07-operador-e-credencial-por-no]], [[f07-tls]].
> - Canal residente e gateway: [[f08-canal-residente-de-fatias]], [[f08-gateway-e-api]].

> [!todo] Falta
> - `CANCEL_WORK` como mensagem de controle (hoje só o abandono do `ShardMatmul`).
> - Telemetria por stream com backpressure.
> - Negociação de features no registro.
> - Quarentena por versão e por saúde, com mensagem própria.
> - Chunking ou streaming de operandos acima de 64 MiB.
