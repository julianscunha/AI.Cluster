---
title: "Telemetria — eventos de ciclo de vida"
type: implementation
status: implemented
tags: [implementation, telemetry, observability, events]
---

# Telemetria — eventos de ciclo de vida

O controller já imprimia o que acontecia. Agora também registra, para que a linha do tempo
seja legível por um cliente e não só por quem está acompanhando o log do container. Isto é
o H2 da fase de Observability do plano, e fecha o `ai-cluster events`, que até então
imprimia "reserved for the telemetry phase".

## O que mudou

- `Event`, `ListEventsRequest`, `ListEventsResponse` e `ListEvents` no proto.
- `EventLog` (`code/crates/controller/src/events.rs`): ring buffer limitado dos últimos
  256 eventos, mais antigo primeiro.
- O serviço registra nos pontos de ciclo de vida: `node_registered`, `node_left`,
  `node_offline`, `work_enqueued`, `graph_enqueued`, `work_completed`.
- O sweep de offline no `main` do controller registra `node_offline` também: é a única
  transição de ciclo de vida que acontece sem uma requisição para pendurar.
- `ai-cluster events [--limit N]` imprime mais antigo primeiro. Padrão são os últimos 20;
  `--limit 0` significa tudo que ainda está guardado.

## Semântica

Todo evento carrega o tempo observado pelo controller. Relógios de nó nunca são usados para
ordenação — a mesma regra da liveness: um nó pode reportar a hora que quiser, então o único
relógio confiável é o do controller.

O log é um **ring buffer, não um store**. Passados 256 eventos o mais antigo é descartado.
Responde "o que acabou de acontecer", que é o que um operador pergunta durante um
incidente; não responde "o que aconteceu semana passada". Ver o teto.

stdout e o log de eventos guardam os mesmos fatos, e não são redundantes: as linhas de
print são o que os scripts de E2E e o `docker logs` leem, os eventos são o que um cliente
pode consultar. Trocar os prints quebraria os testes de falha existentes, que fazem parse
do stdout do controller.

## Evidência

Controller mais dois workers, duas unidades, depois a saída graciosa de um worker:

```
EVENTOS 6
EM_UNIX_MS     TIPO             DETALHE
1792464749316  node_registered  node-1
1792464756107  node_registered  node-2
1792464769235  work_enqueued    2 unidades 64x64x64
1792464769317  work_completed   w-0 em node-1 compute=99us transfer=1901us
1792464769318  work_completed   w-1 em node-1 compute=89us transfer=911us
1792464774608  node_left        node-2 reenfileiradas=0 falharam=0
```

Seis eventos, mais antigo primeiro, uma linha por coisa que aconteceu. Note que as duas
unidades caíram no mesmo nó — isso é o teto abaixo, não um erro de digitação.

O ring buffer é coberto por dois testes unitários em `events.rs` — o mais antigo é
descartado quando enche, e `recent` devolve os N mais novos com o mais antigo primeiro — e
a ligação por `lifecycle_events_are_recorded_and_listed` em `service.rs`, que verifica que
um registro aparece via RPC e que `--limit` corta pela frente.

## Teto conhecido

**Um worker pegou as duas unidades.** Um worker agora pergunta de novo assim que termina
uma unidade, então um worker rápido pode esvaziar a fila antes de um mais lento perguntar.
Este é o teto de placement aparecendo na prática: o dispatch é primeiro-que-pergunta, e
ainda não há cost model para balancear entre nós. Consertar isso pertence ao scheduler, não
à telemetria. Um cooldown curto por worker esconderia o sintoma — ao custo do throughput
que a correção da cadência acabou de comprar, então é a alavanca errada.

**Sem métricas, sem traces, sem prediction error.** Isto é só o H2. Nada agrega (H1), nada
liga uma requisição de cliente às unidades que ela produziu (H3), e nada expõe
previsto-vs-real (H5). Estes eventos são uma linha do tempo, não um dashboard.

**Em memória, perdido no restart.** Diferente da fila de trabalho, que agora persiste
(`persistence.md`). O log de eventos é uma visão de incidente recente por design — o que
precisava sobreviver era a fila. Persistir eventos é uma decisão separada, e a cadeia do
plano põe storage depois de telemetria.
