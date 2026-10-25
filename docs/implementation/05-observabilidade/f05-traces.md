---
title: "F05 — Traces: correlação de requisição"
aliases: ["traces", "H3"]
tags: [implementacao/fase/05, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-05-observabilidade]]"
codigos-antigos: ["H3"]
sistema: ["[[observabilidade]]"]
evidencia: ["code/tests/phase-h-traces.sh"]
---
# F05 — Traces: correlação de requisição

- [ ] Superado

## Objetivo

As métricas do H1 dizem como o cluster está, não **o que aconteceu com a minha requisição**. Um cliente que enfileira dez unidades e vê a décima demorar não consegue, pelas métricas do cluster, saber qual foi a lenta nem onde rodou. Ligar a requisição do cliente às unidades que ela gerou, e cada unidade ao nó e ao tempo que consumiu.

## O que foi feito

- `request_id` no `WorkUnit` e nos pedidos de enfileiramento: o controlador cunha um id quando o cliente não manda, e o devolve em `EnqueueWorkResponse` e `EnqueueGraphResponse`.
- `TraceEntry` e `GetTrace` no proto: uma chamada devolve as unidades de uma requisição, com estado, nó e tempos.
- `WorkStore::trace` (`code/crates/controller/src/work.rs`): varre fila, em voo, concluídas e falhas, e devolve as linhas da requisição. O id viaja junto da unidade desde o enfileiramento, então a correlação não depende de tabela extra.
- CLI: `ai-cluster request trace <id>`.

## Como ler

```text
REQUISIÇÃO abc123 UNIDADES 2
TRABALHO   NÓ             ESTADO      COMPUTE_US TRANSFER_US   TOTAL_US
w1         node-t         completed       12345         678      13023
w2         node-t         completed       11987         654      12641
unidades=2 compute_somado_us=24332 transfer_somado_us=1332 latencia_somada_us=25664
```

`estado` é onde a unidade está na vida dela:

- `queued`: na fila, ainda não despachada (inclui as que esperam dependência do grafo).
- `dispatched`: em voo num nó, sem resultado.
- `completed`: terminou; é a única com tempos.
- `failed`: orçamento de retry esgotado; nunca vai completar.

O que explica a latência é a divisão por unidade. Em `w1`, dos 13.023us de ponta a ponta, 12.345us foram compute e 678us transferência: unidade dominada por compute. Uma unidade com o inverso denuncia que o gargalo é a rede.

## Decisões

- **Id opcional na entrada.** Um cliente que quer rastrear manda o id e usa o mesmo em `request trace`. Um cliente que não se importa manda vazio e recebe um id cunhado pelo controlador. O id não é gerado no cliente porque o controlador sabe quais unidades uma requisição produziu; pedir ao cliente para inventar um id só criaria colisões.

## Evidência

`bash code/tests/phase-h-traces.sh`.

```text
=== A) enfileirar 2 unidades ===
enfileiradas 2 unidades de trabalho de 64x64 requisicao=...
requisicao capturada: ...
=== B) trace da requisição ===
REQUISIÇÃO ... UNIDADES 2
TRABALHO   NÓ             ESTADO      COMPUTE_US TRANSFER_US   TOTAL_US
w1         node-t         completed       ...
w2         node-t         completed       ...
OK: as 2 unidades da requisição aparecem como concluídas
OK: o compute somado do trace é maior que zero (...) us
=== C) id desconhecido ===
REQUISIÇÃO nao-existe UNIDADES 0
(nenhuma unidade: id desconhecido ou trace perdido por restart)
OK: id desconhecido devolve trace vazio, sem erro
```

Cobertura: `a_trace_follows_a_unit_from_queue_to_completion`, `a_trace_lists_every_unit_a_request_produced`, `a_trace_shows_a_failed_unit`, `a_trace_keeps_other_requests_out` e `a_trace_of_an_unknown_request_is_empty` em `work.rs`; `get_trace_follows_a_requests_units_through_the_service` no serviço.

## Limites

> [!warning] Só a execução atual
> Unidades concluídas não são persistidas, e o id de requisição de uma unidade falha se perde no reload. O trace cobre o que o controlador ainda lembra: o suficiente para explicar uma requisição em voo ou recém-terminada. Persistir a correlação é outra fatia, junto da persistência do work store ([[fase-06-estado-e-recuperacao]]).

> [!warning] Sem span por fase
> O trace mostra custo por unidade, não o tempo em cada passo dentro do worker (leitura, multiplicação, escrita). Para isso o worker teria que reportar spans, mudança de protocolo maior.

## Próximos passos

- Persistir a correlação de requisição junto do work store.
- Spans por fase dentro do worker, se o protocolo evoluir.
