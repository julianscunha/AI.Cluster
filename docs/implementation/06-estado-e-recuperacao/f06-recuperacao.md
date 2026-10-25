---
title: "F06 — Recuperação de execução"
aliases: ["recovery", "Fase I4", "I4"]
tags: [implementacao/fase/06, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-06-estado-e-recuperacao]]"
codigos-antigos: ["I4"]
sistema: ["[[hotplug-e-recuperacao]]", "[[state-manager]]"]
evidencia: ["code/tests/phase-i4-recovery.sh", "code/tests/phase-i4-replica.sh", "code/tests/phase-2-failure.sh"]
---
# F06 — Recuperação de execução

- [ ] Superado

> [!note] Contexto
> O [[state-manager]] descreve a recuperação como uma sequência: `node lost → identify state →
> promote replica / recompute → restore → regenerate placement → continue`. A F03 já reatribuía o
> trabalho de um nó perdido, e a F06 de [[f06-persistencia-e-checkpoint]] entregou a posse das
> unidades em voo e o checkpoint podado. Faltava a **interseção**: uma unidade em voo quando o
> controller cai e cujo dono não volta, e prova de que esse caminho fecha.

## Objetivo

Recuperar execução após falha planejada (worker) e abrupta (controller com unidade em voo), sem
perder o estado recuperável, e sem rodar duas vezes sem necessidade.

## O que foi feito

### A sequência, passo a passo

| `state-manager` | o que existe hoje |
| --- | --- |
| node lost | dois relógios distintos: **grace de heartbeat** (o nó ficou mudo com o controller de pé) e **grace desde o start** (a posse veio do disco e o dono não reapareceu) |
| identify state | `WorkSnapshot.dispatched` + `WorkLease`: cada unidade em voo guarda dono e payload, para que um dono que não volta tenha o que reoferecer |
| promote replica / recompute | **recomputação** quando o dono não volta (`requeue_for_node`) e **promoção** quando o controller perde os próprios arquivos: a cópia que um nó guarda é a única fonte da fila (ver "A cópia num nó") |
| restore | o resultado volta como output de uma unidade concluída, no caminho normal |
| regenerate placement | a unidade reenfileirada é reivindicada pelo próximo nó que perguntar, pelo mesmo `claim_placed` de sempre; não há caminho de placement separado para recuperação |
| continue | o grafo segue; `failed` só aparece quando o orçamento de tentativas acaba e a cascata derruba os dependentes |

### Os dois relógios

O sweep do controller (`code/crates/controller/src/main.rs`, a cada 5 s) trata os dois casos:

- `mark_stale_offline(now, grace)` pergunta ao **registry**: nó registrado que parou de mandar
  heartbeat. É o worker morto com o controller vivo.
- `unknown_owners(known)` pergunta ao **store**: dono que está no snapshot mas não no registry. É
  o caso do restart; o registry não é persistido, então todo dono restaurado é desconhecido até
  provar o contrário.

O segundo só age depois de `grace_ms` contados **do start do controller**, não do despacho
original: o que importa é se o dono voltou depois do restart. Antes disso a unidade espera, de
propósito: o dono pode estar subindo, e reoferecer cedo demais faria o trabalho rodar duas vezes.

Os dois caminhos desembocam em `requeue_for_node`: devolve a unidade à fila enquanto o orçamento de
tentativas dura, ou falha (com cascata nos dependentes) quando acabou.

### A cópia num nó

O controller já escrevia dois arquivos, e os dois moram no mesmo volume: quando o volume morre, a
fila morre com ele. Esta parte cobre a cópia fora do controller, num nó.

- O controller entrega o snapshot numa batida de heartbeat, **só quando a época dele avançou além
  da que o nó reporta**. Um nó em dia não recebe nada: uma batida continua sendo uma batida, não um
  envio de estado a cada 5 s.
