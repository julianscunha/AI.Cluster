---
title: "ADR-013 — Estado do controller e recuperação"
aliases: ["ADR-013", "Estado e recuperação"]
tags: [sistema/adr, subsistema/dados]
type: sistema/adr
---

# ADR-013 — Estado do controller e recuperação

Status: decidido · Data: 2026-10-25 · Voltar para [[decisoes]].

## Contexto

O controller guardava a fila e as unidades em voo só na memória. Um restart perdia trabalho, e um restart com uma unidade em voo deixava a dúvida: o dono ainda a segura, ou morreu junto? Reoferecer cedo demais roda a unidade duas vezes; esperar para sempre trava o grafo.

## Decisão

1. **Snapshot do conjunto vivo, não journal.** O `WorkStore` grava a fila, a posse das unidades em voo e os resultados que alguma unidade ainda vai ler, num arquivo (`AI_CLUSTER_STATE`), por escrita atômica (arquivo irmão e rename). O conjunto vivo é pequeno por construção.
2. **Checkpoint podado.** Um resultado só fica guardado enquanto alguma unidade não terminada ainda o lê. É isso que mantém o arquivo pequeno.
3. **Posse preguiçosa.** Cada unidade em voo guarda o dono e o payload, então um dono que não volta tem o que reoferecer. O snapshot não decide por conta própria que o dono morreu.
4. **Dois relógios.** Heartbeat perdido com o controller de pé (`mark_stale_offline`) e dono ausente depois do restart (`unknown_owners`), este contado desde o start do controller, não desde o despacho. Os dois caminhos acabam em `requeue_for_node`, que respeita o orçamento de tentativas.
5. **Cópia num nó.** Um segundo arquivo irmão no volume e, opcionalmente, uma cópia que o controller entrega a um worker (`AI_CLUSTER_STATE_REPLICA`) só quando a época do controller avança além da do nó. A cópia só é adotada quando o controller não tem trabalho nas mãos e ela é mais nova.
6. **Época dentro do snapshot.** `epoch_unix_ms` sobrevive ao restart e avança por `max(relógio, anterior + 1)`, para um relógio que anda para trás não fazer o estado vivo parecer mais velho que uma cópia no campo.
7. **Eventos em arquivo de linhas.** A linha do tempo (`AI_CLUSTER_EVENTS`) é gravada em texto, uma linha por evento; ao subir, o controller recarrega os últimos 256. O arquivo é reescrito quando passa de 1024 linhas. Perder o arquivo custa histórico, não o controller.
8. **Registry não persiste o estado vivo.** Nós reaparecem pelo heartbeat (`acknowledged=false` provoca novo registro). Só a identidade de confiança (credencial, aprovação, quarentena) vai a disco ([[adr-010-papeis-de-credencial]]).

## Consequências

- Depois de um restart, uma unidade com dono espera até o grace antes de ser reoferecida: de propósito, para o dono que está subindo não ver o trabalho rodar duas vezes.
- Eventos de antes do restart não incluem o que o ring buffer já havia descartado.

## Alternativas

- Journal de deltas: escala melhor, mas exige compactação e replay; adiado até o conjunto vivo crescer.
- Banco embutido: mais uma dependência e um formato para um arquivo que cabe em memória.
- Persistir o registry inteiro: os nós o reconstroem em segundos pelo heartbeat, e um registry velho no disco faria o controller acreditar em nós que já saíram.
- Reoferecer a unidade no restart sem esperar o dono: roda trabalho em dobro sempre que o controller reinicia.

## Limites

- Reescrever o snapshot inteiro a cada mudança custa O(conjunto vivo). Com milhares de unidades vivas isso exige um journal.
- A cópia do estado fora do volume do controller é opcional (`AI_CLUSTER_STATE_REPLICA`).
- O estado por chave (KV cache, estado de modelo) não é coberto; só fila, posse e resultados de Work Units.
