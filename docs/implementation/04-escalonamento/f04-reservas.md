---
title: "F04 — Reservas"
aliases: ["reservations", "G6"]
tags: [implementacao/fase/04, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-04-escalonamento]]"
codigos-antigos: ["G6", "roadmap-2"]
sistema: ["[[scheduler]]"]
evidencia: ["code/tests/phase-g-reservations.sh"]
---
# F04 — Reservas

- [ ] Superado

## Objetivo

O scheduler escolhe o nó mais barato para cada unidade. Isso é certo para trabalho anônimo e errado para trabalho que ainda não existe: um cliente que vai enfileirar um pedido logo mais não tem como dizer "não deixe ninguém tomar este nó", e quando o pedido chega o nó já está ocupado ou marcado como caro. Esta fatia (G6) retém um nó **antes** de o trabalho existir.

## O que foi feito

- `Reservation`, `ReserveRequest`/`ReserveResponse`, `ListReservationsRequest`/`ListReservationsResponse` e `ReleaseReservationRequest`/`ReleaseReservationResponse` em `code/proto/cluster.proto`, mais os RPCs `Reserve`, `ListReservations` e `ReleaseReservation`. Liberar é por nó: um nó tem no máximo uma reserva.
- `code/crates/controller/src/reservations.rs`: o store (`reserve`, `release`, `held_for`, `list`). Reserva vencida é descartada na leitura, sem timer de fundo.
- `WorkStore` guarda as reservas e a decisão de placement as consulta nos dois caminhos de despacho (`claim_placed` e `claim`): um nó retido só aceita unidades da requisição dona, e a unidade de uma requisição com reserva só roda no nó retido.
- CLI: `ai-cluster reserve <nó> <requisição> [--ttl-ms N]`, `ai-cluster reservations` e `ai-cluster release <nó>`.
- `ai-cluster benchmark --request-id <id>`: permite enfileirar sob o id reservado. Sem isso a reserva nunca casaria com unidade nenhuma.

## Como ler

Uma reserva é um par (nó, requisição dona) com vencimento. Uso pretendido:

```bash
ai-cluster reserve node-data req-42 --ttl-ms 60000
ai-cluster benchmark --size 256 --request-id req-42
```

Enquanto a reserva vale, `node-data` é da `req-42`:

- unidades de **outras** requisições não são colocadas nele;
- unidades da `req-42` rodam nele, **mesmo que ele seja o pior pelo cost model**: a reserva é a promessa de que o trabalho cai ali, e um ranking de custo não pode desfazê-la.

Uma reserva nova sobre o mesmo nó substitui a anterior. A reserva retém o **nó**, não uma quantidade de cores dele.

## Por que a reserva vence o custo

O placement normal entrega a unidade ao nó mais barato, ou a um nó dentro da banda de histerese ([[f04-placement]], G5). Uma reserva precisa passar por cima disso; senão reservar não muda nada. Quando a requisição da unidade tem nó retido, a unidade é dele e os outros nós cedem. O custo não entra na decisão.

A recusa vale para os dois lados. Um nó retido recusa unidades de outras requisições mesmo no caminho sem placement (`AI_CLUSTER_PLACEMENT=off`): a retenção é propriedade do nó, não da política, e desligar o placement para medir baseline não pode mexer numa promessa feita ao cliente.

## Evidência

`bash code/tests/phase-g-reservations.sh`, com um nó fraco (2 threads) e um forte (8 threads). Sem reserva, o cost model sempre prefere o forte; é essa preferência que dá sentido às pernas:

- A) baseline: sem reserva, a unidade roda em `node-strong`;
- B) dono recebe o nó: `node-weak` retido para `req-held`, e a unidade de `req-held` roda no `node-weak` (o pior), porque a reserva vence o custo;
- C) recusa: `node-strong` retido para `req-locked`, e a unidade de `req-other` cai no `node-weak`, mesmo com `node-strong` ocioso;
- D) liberar devolve `node-strong` ao ranking, e a unidade seguinte volta para ele;
- E) nenhuma reserva ativa ao final.

Testes unitários: `reservations::tests::*` (retenção, vencimento, substituição, liberação) e, em `work.rs`, `a_reserved_node_takes_its_requests_unit_instead_of_a_cheaper_stranger`, `a_node_reserved_for_another_request_takes_no_work`, `an_expired_reservation_stops_holding_the_node`, `an_ungated_claim_honours_a_reservation_too`, `releasing_a_reservation_frees_the_node_for_other_requests` e `reservations_lists_active_holds_and_drops_expired_ones`.

## Limites

> [!warning] Nó inteiro, não fração
> Não existe "reservei 4 dos 8 cores". Reservar parte da capacidade faz sentido junto de um modelo de capacidade por nó, que o placement ainda não tem. Hoje ele enxerga ocioso ou com uma unidade em voo.

> [!warning] Não sobrevive a restart
> As reservas vivem em memória, como as unidades concluídas. Persistir junto do snapshot do work store é a mesma fatia da persistência dos traces.

> [!warning] Uma requisição por reserva, sem fila
> Vários nós podem ser retidos para a mesma requisição. Unidades que não cabem nos nós retidos esperam até o TTL vencer, porque um nó retido para outra requisição não as aceita. Não há fila de espera por reserva nem reorganização das unidades entre os nós retidos.

> [!warning] Vencimento é lido, não disparado
> O TTL é aplicado quando alguém pergunta pela reserva (`held_for`/`list`). Um cliente que reserva e some libera o nó no primeiro poll após o vencimento, não no instante exato.

## Próximos passos

- Reserva de capacidade parcial, junto de um modelo de capacidade por nó.
- Persistir reservas com o snapshot do work store ([[fase-06-estado-e-recuperacao]]).
