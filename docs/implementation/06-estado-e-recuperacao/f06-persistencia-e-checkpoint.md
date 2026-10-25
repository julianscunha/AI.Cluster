---
title: "F06 — Persistência e checkpoint do controller"
aliases: ["persistence", "Fase I1", "Fase I3", "I1", "I3"]
tags: [implementacao/fase/06, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-06-estado-e-recuperacao]]"
codigos-antigos: ["I1", "I3"]
sistema: ["[[state-manager]]"]
evidencia: ["code/tests/phase-i-state.sh", "code/tests/phase-i-checkpoint.sh"]
---
# F06 — Persistência e checkpoint do controller

- [ ] Superado

> [!note] Contexto
> O controller mantinha a fila em memória: um restart (crash, redeploy, container morto) perdia
> a fila e toda unidade que não havia completado. Esta fatia faz a fila e seus resultados
> sobreviverem a um restart. Duas evoluções entraram aqui: o **dono** de uma unidade em voo
> sobrevive (I1), o arquivo ganhou uma **cópia** ao lado (I1), e o checkpoint deixou de carregar
> as matrizes que ninguém vai ler (I3). A recuperação que usa esse estado está em
> [[f06-recuperacao]].

## Objetivo

Persistir o estado de execução do controller (fila, unidades em voo com dono e resultados ainda
necessários) de modo que um restart não perca trabalho e não rode duas vezes sem necessidade.

## O que foi feito

- `WorkSnapshot`, `WorkOutput` e `WorkLease` no proto: a forma em disco do store.
- `WorkStore::with_state(path)`: abre um store apoiado no snapshot em `path`, carregando-o quando
  existe. Arquivo ausente é primeira execução, não erro.
- `WorkStore::save_locked`: grava o store depois de cada mutação que muda o que está na fila, em
  voo ou concluído.
- `AI_CLUSTER_STATE` nomeia o arquivo de snapshot. Sem ela é só memória, que é o que os
  benchmarks e os testes usam.
- (I1) o snapshot carrega `dispatched`: as unidades em voo com o nó que as segura.
- (I1) cada gravação escreve dois arquivos: `path` e `path.replica`.
- (I3) `drop_unreferenced_outputs`: o que é gravado é só o resultado que uma unidade ainda não
  terminada pode ler. Antes o snapshot carregava toda matriz concluída e reescrevia todas a cada
  gravação.
- Sem dependência nova: `prost` já está na árvore via `ai-cluster-protocol`, e o snapshot reusa os
  tipos gerados em vez de criar um segundo caminho de serialização.

## Decisões

**Só o que ainda serve.** Um resultado é lido em dois lugares: `ready` pergunta se o resultado de
uma dependência existe, e `take_locked` substitui uma entrada vinculada por ele. Os dois leem em
nome de uma unidade que não terminou (na fila, ou em voo e possivelmente devolvida por
`requeue_for_node`), então só essas unidades seguram um resultado. Qualquer outro é uma matriz
guardada para ninguém, e é o grosso do que o snapshot reescrevia. A poda roda dentro de
`save_locked`, o único ponto por onde toda mutação passa, então a invariante não pode ser
esquecida por um chamador. Vale para a memória também: o mapa crescia com todo o trabalho já
concluído. A contrapartida: um resultado é descartado assim que ninguém o lê. Snapshot não é
histórico de resultados.

**Posse.** Uma unidade em voo volta ao reload ainda pertencendo ao mesmo nó, e não para a fila.
O resultado que aquele worker estava calculando é aceito em vez de recusado. Sem o lease, o
controller recarregado não tinha registro de dispatch e descartava exatamente o resultado já
computado. Depois de um restart o registry está vazio, então o dono não é reconhecido de imediato:
a varredura espera o grace e, se o nó não voltar, trata a unidade como as de um nó perdido —
reoferece, ou falha se o orçamento de tentativas acabou (`WorkStore::unknown_owners`).

**Cópia.** O primário é lido primeiro; se estiver ausente, ilegível ou decodificar como vazio, a
cópia ao lado é carregada no lugar. As duas são escritas na mesma gravação, então podem diferir em
idade, nunca em validade. Só quando nenhuma serve o store começa vazio, porque um snapshot
corrompido não pode impedir o cluster de aceitar trabalho.

**Zero bytes não é "sem trabalho".** Um arquivo vazio decodifica como snapshot default (todos os
campos no valor zero), o que passaria por "nada a fazer" mesmo com a fila sumida. Zero bytes é
tratado como arquivo não carregável, e a cópia tem a vez.

**Não salva no `claim`.** Uma unidade reivindicada e ainda não gravada está no `pending` do
snapshot anterior, então volta sem uma escrita por poll. Salvar a cada claim poria o estado
inteiro em disco na frequência do poll (com o poll ocioso de 200 ms, uma escrita por worker por
poll) sem segurança adicional. Custo: a unidade perde o dono e pode rodar duas vezes; para uma
multiplicação de matrizes determinística, é a mesma resposta. Ver [[f06-recuperacao]].

