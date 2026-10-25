---
title: "F05 — Telemetria: eventos de ciclo de vida"
aliases: ["telemetry", "H2"]
tags: [implementacao/fase/05, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-05-observabilidade]]"
codigos-antigos: ["H2"]
sistema: ["[[observabilidade]]"]
evidencia: ["code/crates/controller/src/events.rs"]
---
# F05 — Telemetria: eventos de ciclo de vida

- [ ] Superado

## Objetivo

O controller já imprimia o que acontecia. Agora também registra, para que a linha do tempo seja legível por um cliente, e não só por quem acompanha o log do container. Fecha `ai-cluster events`, que antes imprimia "reserved for the telemetry phase".

## O que foi feito

- `Event`, `ListEventsRequest`, `ListEventsResponse` e `ListEvents` no proto.
- `EventLog` (`code/crates/controller/src/events.rs`): ring buffer dos últimos 256 eventos, mais antigo primeiro.
- O serviço registra nos pontos de ciclo de vida: `node_registered`, `node_left`, `node_offline`, `work_enqueued`, `graph_enqueued`, `work_completed`.
- O sweep de offline no `main` do controller registra `node_offline` também: é a única transição de ciclo de vida que acontece sem uma requisição para pendurar.
- `ai-cluster events [--limit N]` imprime mais antigo primeiro. Padrão são os últimos 20; `--limit 0` significa tudo que ainda está guardado.

## Decisões

- **Tempo do controller.** Todo evento carrega o tempo observado pelo controller. Relógios de nó nunca são usados para ordenação: um nó pode reportar a hora que quiser, então o único relógio confiável é o do controller.
- **Ring buffer, não store.** Passados 256 eventos, o mais antigo é descartado. Responde "o que acabou de acontecer", que é o que um operador pergunta durante um incidente; não responde "o que aconteceu semana passada".
- **Stdout e eventos são redundantes de propósito.** As linhas de print são o que os scripts de E2E e o `docker logs` leem; os eventos são o que um cliente consulta. Trocar os prints quebraria os testes de falha, que fazem parse do stdout.

## Evidência

Controller com dois workers, duas unidades, depois a saída graciosa de um worker:

```text
EVENTOS 6
EM_UNIX_MS     TIPO             DETALHE
1792464749316  node_registered  node-1
1792464756107  node_registered  node-2
1792464769235  work_enqueued    2 unidades 64x64x64
1792464769317  work_completed   w-0 em node-1 compute=99us transfer=1901us
1792464769318  work_completed   w-1 em node-1 compute=89us transfer=911us
1792464774608  node_left        node-2 reenfileiradas=0 falharam=0
```

Seis eventos, mais antigo primeiro. As duas unidades caíram no mesmo nó; isso é o teto abaixo, não erro de digitação.

Cobertura: dois testes unitários em `events.rs` (o mais antigo é descartado quando enche; `recent` devolve os N mais novos, mais antigo primeiro) e `lifecycle_events_are_recorded_and_listed` em `service.rs` (registro aparece via RPC e `--limit` corta pela frente).

## Limites

> [!warning] Um worker pegou as duas unidades
> Um worker agora pergunta de novo assim que termina uma unidade, então um worker rápido pode esvaziar a fila antes de um mais lento perguntar. É o teto de placement aparecendo na prática: o dispatch é primeiro-que-pergunta e ainda sem cost model para balancear. Consertar pertence ao scheduler ([[f04-placement]]), não à telemetria. Um cooldown por worker esconderia o sintoma, ao custo do throughput que a correção da cadência comprou.

> [!warning] Só linha do tempo
> Sem métricas, traces ou prediction error neste slice. Nada agrega ([[f05-metricas]]), nada liga requisição às unidades ([[f05-traces]]), nada expõe previsto-vs-real ([[f05-previsto-vs-real]]).

> [!todo] Eventos em memória
> Perdidos no restart, diferente da fila de trabalho, que agora persiste ([[fase-06-estado-e-recuperacao]]). Persistir eventos é decisão separada; a cadeia do plano põe storage depois de telemetria.
