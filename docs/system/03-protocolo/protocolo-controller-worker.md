---
title: "Protocolo Controller ↔ Worker"
aliases: ["protocol/controller-worker", "Protocolo controller-worker"]
tags: [sistema/protocolo]
type: sistema/protocolo
---

# Protocolo Controller ↔ Worker

Como o controller e os workers conversam: transporte, quem inicia cada conversa, identidade, metadados e limites. O catálogo de cada RPC e mensagem está em [[mensagens]]; o canal residente de fatias está em [[canal-de-fatias]]. Índice: [[protocolo]].

## Transporte

- gRPC sobre HTTP/2, com payload em Protobuf (ver [[adr-003-controller-worker]]). O schema é `code/proto/cluster.proto`, pacote `ai.cluster.v1`, com dois serviços no mesmo servidor do controller: `ClusterControl` e `LinkProbe`.
- **TLS.** O controller exige `AI_CLUSTER_TLS_CERT` e `AI_CLUSTER_TLS_KEY` e não sobe sem eles, a menos de `AI_CLUSTER_INSECURE=1`, que só se usa em rede fechada. O cliente fala `https://` e verifica o controller com `AI_CLUSTER_TLS_CA`; `AI_CLUSTER_TLS_DOMAIN` entra quando o endereço é um IP ou um alias. `http://` é recusado sem `AI_CLUSTER_INSECURE`, para que um token nunca vá em claro por padrão.
- **mTLS opcional** ([[adr-016-mtls-opcional-no-plano-de-controle]]). Com `AI_CLUSTER_TLS_CLIENT_CA` no controller, só entram clientes com certificado assinado por essa CA. O cliente apresenta o certificado com `AI_CLUSTER_TLS_CLIENT_CERT` e `AI_CLUSTER_TLS_CLIENT_KEY`.
- **Keepalive HTTP/2 de 10 s** nos dois lados, inclusive ocioso. Detecta conexão meio aberta (Wi-Fi caído, nó suspenso) em segundos.
- **Tamanho.** 64 MiB por mensagem nas duas direções. `ProbeLink` tem teto próprio de 4 MiB, para a sonda não herdar o limite de dados.
- **Quem inicia.** Sempre o worker. O controller nunca abre conexão para um worker, e o worker não expõe porta. O trabalho é puxado (`RequestWork`); o canal de fatias é um stream que o worker abre e o controller só usa.

## Fluxo de interações

| Interação | Iniciador | RPC | Padrão |
|---|---|---|---|
| Registro | worker | `RegisterNode` | unário; devolve `node_credential` |
| Aprovação | operador | `ApproveNode` | unário; nó novo não é confiável até ser aprovado |
| Atualização do sistema | operador define; o nó recebe | `SetRelease`, depois `Heartbeat` | a release chega na resposta da batida ([[versionamento]]) |
| Batida de vida | worker | `Heartbeat` | unário a cada 5 s; sem batida por 15 s (`AI_CLUSTER_HEARTBEAT_GRACE_MS`) o nó vira `OFFLINE` |
| Saúde e perfil | worker | `ReportHealth` | unário; leva o perfil remedido quando houver |
| Trabalho | worker | `RequestWork` → `SubmitWorkResult` | pull; o worker espera 200 ms quando ocioso |
| Sonda de enlace | worker | `ProbeLink` | upload de 2 MiB, cronometrado, melhor de três |
| Canal de fatias | worker abre | `ShardChannel` | stream bidirecional residente |
| Inferência fatiada | gateway (com token de inferência ou de operador) | `ShardMatmul` | unário; o controller repassa pelo canal |
| Saída do nó | worker | `LeaveNode` | unário; devolve o que estava em voo |
| Enfileirar | operador | `EnqueueWork`, `EnqueueGraph` | unário |
| Cancelar | operador | `CancelRequest` | unário; retira da fila o que não foi despachado |
| Reservar | operador | `Reserve`, `ReleaseReservation` | unário |
| Observação | operador ou CLI | `ListNodes`, `ListEvents`, `GetMetrics`, `GetTrace`, `WorkReport`, `ListReservations` | unário, leitura |

## Como o protocolo cobre o ciclo de vida

