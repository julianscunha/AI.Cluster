---
title: "Camada de transporte"
aliases: ["Camada de transporte", "Transport Layer"]
tags: [sistema/subsistema/substrato]
type: sistema/subsistema
---

# Camada de transporte

Leva Work Units, fatias e estado entre o controller e os workers. Hoje o único meio em uso é gRPC sobre HTTP/2, com TCP. O protocolo de controle (quem faz o quê) e o transporte de dados (os bytes) hoje passam pelo mesmo canal.

## Conexão

- gRPC sobre HTTP/2, com `tonic`. Keepalive HTTP/2 de 10 s no controller.
- TLS opcional. Com `https://`, o worker verifica o controller contra `AI_CLUSTER_TLS_CA` (e `AI_CLUSTER_TLS_DOMAIN`). O controller sobe com `AI_CLUSTER_TLS_CERT` e `AI_CLUSTER_TLS_KEY`, ou recusa iniciar sem TLS, salvo liberação explícita para rede fechada. Ver [[protocolo-controller-worker]].
- Teto de mensagem de 64 MiB em ambos os sentidos. Uma unidade ou fatia maior que isso não cabe. Ver [[work-unit]].
- A sonda de link tem teto de 4 MiB no servidor, por ser um serviço separado.

## Sonda de link

O serviço `LinkProbe` é separado do serviço de controle. Usos:

- **Vazão de upload.** O worker envia 3 cargas de 2 MiB ao controller e fica com a melhor, com timeout de 5 s. O resultado, normalizado por 1 GbE, vira `network_achieved_score`.
- **Latência.** O worker mede o menor tempo entre 5 sondas vazias. O resultado vai em `network_latency_ms`.

Ambas são remedidas com o nó ocioso, na mesma cadência do compute. Uma sonda que falha mantém o valor anterior; zero significa "não medido".

O placement usa o menor entre a velocidade nominal do link e a velocidade alcançada.

## Canal residente de fatias

`ShardChannel` é um stream bidirecional entre controller e worker, usado pela inferência:

- O worker abre o stream com o `node_id` no cabeçalho. O controller mantém uma sessão por nó.
- A fila de resposta tem 64 quadros e no máximo 64 operações pendentes por nó. O controller aceita resposta só para operação enviada àquele nó.
- Detalhes em [[canal-de-fatias]] e [[adr-009-canal-residente-de-fatias]].

## Decisões antes de transferir

Hoje o sistema não decide entre transferir e recomputar. Toda unidade vai ao nó escolhido pelo placement, e os operandos sempre viajam pelo controller.

## Plano de dados

Os tensores viajam como `repeated double` dentro das mensagens protobuf. Não há buffer binário nem zero-copy. Cada elemento custa 8 bytes na rede, mais o custo de codificação.

## Transporte em memória

Existe a abstração `Transport` no crate `transport`, com envio, recebimento e sonda, e dois meios: TCP e socket de domínio Unix. Ela foi medida, mas o cluster ainda não passa por ela. O tráfego real é todo gRPC. O enum `TransportKind` declara `SharedMemory` e `Usb`, sem implementação. Ver [[adr-014-abstracao-de-transporte]].

## Limites conhecidos

- O cluster só fala gRPC sobre TCP, mesmo entre processos do mesmo host.
- Os operandos cruzam o controller duas vezes em um grafo de dependências: ida do worker de origem e volta para o próximo worker.
- A medição de rede é de upload até o controller. Não há perna de download, nem medida entre dois workers.
- Perda de pacotes e congestionamento não são medidos. Sobre TCP, a perda aparece como atraso.
- Worker e controller no mesmo host medem o loopback. Nesse caso, o teto nominal da sonda é o que vale.

> Planejado: memória compartilhada entre processos do mesmo host, transporte USB para workers móveis, RDMA, plano de dados binário, compressão, chunking com retomada e checksum por chunk, e decisões de transferência por custo (tensor local, cache, recomputação, réplica mais barata, caminho).
