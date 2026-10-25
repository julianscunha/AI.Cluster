---
title: "Camada de transporte"
aliases: ["Camada de transporte", "Transport Layer", "docs/12", "12 — Transport Layer", "12 — Camada de transporte"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
status: proposto
implementacao: parcial
fases: ["[[fase-09-transporte]]", "[[fase-08-inferencia]]"]
---

# Camada de transporte

Transporta Work Units, tensores, estado e artefatos pelo meio mais adequado para cada par de nós. O protocolo de controle (quem faz o quê) e o transporte de dados (os bytes) são tratados separadamente: o primeiro é pequeno e autenticado; o segundo é otimizado para buffers e tensores.

## Abstração de transporte

```text
Transport Manager
 |- shared memory
 |- Ethernet/TCP
 |- RDMA (future)
 |- USB
 `- future transports
```

O Transport Manager escolhe o meio por par de nós, com as mesmas métricas de link usadas no placement. Quem chama não sabe qual meio foi usado.

## Plano de controle vs plano de dados

- **Plano de controle**: mensagens pequenas, versionadas e autenticadas ([[protocolo-controller-worker]], [[versionamento]]).
- **Plano de dados**: buffers e tensores, sem serialização por elemento. Recebe o canal mais largo disponível.

## Métricas de link

- banda;
- latência;
- jitter;
- perda de pacotes;
- retransmissões;
- utilização atual;
- saúde.

## Decisões antes de transferir

Antes de cada transferência, nesta ordem:

1. o tensor já está local?
2. há cache hit?
3. recomputar sai mais barato?
4. comprimir ou quantizar compensa?
5. qual réplica é a melhor fonte?
6. qual caminho é o melhor?

## Chunking

Objetos grandes são divididos em chunks, com retry, resume e checksum por chunk.

## Compressão

Aplicada só quando o ganho de banda supera o custo de comprimir e descomprimir.

## USB

USB é transporte de primeira classe para workers móveis ([[fase-18-worker-movel]]), desde que o caminho efetivo ofereça desempenho e estabilidade suficientes.

> [!info] O que já existe
> - **gRPC sobre HTTP/2 com `tonic`**: único transporte em uso. Cliente e servidor em `code/crates/protocol/src/metadata.rs` (`open_channel`, `connect_node`, `connect_probe`); o controller (`code/crates/controller/src/main.rs`) define keepalive HTTP/2 de 10 s.
> - **TLS opcional**: `https://` verifica o controller com `AI_CLUSTER_TLS_CA` (e `AI_CLUSTER_TLS_DOMAIN`); o controller sobe com `AI_CLUSTER_TLS_CERT` e `AI_CLUSTER_TLS_KEY`, ou recusa iniciar sem TLS, salvo a liberação explícita para rede fechada ([[f07-tls]]).
> - **Tamanho de mensagem**: 64 MiB em ambos os sentidos (`MAX_MESSAGE_BYTES`). Uma unidade ou fatia maior não cabe.
> - **Sonda de link (`ProbeLink`)**: serviço `LinkProbe` separado do `ClusterControl`. O worker envia 2 MiB, 3 vezes, e fica com a melhor taxa, com timeout de 5 s. Resultado normalizado por 1 GbE vira `network_achieved_score` no registro; é remedido com o nó ocioso (`current_network_score`) ([[f02-descoberta-de-rede]]).
> - **Canal residente de fatias**: `rpc ShardChannel(stream ShardResult) returns (stream ShardCompute)` em `code/proto/cluster.proto`. O worker abre o stream com o `node_id` no cabeçalho; o controller mantém uma sessão por nó, com fila de 64 frames e 64 operações pendentes, e aceita resposta só para operação enviada àquele nó (`code/crates/controller/src/shards.rs`) ([[f08-canal-residente-de-fatias]], [[adr-009-canal-residente-de-fatias]]).
> - **Plano de dados atual**: tensores viajam como `repeated double` dentro de mensagens protobuf (`WorkUnit.matrix_a`, `ShardCompute.x`). Não há buffer binário nem zero-copy.
> - **Abstração de transporte**: `code/crates/transport/src/lib.rs` tem o enum `TransportKind` (`Tcp`, `SharedMemory`, `Usb`) e o trait `Transport` com um único método `kind()`. Nenhum crate depende dele e nada implementa o trait.
>
> Fases: [[fase-09-transporte]] (abstração e shared memory, pendente), [[fase-08-inferencia]] (canal residente).

> [!todo] Abstração send/receive/stream/probe implementada de fato; transporte por memória compartilhada; USB ([[fase-18-worker-movel]]); RDMA; plano de dados binário; chunking, resume e checksum de objetos grandes; compressão; métricas de link além de banda (latência de rede, jitter, perda, retransmissões, utilização); decisões de transferência (as seis perguntas acima).

> [!warning] Divergência: o alvo separa plano de controle e plano de dados. Hoje os dois passam pelo mesmo gRPC, e os tensores são `double` por elemento, o que custa tamanho e CPU. O limite de 64 MiB por mensagem é o teto real de uma unidade. A migração para buffers binários não está planejada em nenhum slice; precisa entrar em [[fase-09-transporte]] antes do modelo distribuído.
