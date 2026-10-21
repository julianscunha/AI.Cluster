---
title: "Reservas — recursos para execução futura"
type: implementation
status: implemented
tags: [implementation, scheduler, reservations]
---

# Reservas — recursos para execução futura

O scheduler escolhe o nó mais barato para cada unidade. Isso é o certo para trabalho
anônimo e o errado para trabalho que ainda não existe: um cliente que vai enfileirar um
pedido logo mais não tem como dizer "não deixe ninguém tomar este nó", e quando o pedido
chega o nó já está ocupado ou foi marcado como caro. Isto é o G6 da fase de Scheduler:
reter um nó **antes** de o trabalho existir — reservar recursos para execução futura.

## O que mudou

- `Reservation`, `ReserveRequest`/`ReserveResponse`, `ListReservationsRequest`/
  `ListReservationsResponse` e `ReleaseReservationRequest`/`ReleaseReservationResponse` no
  `code/proto/cluster.proto`, mais os RPCs `Reserve`, `ListReservations` e
  `ReleaseReservation`. Liberar é por nó: um nó tem no máximo uma reserva, então o nó
  identifica qual soltar.
- `code/crates/controller/src/reservations.rs`: o store das reservas — `reserve`, `release`,
  `held_for` e `list`. Reserva vencida é descartada na leitura, então não há timer de fundo.
- `WorkStore` guarda as reservas (`work.rs`) e a decisão de placement as consulta nos dois
  caminhos de despacho, `claim_placed` e `claim`: um nó retido só aceita unidades da
  requisição dona, e a unidade de uma requisição com reserva só roda no nó retido.
- `ai-cluster reserve <nó> <requisição> [--ttl-ms N]`, `ai-cluster reservations` e
  `ai-cluster release <nó>` na CLI.
- `ai-cluster benchmark --request-id <id>`: sem isso o cliente não consegue enfileirar sob o
  id para o qual reservou, e a reserva nunca casaria com unidade nenhuma.

## Como ler

Uma reserva é um par (nó, requisição dona) com vencimento. O uso pretendido é:

```
ai-cluster reserve node-data req-42 --ttl-ms 60000
ai-cluster benchmark --size 256 --request-id req-42
```

Enquanto a reserva vale, `node-data` é da `req-42`:

- unidades de **outras** requisições não são colocadas nele;
- unidades da `req-42` rodam nele, **mesmo que ele seja o nó pior pelo cost model** — a
  reserva é a promessa de que o trabalho cai ali, e um ranking de custo pode desfazer uma
  promessa.

Uma reserva nova sobre o mesmo nó substitui a anterior: um nó, uma reserva. Não confundir
com o G4: a reserva retém o **nó**, não uma quantidade de cores dele.

## Por que a reserva vence o custo

O placement normal entrega a unidade ao nó mais barato, ou a um nó próximo o bastante do
mais barato para valer o poll (a banda de histerese do G5). Uma reserva precisa passar por
cima disso, senão reservar não muda nada: o nó retido perderia a unidade para o mesmo nó mais
barato que ganharia sem reserva alguma. Por isso, quando a requisição da unidade tem nó
retido, a unidade é dele e os outros nós cedem — o custo não entra na decisão.

A recusa vale para os dois lados. Um nó retido recusa unidades de outras requisições mesmo no
caminho sem placement (`AI_CLUSTER_PLACEMENT=off`): a retenção é propriedade do nó, não da
política de placement, e um desligamento do placement para medir baseline não pode fuçar numa
promessa feita ao cliente.

## Teto conhecido

- **Nó inteiro, não fração.** Não existe "reservei 4 dos 8 cores". Reservar parte da
  capacidade só faz sentido junto de um modelo de capacidade por nó, que o placement ainda
  não tem — hoje ele enxerga ocioso ou com uma unidade em voo. Enquanto isso, quem reserva
  reserva o nó.
- **Não sobrevive a restart.** As reservas vivem em memória, como as unidades concluídas;
  persistir junto do snapshot do work store é a mesma fatia da persistência dos traces.
- **Uma requisição por reserva, e nenhuma fila.** Vários nós podem ser retidos para a mesma
  requisição; unidades da requisição que não couberem no nó retido esperam até o TTL vencer,
  porque um nó retido para outra requisição não as aceita. Não há fila de espera por reserva
  nem reorganização das unidades entre os nós retidos.
- **Vencimento é lido, não disparado.** O TTL é aplicado quando alguém pergunta pela reserva
  (`held_for`/`list`). Um cliente que reserva e some libera o nó no primeiro poll após o
  vencimento, não no instante exato dele.

## Evidência

`bash code/tests/phase-g-reservations.sh`, com um nó fraco (2 threads) e um forte (8 threads)
— sem reserva o cost model sempre prefere o forte, e é essa preferência que dá sentido às
pernas:

- A) baseline: sem reserva, a unidade roda em `node-strong`;
- B) dono recebe o nó: `node-weak` retido para `req-held`, e a unidade de `req-held` roda no
  `node-weak` — o nó **pior** — porque a reserva vence o custo;
- C) recusa: `node-strong` retido para `req-locked`, e a unidade de `req-other` cai no
  `node-weak`, mesmo com o `node-strong` ocioso;
- D) liberar devolve `node-strong` ao ranking, e a unidade seguinte volta para ele;
- E) nenhuma reserva ativa ao final.

Os testes unitários que sustentam as pernas: `reservations::tests::*` (retenção, vencimento,
substituição, liberação) e, em `work.rs`, `a_reserved_node_takes_its_requests_unit_instead_of_a_cheaper_stranger`,
`a_node_reserved_for_another_request_takes_no_work`,
`an_expired_reservation_stops_holding_the_node`, `an_ungated_claim_honours_a_reservation_too`,
`releasing_a_reservation_frees_the_node_for_other_requests` e
`reservations_lists_active_holds_and_drops_expired_ones`.
