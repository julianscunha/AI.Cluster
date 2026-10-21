---
title: "Persistência de estado do controller"
type: implementation
status: implemented
tags: [implementation, persistence, storage, state]
---

# Persistência de estado do controller

O controller mantinha a fila em memória. Um restart — crash, redeploy, container morto —
perdia tudo que estava na fila e toda unidade que não havia completado. Esta é a fatia de
storage do plano que faz a fila e seus outputs sobreviverem a um restart. Dois avanços
posteriores (I1) entraram aqui: o **dono** de uma unidade em voo também sobrevive, e o
arquivo ganhou uma **cópia** ao lado. Ver `work.rs:68`, onde a fatia anterior registrou a
lacuna.

## O que mudou

- `WorkSnapshot`, `WorkOutput` e `WorkLease` no proto: a forma em disco do store.
- `WorkStore::with_state(path)`: abre um store apoiado no snapshot em `path`, carregando-o
  quando existe. Arquivo ausente é primeira execução, não erro.
- `WorkStore::save_locked`: grava o store inteiro depois de cada mutação que muda o que
  está na fila, em voo ou concluído.
- `AI_CLUSTER_STATE` nomeia o arquivo de snapshot. Sem ela, é só memória — que é o que os
  benchmarks e os testes usam.
- (I1) o snapshot carrega `dispatched`: as unidades em voo com o nó que as segura, gravadas
  junto do resto.
- (I1) cada gravação escreve dois arquivos: `path` e `path.replica`.

Sem dependência nova: `prost` já está na árvore via `ai-cluster-protocol`, e o snapshot
reusa os tipos gerados em vez de criar um segundo caminho de serialização.

## Semântica

O snapshot guarda o que **ainda não foi feito**: a fila, o que está em voo com o dono, e os
outputs do que completou.

**Posse.** Uma unidade em voo volta ao reload ainda pertencendo ao mesmo nó, e não para a
fila. O resultado que aquele worker estava calculando é aceito em vez de recusado — sem o
lease, o controller recarregado não tinha registro de dispatch e descartava exatamente o
resultado que já estava sendo computado. Depois de um restart o registry está vazio, então o
dono não é reconhecido de imediato: a varredura espera o grace e, se o nó não voltar nesse
tempo, trata a unidade como as de um nó perdido — reoferece, ou falha se o orçamento de
tentativas acabou (`WorkStore::unknown_owners`).

**Cópia.** O primário é lido primeiro; se estiver ausente, ilegível ou decodificar como
vazio, a cópia ao lado é carregada no lugar. As duas são escritas na mesma gravação, então
podem diferir em idade, nunca em validade. Só quando nenhuma das duas serve o store começa
vazio — e começa, porque um snapshot corrompido não pode impedir o cluster de aceitar
trabalho.

**Zero bytes não é "sem trabalho".** Um arquivo vazio decodifica como snapshot default (todos
os campos no valor zero), então passaria por "nada a fazer" mesmo com a fila sumida. Zero
bytes é tratado como arquivo não carregável, e a cópia tem a vez.

Deliberadamente **não** salva no `claim`. Uma unidade reivindicada e ainda não gravada está no
`pending` do snapshot anterior, então volta sem uma escrita por poll. Salvar a cada claim poria
o estado inteiro em disco na frequência do poll — com o poll ocioso de 200ms, uma escrita por
worker por poll — sem segurança adicional. O custo é que essa unidade perde o dono e pode rodar
duas vezes; para uma multiplicação de matrizes determinística, é a mesma resposta.

Cada arquivo é escrito primeiro num irmão e depois renomeado para o lugar. Um crash no meio da
escrita deixa a gravação anterior intacta em vez de um arquivo truncado. Uma escrita que falha
é reportada e nunca fatal: o estado em memória continua autoritativo para a execução.

## Evidência

Um DAG diamante enfileirado sem nenhum worker rodando, o controller morto por `docker kill`,
o primário zerado, depois um controller novo sobre o mesmo arquivo:

```
estado carregado de /t/state-i1/state.bin.replica: 4 pendente(s), 0 em voo
controller escutando em 0.0.0.0:50051 (graça de heartbeat 15000ms)
```

Um worker então entrou e o grafo drenou sem ser re-enfileirado:

```
nodes_total=1 nodes_available=1
units_completed=4 units_pending=0 units_blocked=0 units_failed=0
total_compute_time_us=3466 avg_compute_time_us=866
```

Quatro unidades, de uma fila que o primeiro controller nunca chegou a despachar. O E2E é
`bash code/tests/phase-i-state.sh` — quatro pernas: grava os dois arquivos, mata o controller
sem shutdown limpo, zera o primário e carrega da cópia, e deixa um worker drenar a fila
restaurada. Os testes de regressão em `work.rs`: `a_snapshot_round_trips_pending_work_and_outputs`
(a fila e os outputs voltam, e a sequência de ids continua),
`an_in_flight_unit_keeps_its_owner_across_a_restart` (o dono sobrevive e o resultado é aceito),
`a_corrupt_primary_snapshot_falls_back_to_the_replica` (a cópia carrega; zero bytes não passa
por vazio) e `an_unknown_owner_is_listed_until_its_node_registers` (o dono ausente é
reoferecido).

## Teto conhecido

**O estado inteiro é reescrito a cada mutação — e agora duas vezes.** O custo cresce com o
tamanho da fila, cada conclusão reescreve os outputs de todas as unidades, e a cópia dobra a
escrita. Ok na escala de benchmark; migrar para um log append-only ou um snapshot periódico
quando a fila crescer ou as mutações ficarem frequentes. Marcado com um comentário `ponytail:`
na escrita.

**A cópia é local.** É um segundo arquivo no mesmo lugar, não uma réplica em outro nó: se o
volume sumir, as duas somem juntas. Replicação de verdade precisa de onde colocar a segunda, e
isso depende de estado por chave, que ainda não existe.

**Não há diretório de estado.** `AI_CLUSTER_STATE` aponta um arquivo, não um diretório de
referências lógicas (`state://req-1842/kv/layer-42`). O `docs/13-state-manager.md` descreve o
directory com primary/replica/localization; nenhuma dessas noções tem consumidor enquanto não
houver estado por chave — KV cache, checkpoint incremental — para endereçar.

**O registry não é persistido.** Nós se recuperam sozinhos — um worker cujo heartbeat não é
reconhecido se re-registra — mas entre um restart e o primeiro heartbeat, `ai-cluster nodes`
não mostra nada. Essa janela é o intervalo de grace, e é o mesmo tempo que uma posse
restaurada espera antes de o dono ausente ser reoferecido.

**Eventos não são persistidos.** O log de eventos começa vazio no reload. A fila de trabalho
era o que precisava sobreviver; a linha do tempo é uma visão de incidente recente por design.

**Não é a fase de storage inteira.** Isto é a fila e seus outputs, com o dono e uma cópia.
Checkpoint de uma unidade no meio do compute e tiers HOT/WARM/COLD são peças separadas da
mesma fase — os tiers estão em `storage-tiers.md`.