**Escrita atômica.** Cada arquivo é escrito primeiro num irmão e depois renomeado para o lugar. Um
crash no meio da escrita deixa a gravação anterior intacta. Uma escrita que falha é reportada e
nunca fatal: o estado em memória continua autoritativo para a execução.

## Evidência

Um DAG diamante enfileirado sem worker rodando, o controller morto por `docker kill`, o primário
zerado, depois um controller novo sobre o mesmo arquivo:

```text
estado carregado de /t/state-i1/state.bin.replica: 4 pendente(s), 0 em voo
controller escutando em 0.0.0.0:50051 (graça de heartbeat 15000ms)
```

Um worker entrou e o grafo drenou sem ser re-enfileirado:

```text
nodes_total=1 nodes_available=1
units_completed=4 units_pending=0 units_blocked=0 units_failed=0
total_compute_time_us=3466 avg_compute_time_us=866
```

- `bash code/tests/phase-i-state.sh`: quatro pernas: grava os dois arquivos, mata o controller sem
  shutdown limpo, zera o primário e carrega da cópia, e deixa um worker drenar a fila restaurada.
- Testes unitários em `code/crates/controller/src/work.rs`:
  `a_snapshot_round_trips_pending_work_and_outputs` (fila, outputs e sequência de ids voltam),
  `an_in_flight_unit_keeps_its_owner_across_a_restart` (dono sobrevive, resultado aceito),
  `a_corrupt_primary_snapshot_falls_back_to_the_replica` (cópia carrega; zero bytes não passa por
  vazio) e `an_unknown_owner_is_listed_until_its_node_registers` (dono ausente é reoferecido).
- Tamanho do checkpoint: oito unidades de 256³ sem dependências entre si (oito matrizes de saída,
  8 × 256 × 256 × 8 = 4.194.304 bytes), com o estado ligado:

```text
antes   state.bin = 4194410 bytes   (exatamente as 8 matrizes)
depois  state.bin =       2 bytes
```

- `bash code/tests/phase-i-checkpoint.sh`: roda as 8 unidades, afirma que o snapshot é menor que
  uma única matriz de saída e reinicia o controller sobre o mesmo arquivo para provar que a poda não
  corrompeu nem ressuscitou trabalho.
- Testes de poda: `an_output_is_dropped_once_nothing_can_read_it` (resultado sem leitor sai) e
  `an_output_an_unfinished_unit_still_reads_is_kept` (os dois caminhos de leitura seguram o
  resultado enquanto a unidade não termina).

## Limites e próximos passos

> [!warning] Teto: o estado vivo é reescrito a cada mutação, e duas vezes
> O custo é proporcional ao trabalho *que falta* (fila, voo, resultados ainda lidos), não ao já
> feito. Medido em 4,19 MB para 8 unidades de 256³, hoje 2 bytes. A cópia ainda dobra a escrita.
> Ok na escala de benchmark; um journal de deltas ou snapshot periódico quando o conjunto vivo
> crescer. Marcado com um comentário `ponytail:` na escrita.

- **A cópia é local.** Um segundo arquivo no mesmo lugar, não réplica em outro nó: se o volume
  sumir, as duas somem juntas. Replicação real depende de estado por chave, que não existe.
- **Não há diretório de estado.** `AI_CLUSTER_STATE` aponta um arquivo, não um diretório de
  referências lógicas (`state://req-1842/kv/layer-42`). O [[state-manager]] descreve o diretório
  com primary/replica/localization; nenhuma dessas noções tem consumidor enquanto não houver estado
  por chave (KV cache, checkpoint incremental).
- **O registry não é persistido.** Nós se recuperam sozinhos (um worker cujo heartbeat não é
  reconhecido se re-registra), mas entre um restart e o primeiro heartbeat `ai-cluster nodes` não
  mostra nada. Essa janela é o grace, o mesmo tempo que uma posse restaurada espera.
- **Eventos não são persistidos.** O log de eventos começa vazio no reload. A fila era o que
  precisava sobreviver; a linha do tempo é visão de incidente recente por design.
- **Não é a fase de storage inteira.** Isto é a fila e seus resultados, com dono, cópia e poda.
  Checkpoint de uma unidade no meio do compute e tiers HOT/WARM/COLD são peças separadas: os tiers
  estão em [[f10-classes-de-armazenamento]].
- **Um resultado descartado não volta.** A poda só é segura porque o controller nunca relê uma
  matriz depois que a última unidade que a lê termina. Se um resultado precisar sobreviver ao seu
  último leitor (replay, inspeção), ele precisa de outro lugar para morar, não do snapshot de
  trabalho.

## Relacionados

- Fase: [[fase-06-estado-e-recuperacao]] · Sistema: [[state-manager]], [[hotplug-e-recuperacao]]
- Irmãos: [[f06-recuperacao]]