| Conceito | Como é feito |
|---|---|
| Registro e credencial | `RegisterNode`; a credencial do nó volta na resposta |
| Capacidade | `RegisterNodeRequest.capability` no registro e `HealthStreamRequest.capability` na remedição. O benchmark roda no próprio worker, no registro; não há RPC para pedi-lo |
| Batida e saúde | `Heartbeat` e `ReportHealth` |
| Modelo | um subdiretório de `AI_CLUSTER_MODEL_DIR`, nomeado em cada `ShardOp`. Não há mensagem de carregar ou descarregar |
| Fatia residente | carregada no primeiro uso e despejada por LRU ([[canal-de-fatias]]) |
| Atribuição de trabalho | o worker puxa com `RequestWork` |
| Cancelamento | `CancelRequest` retira unidades da fila; uma chamada de fatia em voo é abortada quando o gateway desiste |
| Réplica de estado (checkpoint) | `StateReplica` viaja dentro de `RegisterNode` e de `Heartbeat` |
| Saída | `LeaveNode`: o nó vira `DRAINING`, devolve o que tinha em voo e sai |
| Atualização | `Release` vem na resposta de `Heartbeat`; o nó sai ordenadamente antes de atualizar |
| Telemetria | por pull: `ListEvents`, `GetMetrics` |

## Identidade e autenticação

| Segredo | Variável | Quem apresenta | Vale para |
|---|---|---|---|
| Token de join | `AI_CLUSTER_JOIN_TOKEN` | todo nó e todo cliente | registro, leitura e os RPCs de nó |
| Token de operador | `AI_CLUSTER_OPERATOR_TOKEN` | CLI do operador | o que o join não cobre: aprovar, revogar, liberar release, enfileirar, cancelar, reservar, `GetOutput` |
| Token de inferência | `AI_CLUSTER_INFERENCE_TOKEN` | gateway | **só** `ShardMatmul` ([[canal-de-fatias]]) |
| Credencial do nó | emitida em `RegisterNode` | o próprio nó, no cabeçalho `x-ai-cluster-node-auth` | chamadas de nó: `Heartbeat`, `RequestWork`, `SubmitWorkResult`, `ShardChannel` etc. |

- Os três tokens de servidor precisam ser diferentes, ou o controller não sobe.
- Chamadas de nó aceitam a credencial do nó ou o token de operador. O canal de fatias **não** aceita o operador: exige a credencial do nó.
- Um nó precisa estar registrado, aprovado e em estado elegível. Revogação e saída derrubam a sessão.
- `AI_CLUSTER_AUTO_APPROVE=1` aprova sem operador todo nó que se registra. Sem essa variável, cada nó novo espera `ApproveNode`.
- **Atestação.** O nó informa `image_sha256` no registro, a medida do slot que roda. `NodeSnapshot.attested` compara essa medida com a release desejada. É prova de software apenas: um nó comprometido pode informar qualquer valor ([[seguranca]]).
- **Quarentena.** Três resultados inválidos seguidos põem o nó em quarentena. Ele continua vivo e visível em `ListNodes`, mas deixa de ser candidato a trabalho. Aprovar o nó de novo a libera.

Detalhes de quem chama o quê estão em [[mensagens]], na coluna de autorização.

## Metadados de requisição

Um interceptor cliente põe os metadados em toda chamada.

| Campo | Cabeçalho | Conteúdo |
|---|---|---|
| versão do protocolo | `x-ai-cluster-protocol-version` | `v1`. O controller rejeita qualquer outra com `FailedPrecondition` |
| correlação | `x-ai-cluster-correlation-id` | valor novo a cada chamada |
| autenticação | `x-ai-cluster-auth` | token de join, de operador ou de inferência |
| credencial do nó | `x-ai-cluster-node-auth` | credencial emitida no registro |
| nó do canal | `x-ai-cluster-node-id` | só na abertura de `ShardChannel` |
| nó culpado | `x-ai-cluster-failed-node` | só em resposta de erro de `ShardMatmul` |

Idempotência não tem chave explícita. A garantia é estrutural: a unidade sai da tabela de despacho quando completa, então uma submissão duplicada não tem o que completar.

## Streaming e backpressure

- **Canal de fatias**: stream bidirecional com fila de 64 quadros por sentido. Fila cheia falha na hora, sem esperar.
- **Telemetria**: sem stream. O operador lê por pull.
- **Transferências longas**: uma `WorkUnit` vai inteira numa mensagem, limitada a 64 MiB. Não há divisão nem streaming de operandos.

## Limites conhecidos

- **Trabalho por pull.** `RequestWork` com espera ociosa de 200 ms põe um piso de dezenas de milissegundos em cada operação dependente. Por isso o canal de fatias empurra os quadros, em vez de puxar.
- **Um segredo de join para todos os nós.** A identidade de cada nó vem da credencial emitida no registro, não de certificado por nó. Sem TPM, a atestação não prova integridade do hardware.
- **Rejeição por chamada.** Uma versão incompatível não registra o nó, mas o controller não põe nó em quarentena por versão.
- **Sem negociação de features.** O controller não sabe o que cada nó suporta antes de mandar uma operação; a compatibilidade depende de a versão do protocolo bater.
- **Sem chunking de operandos** acima de 64 MiB.

> Planejado: negociação de features no registro; quarentena por versão e por saúde, com mensagem própria visível ao operador.