- O nó grava a cópia em `AI_CLUSTER_STATE_REPLICA`, com a mesma escrita atômica dos arquivos do
  controller (irmão + rename). Sem essa variável o nó não guarda nada e o resto fica inerte.
- No (re-)registro o nó oferece o que guardou. O controller **adota** quando não tem trabalho nas
  mãos e a cópia é mais nova que a dele; fora disso ignora. Adotar com trabalho nas mãos trocaria a
  fila sendo servida por uma fotografia antiga.
- A adoção grava o estado de volta nos arquivos locais: a cópia volta para casa, e o restart
  seguinte é um restart normal.

O gatilho do re-registro é o de sempre: depois de um restart o registry está vazio, a batida do nó
volta `acknowledged=false`, e o worker se registra de novo. É nessa hora que a cópia sobe.

A **época** ordena tudo isso e mora dentro do snapshot (`WorkSnapshot.epoch_unix_ms`) justamente
para sobreviver ao restart: sem o campo, um controller que acabou de carregar um arquivo pareceria
época zero e tomaria a cópia de um nó por mais nova, e a fila andaria para trás. O avanço é
`max(relógio, época anterior + 1)`, então um relógio que anda para trás não faz o estado vivo
parecer mais velho que uma cópia já no campo.

Uma cópia nunca é mais nova que o arquivo do controller em operação normal: a oferta a um nó vem
**depois** da gravação local. Adotar só acontece quando os dois arquivos locais sumiram.

## Evidência

`bash code/tests/phase-i4-recovery.sh`: uma unidade em voo, o controller morto sem shutdown limpo, e
o dono morto junto.

```text
=== A) node-old reivindica uma unidade e a segura ===
w-0        node-old       dispatched             0            0            0            0
OK: a unidade está em voo em node-old
=== B) node-fast conclui outras duas, levando a posse para o disco ===
conclusoes em node-fast: 2
OK: node-fast concluiu as duas; a gravação levou o lease de node-old ao disco
=== C) kill no controller e em node-old ===
=== D) o controller sobe do mesmo estado ===
estado carregado de /t/state-i4/state.bin: 0 pendente(s), 1 em voo
OK: a unidade em voo voltou do disco, ainda dona de node-old
=== E) passado o grace, a unidade volta para a fila ===
nó node-old não voltou após o restart; 1 unidades em voo reenfileiradas, 0 falharam
OK: o dono ausente foi detectado depois do restart
=== F) node-new termina o trabalho ===
units_completed=1 units_pending=0 units_blocked=0 units_failed=0
w-0        node-new       completed            875           17         1125         2000
OK: a unidade recuperada foi concluída em node-new, não no dono perdido
=== I4 OK ===
```

`1 em voo` no restart é a linha que importa: a unidade **não** voltou para a fila, voltou com dono.
É a diferença entre "o trabalho será recalculado" e "o trabalho será retomado ou, se o dono sumiu,
recalculado só depois de esperar o grace".

- Falha planejada de worker com o controller vivo: `bash code/tests/phase-2-failure.sh`.
- Falha planejada de worker com o controller vivo: `bash code/tests/phase-2-failure.sh`.
- Restart do controller com a fila preservada e a cópia assumindo se o primário for ilegível:
  `bash code/tests/phase-i-state.sh`. Poda do checkpoint: `bash code/tests/phase-i-checkpoint.sh`.
- Volume do controller morto (os dois arquivos apagados) e fila recuperada da cópia de um nó:
  `bash code/tests/phase-i4-replica.sh`. O roteiro falha de propósito se o controller tiver
  carregado um arquivo local: sem essa guarda, "a fila voltou" não diria de onde veio.

