---
title: "Recuperação de execução"
type: implementation
status: implemented
tags: [implementation, recovery, state-manager, phase-i]
---

# Recuperação de execução

O `docs/13-state-manager.md` descreve a recuperação como uma sequência: `node lost → identify
state → promote replica / recompute → restore → regenerate placement → continue`. As fatias I1
a I3 entregaram o diretório de estado, a posse das unidades em voo e o checkpoint podado; a
Fase 2 já reatribuía o trabalho de um nó perdido. O que faltava era a **interseção** das duas
coisas — uma unidade em voo quando o controller cai *e* cujo dono não volta — e prova de que
esse caminho fecha. Esta é a fatia I4.

## A sequência, passo a passo

| `docs/13` | o que existe hoje |
| --- | --- |
| node lost | dois relógios distintos: **grace de heartbeat** (o nó ficou mudo com o controller de pé) e **grace desde o start** (a posse veio do disco e o dono não reapareceu) |
| identify state | `WorkSnapshot.dispatched` + `WorkLease` — cada unidade em voo guarda dono e payload, para que um dono que não volta ainda tenha o que reoferecer |
| promote replica / recompute | **recomputação**: `requeue_for_node` devolve a unidade à fila. Réplica não existe — ver teto |
| restore | o resultado volta como o output de uma unidade concluída, no caminho normal |
| regenerate placement | a unidade reenfileirada é reivindicada pelo próximo nó que perguntar, pelo mesmo `claim_placed` de sempre — não há caminho de placement separado para recuperação |
| continue | o grafo segue; `failed` só aparece quando o orçamento de tentativas acaba e a cascata derruba os dependentes |

## Os dois relógios

O sweep do controller (`main.rs`) roda a cada 5s e trata os dois casos:

- `mark_stale_offline(now, grace)` pergunta ao **registry**: um nó registrado que parou de
  mandar heartbeat. É o caso do worker morto com o controller vivo.
- `unknown_owners(known)` pergunta ao **store**: um dono que está no snapshot mas não no
  registry. É o caso do restart — o registry não é persistido (só a fila é, B1), então todo
  dono restaurado é desconhecido até provar o contrário.

O segundo só age depois de `grace_ms` contados **do start do controller**, e não do despacho
original: o que importa é se o dono voltou depois do restart. Antes disso a unidade espera, de
propósito — o dono pode estar subindo, e reoferecer cedo demais faria o trabalho rodar duas
vezes.

Os dois caminhos desembocam na mesma função, `requeue_for_node`: devolve a unidade à fila
enquanto o orçamento de tentativas dura, ou a falha (com cascata nos dependentes) quando
acabou.

## Evidência

`bash code/tests/phase-i4-recovery.sh`. O roteiro monta o caso que nenhum outro cobria: uma
unidade em voo, o controller morto sem shutdown limpo, e o dono morto junto.

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

`1 em voo` no restart é a linha que importa: a unidade **não** voltou para a fila — voltou com
dono. É a diferença entre "o trabalho será recalculado" e "o trabalho será retomado ou, se o
dono sumiu, recalculado só depois de esperar o grace".

As outras partes do caminho têm prova própria: falha planejada de worker com o controller vivo
(`phase-2-failure.sh`), restart do controller com a fila preservada e a cópia assumindo se o
primário for ilegível (`phase-i-state.sh`), poda do checkpoint (`phase-i-checkpoint.sh`). O
teste unitário do caso "dono volta" é `an_in_flight_unit_keeps_its_owner_across_a_restart`.

## Teto conhecido

**A posse é persistida de forma preguiçosa.** `claim` **não** grava o snapshot: quem grava é
`enqueue`, `complete` e `requeue_for_node`. Um lease só chega ao disco se alguma outra mutação
acontecer depois do despacho. Consequências, ambas vistas no roteiro acima:

- A seção B do E2E existe só por causa disso — sem as duas conclusões de `node-fast`, a posse
  de `node-old` não estaria no arquivo, e o restart teria trazido a unidade de volta para a
  **fila** (`0 em voo`). O trabalho ainda termina; o que se perde é a informação de quem o
  segurava.
- No caso pior, o dono **não** morreu: ele volta, termina, envia o resultado e o controller o
  recusa (não há registro de despacho), enquanto outro nó recalcula a mesma unidade. Compute
  duplicado, um retry queimado. O orçamento de tentativas limita o dano, mas não o evita.

O motivo de não gravar no claim é o custo: o snapshot é o conjunto vivo inteiro, e uma unidade
de 256³ carrega ~1 MiB de payload. Gravar por despacho trocaria escrita O(1) por escrita
O(conjunto vivo) a cada unidade. O degrau seguinte é persistir **só a tabela de leases**
(id → nó) num arquivo pequeno ao lado, que muda devagar e é barato de reescrever; ou uma
tabela só de ids, se o payload parar de caber em RAM.

**Sem promoção de réplica.** Nada é replicado para outro nó: não há KV cache, execution state
nem model state no cluster — os únicos estados são a fila e os outputs, e o output de uma
unidade concluída é reconstruível. Promover réplica é o degrau seguinte, e é o que dá sentido
ao `state://…` do `docs/13`.

**Recomputação é sempre do zero.** Não há estado parcial de uma unidade para retomar: uma
unidade reoferecida recalcula o matmul inteiro. Recuperação incremental depende das classes de
estado do `docs/13`, que ainda não existem.

**A reatribuição não desvia do nó que falhou.** Uma unidade reoferecida vai para quem a
reivindicar em seguida; com um nó só não há para onde ir. Mesmo teto do `fault-tolerance.md`.
