---
title: "Fase 2 — Placement"
type: implementation
status: implemented
tags: [implementation, phase-2, placement, scheduler, capability]
---

# Fase 2 — Placement

Filtragem de candidatos: uma unidade de trabalho declara a capability de que precisa, e o
controller se recusa a entregá-la a um nó que não a tenha. O dispatch deixa de ser
por ordem de chegada e vira uma decisão.

Esta é a primeira peça do scheduler. Ela filtra candidatos; ainda não os custa. Escolher
*entre* vários nós capazes precisa do modelo de custo e vem depois.

## O que mudou

- `WorkUnit.min_cpu_threads` (campo 10 do proto) declara o requisito. `0` significa
  sem requisito, o que vale para toda unidade que o controller constrói hoje, exceto as
  que um cliente marca.
- `EnqueueWorkRequest.min_cpu_threads` (campo 6 do proto) permite um cliente defini-lo.
- `WorkStore::claim` recebe o `cpu_threads` do nó que pede e pula unidades que o
  nó não consegue rodar. O nó continua fazendo poll e pega a próxima unidade que pode assumir.
- `Registry::capability` expõe o último `CapabilityReport` reportado para um
  nó. O registry já o guardava; nada o lia de volta por nó antes.
- `request_work` lê a capability do nó e passa seu `cpu_threads` para
  `claim`, então o filtro roda no único lugar onde o trabalho é entregue.
- O worker reporta menos threads que o host tem quando
  `AI_CLUSTER_CPU_THREADS` está definida. Sem definir, reporta a realidade. Existe para que um
  único host possa fazer as vezes de duas máquinas de tamanhos diferentes.
- `ai-cluster benchmark --min-threads N` define o requisito nas unidades que
  enfileira.

## Semântica

O requisito é **duro**, não uma preferência. Uma unidade que precisa de mais threads do que
um nó tem não é entregue a ele de jeito nenhum — mesmo que isso signifique que a unidade espere. Um nó
que reporta menos threads do que uma unidade precisa recebe a próxima unidade que *pode* rodar, ou
nada.

Isso importa porque o modelo de dispatch é baseado em pull: os workers fazem poll
em `RequestWork`, e a única alavanca do controller é o que dar a cada solicitante. Então
o placement é expresso como um filtro na fila, não como uma escolha do controller sobre
o nó. A primeira unidade pronta que tanto tem suas dependências satisfeitas quanto se encaixa no
solicitante é a que sai.

Duas consequências se seguem, e ambas são deliberadas:

- Uma unidade cujo requisito nenhum nó sobrevivente atende fica pendente. É o
  mesmo caso de "o cluster não consegue rodar isto" que um cluster vazio; não há tratamento
  separado nem erro retornado. Ver o teto abaixo.
- O filtro é aplicado por poll. Um nó fraco fazendo poll em laço apertado não
  pega uma unidade pesada; ele só não recebe trabalho até aparecer uma unidade leve ou um nó
  capaz pegar a pesada.

## Evidência

Reproduza com `bash code/tests/phase-2-placement.sh`. Um nó fraco reportando 2
threads sobe sozinho; uma unidade exigindo 4 threads é enfileirada; nada termina;
então um nó forte reportando 8 threads entra e pega a unidade.

```
--- nodes: the weak node reports 2 threads ---
NODES 1
NODE                 STATE      ARCH       THREADS    RAM_GB   HEALTH
node-weak            READY      x86_64           2      3.77     0.73
=== enqueue a unit needing 4 threads; only the weak node is present ===
enqueued 1 work units of 256x256
--- controller log during the weak-only window ---
OK: nothing finished; the weak node did not take the heavy unit
=== start the strong node; the waiting unit should go to it ===
WORK       NODE             COMPUTE_US  TRANSFER_US     TOTAL_US
w-0        node-strong            5708         8292        14000
completed=1 pending=0 failed=0 avg_compute_us=5708 avg_transfer_us=8292
```

`w-0 node-strong` é a linha que importa. O nó fraco era o único nó
presente enquanto a unidade estava na fila e nunca a rodou; a unidade terminou no nó
forte assim que ele entrou. Por ordem de chegada, a unidade teria ido para o nó fraco
no momento em que ele fez poll, e a execução teria terminado com
`w-0 node-weak`.

O filtro em si é coberto pelo teste unitário
`a_weak_node_is_skipped_for_a_unit_that_requires_more_threads`, que verifica os
três casos num só lugar: um nó fraco recebe a unidade leve em vez da pesada,
um nó capaz recebe a pesada, e um nó fraco sozinho não recebe nada.

## Teto conhecido

**Só cpu_threads é filtrado.** Memória, precisão, aceleradores e armazenamento são
reportados pelo worker e guardados pelo registry, mas nenhuma unidade pode exigi-los
ainda. O campo de requisito é um número; estenda-o para uma mensagem de requisitos quando
a próxima capability importar.

**Uma unidade cujo requisito nenhum nó atende fica pendente.** Sem erro, sem fail-fast:
o cliente faz poll até seu próprio deadline. O placement não consegue detectar "isto nunca vai
rodar" porque só vê o nó que está perguntando, não o cluster inteiro. Fechar
isso significa resolver requisitos contra o registry quando a unidade é enfileirada,
ou quando o último nó capaz sai.

**Sem modelo de custo.** Entre nós que todos satisfazem o requisito, o primeiro a fazer poll
vence. Escolher o nó capaz mais barato precisa de estimativas de compute, comunicação e fila —
a próxima fatia do scheduler.
