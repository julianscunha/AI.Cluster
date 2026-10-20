---
title: "Persistência de estado do controller"
type: implementation
status: implemented
tags: [implementation, persistence, storage, state]
---

# Persistência de estado do controller

O controller mantinha a fila em memória. Um restart — crash, redeploy, container morto —
perdia tudo que estava na fila e toda unidade que não havia completado. Esta é a primeira
peça da fase de storage do plano: a fila de trabalho e seus outputs sobrevivem a um
restart. Ver `work.rs:68`, onde a fatia anterior registrou a lacuna.

## O que mudou

- `WorkSnapshot` e `WorkOutput` no proto: a forma em disco do store.
- `WorkStore::with_state(path)`: abre um store apoiado no snapshot em `path`, carregando-o
  quando existe. Arquivo ausente é primeira execução, não erro.
- `WorkStore::save_locked`: grava o store inteiro depois de cada mutação que muda o que
  está na fila ou concluído.
- `AI_CLUSTER_STATE` nomeia o arquivo de snapshot. Sem ela, é só memória — que é o que os
  benchmarks e os testes usam.

Sem dependência nova: `prost` já está na árvore via `ai-cluster-protocol`, e o snapshot
reusa os tipos gerados em vez de criar um segundo caminho de serialização.

## Semântica

O snapshot guarda o que **ainda não foi feito**: a fila, mais o que estava em voo, mais os
outputs do que completou. No reload, unidades em voo voltam para a fila. O worker que
segurava uma pode ainda estar rodando, mas o controller recarregado não tem registro de
dispatch dela, então esse resultado seria recusado. Reoferecer a unidade é o que impede o
trabalho de sumir. Uma unidade pode, portanto, rodar duas vezes; para uma multiplicação de
matrizes determinística, é a mesma resposta.

Deliberadamente **não** salva no `claim`. Uma unidade em voo quando o controller morre
ainda está no `pending` do snapshot anterior, então volta sem uma escrita por poll. Salvar
a cada claim poria o estado inteiro em disco na frequência do poll — com o poll ocioso de
200ms, uma escrita por worker por poll — sem segurança adicional.

Escrito primeiro num arquivo irmão, depois renomeado para o lugar. Um crash no meio da
escrita deixa o snapshot anterior intacto em vez de um truncado. Uma escrita que falha é
reportada e nunca fatal: o estado em memória continua autoritativo para a execução. Um
snapshot ilegível é tratado como vazio e reportado, pelo mesmo raciocínio — um arquivo
corrompido não pode impedir o cluster de aceitar trabalho.

## Evidência

Um DAG diamante enfileirado sem nenhum worker rodando, o controller morto, depois um
controller novo sobre o mesmo arquivo:

```
estado carregado de /t/state.bin: 4 unidade(s) pendente(s)
controller escutando em 0.0.0.0:50051 (graça de heartbeat 15000ms)
```

Um worker então entrou e o grafo drenou sem ser re-enfileirado:

```
EVENTOS 5
1792467794202  node_registered  node-1
1792467794204  work_completed   w-0 em node-1 compute=94us transfer=1906us
1792467794207  work_completed   w-1 em node-1 compute=88us transfer=912us
1792467794210  work_completed   w-2 em node-1 compute=88us transfer=912us
1792467794212  work_completed   w-3 em node-1 compute=88us transfer=0us
```

Quatro unidades, de uma fila que o primeiro controller nunca chegou a despachar. O teste
de regressão é `a_snapshot_round_trips_pending_work_and_outputs` em `work.rs`: ele afirma
que uma unidade dependente fica *pronta* após o reload, o que só vale se o output da sua
dependência voltou junto, e que a sequência de ids continuou em vez de reiniciar.

## Teto conhecido

**O estado inteiro é reescrito a cada mutação.** O custo cresce com o tamanho da fila, e
cada conclusão reescreve os outputs de todas elas. Ok na escala de benchmark; migrar para
um log append-only ou um snapshot periódico quando a fila crescer ou as mutações ficarem
frequentes. Marcado com um comentário `ponytail:` na escrita.

**O registry não é persistido.** Nós se recuperam sozinhos — um worker cujo heartbeat não
é reconhecido se re-registra — mas entre um restart e o primeiro heartbeat, `ai-cluster
nodes` não mostra nada. Essa janela é o intervalo de grace.

**Eventos não são persistidos.** O log de eventos começa vazio no reload. A fila de
trabalho era o que precisava sobreviver; a linha do tempo é uma visão de incidente
recente por design.

**Não é a fase de storage inteira.** Isto é a fila e seus outputs. Checkpoint de uma
unidade no meio do compute, réplicas e tiers HOT/WARM/COLD são peças separadas da mesma
fase.
