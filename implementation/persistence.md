---
title: "Persistência de estado do controller"
type: implementation
status: implemented
tags: [implementation, persistence, storage, state]
---

# Persistência de estado do controller

O controller mantinha a fila em memória. Um restart — crash, redeploy, container morto —
perdia tudo que estava na fila e toda unidade que não havia completado. Esta é a fatia de
storage do plano que faz a fila e seus outputs sobreviverem a um restart. Três avanços
posteriores entraram aqui: o **dono** de uma unidade em voo também sobrevive (I1), o arquivo
ganhou uma **cópia** ao lado (I1), e o checkpoint deixou de carregar as matrizes que ninguém
vai ler (I3). Ver `work.rs:68`, onde a fatia anterior registrou a lacuna.

## O que mudou

- `WorkSnapshot`, `WorkOutput` e `WorkLease` no proto: a forma em disco do store.
- `WorkStore::with_state(path)`: abre um store apoiado no snapshot em `path`, carregando-o
  quando existe. Arquivo ausente é primeira execução, não erro.
- `WorkStore::save_locked`: grava o store depois de cada mutação que muda o que está na fila,
  em voo ou concluído.
- `AI_CLUSTER_STATE` nomeia o arquivo de snapshot. Sem ela, é só memória — que é o que os
  benchmarks e os testes usam.
- (I1) o snapshot carrega `dispatched`: as unidades em voo com o nó que as segura, gravadas
  junto do resto.
- (I1) cada gravação escreve dois arquivos: `path` e `path.replica`.
- (I3) `drop_unreferenced_outputs`: o que é gravado é só o resultado que uma unidade ainda não
  terminada pode ler. Antes o snapshot carregava toda matriz já concluída, e reescrevia todas
  elas a cada gravação.

Sem dependência nova: `prost` já está na árvore via `ai-cluster-protocol`, e o snapshot
reusa os tipos gerados em vez de criar um segundo caminho de serialização.

## Semântica

O snapshot guarda o que **ainda não foi feito**: a fila, o que está em voo com o dono, e os
resultados intermediários que uma unidade ainda não terminada vai ler.

**Só o que ainda serve.** Um resultado é lido em dois lugares: `ready` pergunta se o resultado
de uma dependência existe, e `take_locked` substitui uma entrada vinculada por ele. Os dois
leem em nome de uma unidade que não terminou — na fila agora, ou em voo e possivelmente
devolvida à fila por `requeue_for_node` — então só essas unidades seguram um resultado.
Qualquer outro é uma matriz guardada para ninguém, e é o grosso do que o snapshot reescrevia.
A poda roda dentro do `save_locked`, o único ponto por onde toda mutação passa, então a
invariante não pode ser esquecida num chamador. Vale para a memória também: o mapa crescia com
todo o trabalho já concluído. A contrapartida é que um resultado é descartado assim que ninguém
o lê — um snapshot não é um histórico de resultados.

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

**Quanto o checkpoint pesa.** Oito unidades de 256³ sem dependências entre si — oito matrizes
de saída, 8 × 256 × 256 × 8 = 4.194.304 bytes — com o estado ligado, antes e depois da poda:

```
antes   state.bin = 4194410 bytes   (exatamente as 8 matrizes)
depois  state.bin =       2 bytes
```

Oito unidades de trabalho e o estado final cabe em dois varints: nada que uma unidade já
terminada tenha produzido é gravado. O E2E é `bash code/tests/phase-i-checkpoint.sh` — roda as
8 unidades, afirma que o snapshot é menor que uma única matriz de saída, e reinicia o
controller sobre o mesmo arquivo para provar que a poda não corrompeu nem ressuscitou trabalho.
Os testes unitários são `an_output_is_dropped_once_nothing_can_read_it` (o resultado sem leitor
sai) e `an_output_an_unfinished_unit_still_reads_is_kept` (os dois caminhos de leitura — a
dependência e a entrada vinculada — seguram o resultado enquanto a unidade não termina).

## Teto conhecido

**O estado vivo é reescrito a cada mutação — e agora duas vezes.** O custo passou a ser
proporcional ao trabalho *que falta* — a fila, o que está em voo, e os resultados que alguém
ainda vai ler — e não ao trabalho já feito. Antes cada conclusão carregava todas as matrizes de
saída para o snapshot e as reescrevia; medido em 4,19 MB para 8 unidades de 256³, hoje 2 bytes.
A cópia ainda dobra a escrita. Ok na escala de benchmark; um journal de deltas ou um snapshot
periódico quando o conjunto vivo crescer. Marcado com um comentário `ponytail:` na escrita.

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

**Não é a fase de storage inteira.** Isto é a fila e seus resultados, com o dono, uma cópia e a
poda do que ninguém lê. Checkpoint de uma unidade no meio do compute e tiers HOT/WARM/COLD são
peças separadas da mesma fase — os tiers estão em `storage-tiers.md`.

**Um resultado descartado não volta.** A poda só é segura porque o controller nunca precisa
reler uma matriz depois que a última unidade que a lê termina. Se algum dia um resultado tiver
de sobreviver ao seu último leitor — replay, inspeção, uma unidade enfileirada depois do fato —
ele precisa de outro lugar para morar, e não do snapshot de trabalho.