```text
=== B) enfileira: a gravação leva a cópia até o nó ===
OK: o nó guardou a cópia em /t/state-i4r/node-a.bin (196718 bytes)
=== C) o controller morre; os dois arquivos locais vão com ele ===
OK: o volume do controller morreu (primário e cópia local apagados)
=== D) o controller volta vazio e o nó oferece a cópia ===
estado restaurado da réplica de um nó: 3 pendente(s), 0 em voo, época 1792633188323
OK: a cópia adotada foi gravada de volta em /t/state-i4r/state.bin
=== E) o cluster drena o trabalho recuperado ===
units_completed=3 units_pending=0 units_blocked=0 units_failed=0
```

- Testes unitários: `an_in_flight_unit_keeps_its_owner_across_a_restart` (dono volta),
  `a_controller_without_files_restores_the_copy_a_node_holds`,
  `a_reloaded_state_keeps_the_epoch_it_was_saved_with`,
  `a_copy_never_overwrites_work_already_in_hand`,
  `a_copy_no_newer_than_the_state_in_hand_is_refused` e
  `a_memory_only_store_keeps_and_takes_no_copy`.

## Limites e próximos passos

> [!warning] A posse é persistida de forma preguiçosa
> `claim` **não** grava o snapshot: quem grava é `enqueue`, `complete` e `requeue_for_node`. Um
> lease só chega ao disco se outra mutação acontecer depois do despacho. Consequências:
> - A seção B do E2E existe por causa disso: sem as duas conclusões de `node-fast`, a posse de
>   `node-old` não estaria no arquivo, e o restart traria a unidade de volta para a **fila**
>   (`0 em voo`). O trabalho ainda termina; o que se perde é quem o segurava.
> - No caso pior, o dono **não** morreu: ele volta, termina, envia o resultado, o controller o
>   recusa (não há registro de despacho), e outro nó recalcula a mesma unidade. Compute duplicado e
>   um retry queimado. O orçamento de tentativas limita o dano, mas não o evita.

- **Motivo do custo:** o snapshot é o conjunto vivo inteiro, e uma unidade de 256³ carrega ~1 MiB
  de payload. Gravar por despacho trocaria escrita O(1) por escrita O(conjunto vivo) a cada
  unidade. Degrau seguinte: persistir **só a tabela de leases** (id → nó) num arquivo pequeno ao
  lado; ou uma tabela só de ids, se o payload parar de caber em RAM.
- **A cópia está no nó, não no cluster.** Cada nó com `AI_CLUSTER_STATE_REPLICA` guarda a cópia
  inteira, e o controller oferece o snapshot inteiro a cada gravação. Não há fator de replicação
  nem lugar designado. Com estado grande isso vira tráfego proporcional a nós × gravações; um
  portador designado, ou taxa de atualização em vez de oferta por gravação, é o degrau seguinte.
- **Sem identidade de cluster na cópia:** um controller novo apontado para nós que guardam a cópia
  de um cluster anterior adotaria aquela fila.
- **O E2E roda num volume compartilhado:** o arquivo do nó está no mesmo disco dos arquivos do
  controller. O que se prova ali é de onde a fila voltou, não a sobrevivência física do disco.
- **Só a fila e os outputs são replicados.** Não há estado por chave (KV cache, execution state,
  model state) no cluster, então não há o que promover além da fila. É o que daria sentido ao
  `state://…` do [[state-manager]]; é o degrau seguinte, na [[fase-12-model-runtime]].
- **Recomputação é sempre do zero.** Não há estado parcial de unidade para retomar: uma unidade
  reoferecida recalcula o matmul inteiro. Recuperação incremental depende das classes de estado,
  que ainda não existem.
- **A reatribuição não desvia do nó que falhou.** Uma unidade reoferecida vai para quem a
  reivindicar em seguida; com um nó só não há para onde ir. Mesmo teto de
  [[f03-tolerancia-a-falhas]].

## Relacionados

- Fase: [[fase-06-estado-e-recuperacao]] · Sistema: [[state-manager]], [[hotplug-e-recuperacao]]
- Irmãos: [[f06-persistencia-e-checkpoint]]
