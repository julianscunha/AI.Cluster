---
title: "Fase 2 — Placement"
type: implementation
status: implemented
tags: [implementation, phase-2, placement, scheduler, capability]
---

# Fase 2 — Placement

Filtragem de candidatos: o controller se recusa a entregar uma unidade a um nó que não a
possa rodar. O requisito vem de duas fontes — o `min_cpu_threads` que a unidade declara e o
footprint da própria unidade contra a memória que o nó reporta. O dispatch deixa de ser por
ordem de chegada e vira uma decisão.

Esta é a primeira peça do scheduler. Ela filtra candidatos; o modelo de custo (G2) os custa
e o scheduler global (G3) usa esse custo para decidir — o nó que não é o melhor candidato
cede a vez. Ver as duas seções abaixo.

## O que mudou

- `WorkUnit.min_cpu_threads` (campo 10 do proto) declara o requisito. `0` significa
  sem requisito, o que vale para toda unidade que o controller constrói hoje, exceto as
  que um cliente marca.
- `EnqueueWorkRequest.min_cpu_threads` (campo 6 do proto) permite um cliente defini-lo.
- `WorkStore::claim` e `claim_placed` recebem o `CapabilityReport` do nó que pede e pulam
  unidades que ele não consegue rodar. O nó continua fazendo poll e pega a próxima unidade que
  pode assumir.
- `placement::fits` é o único lugar onde a aptidão de um candidato é decidida: passa
  `min_cpu_threads` e o footprint contra threads e memória. Os dois caminhos de despacho o usam,
  então o filtro não pode divergir entre eles.
- `placement::unit_footprint_bytes` deriva o requisito de memória da própria unidade — A e B
  lidos, C escrito, 8 bytes por elemento. Nenhum campo novo no proto: o requisito não é
  declarado, é calculado, porque o controller já sabe o tamanho da unidade.
- `Registry::capability` expõe o último `CapabilityReport` reportado para um
  nó. O registry já o guardava; nada o lia de volta por nó antes.
- `request_work` lê a capability do nó e a passa para `claim`/`claim_placed`, então o filtro
  roda no único lugar onde o trabalho é entregue.
- O worker reporta menos threads e menos memória que o host tem quando
  `AI_CLUSTER_CPU_THREADS` / `AI_CLUSTER_MEMORY_BYTES` estão definidas. Sem definir, reporta a
  realidade. Existem para que um único host possa fazer as vezes de duas máquinas de tamanhos
  diferentes.
- `ai-cluster benchmark --min-threads N` define o requisito nas unidades que
  enfileira.

## Semântica

O requisito é **duro**, não uma preferência. Uma unidade que precisa de mais threads, ou de
mais memória, do que um nó tem não é entregue a ele de jeito nenhum — mesmo que isso signifique
que a unidade espere. Um nó que não comporta a unidade recebe a próxima unidade que *pode*
rodar, ou nada.

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

## Evidência — threads

Reproduza com `bash code/tests/phase-2-placement.sh`. Um nó fraco reportando 2
threads sobe sozinho; uma unidade exigindo 4 threads é enfileirada; nada termina;
então um nó forte reportando 8 threads entra e pega a unidade.

```
--- nós: o nó fraco reporta 2 threads ---
NÓS 1
NÓ                   ESTADO     ARQ        THREADS    RAM_GB   SAÚDE
node-weak            READY      x86_64           2      3.77     0.73
=== enfileirar uma unidade que precisa de 4 threads; apenas o nó fraco está presente ===
enfileiradas 1 unidades de trabalho de 256x256
--- log do controller durante a janela só-fraco ---
OK: nada finalizou; o nó fraco não pegou a unidade pesada
=== iniciar o nó forte; a unidade em espera deve ir para ele ===
TRABALHO   NÓ               COMPUTE_US  TRANSFER_US     TOTAL_US
w-0        node-strong            5708         8292        14000
concluidas=1 pendentes=0 falharam=0 media_compute_us=5708 media_transfer_us=8292
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

## Evidência — memória

Reproduza com `bash code/tests/phase-g-memory.sh`. Um nó reportando 1.000.000 bytes sobe
sozinho; uma unidade 256x256 (footprint de 1.572.864 bytes) é enfileirada; nada termina; então
um nó reportando 2.000.000.000 bytes entra e pega a unidade.

```text
--- unidade 256x256 precisa de 1572864 bytes ---
NÓS 1
NÓ                   ESTADO     ARQ        THREADS    RAM_GB    SAÚDE
node-small           READY      x86_64           4      0.00     0.75
--- log do controller durante a janela só-pequeno ---
OK: nada finalizou; o nó pequeno não pegou uma unidade que não cabe nele
TRABALHO   NÓ               COMPUTE_US  TRANSFER_US     TOTAL_US
w-0        node-large             5676        14324        20000
```

`w-0 node-large` é a linha que importa. O nó pequeno estava presente e fazendo poll enquanto a
unidade esperava, e nunca a pegou, porque o footprint dela não cabia nos bytes que ele reportou.
Repare que as threads dos dois nós são as mesmas (4): o discriminante aqui é só a memória.

Coberto pelos testes unitários `a_node_that_cannot_hold_the_unit_is_skipped` (limite exato:
95 bytes não cabe, 96 cabe), `footprint_is_the_three_matrices_in_bytes` e
`a_unit_that_does_not_fit_the_reported_memory_is_excluded`.
`unreported_memory_does_not_filter_a_unit_out` fixa a convenção de que `memory_bytes = 0`
significa "não reportado" e não "sem RAM".

## Modelo de custo (G2)

`code/crates/controller/src/placement.rs` estima o custo de rodar uma unidade num nó antes do
dispatch, para que o scheduler possa ordenar candidatos capazes em vez de aceitar o primeiro
que faz poll. É uma função pura — mesmas entradas, mesma pontuação — então a ordenação é
testável sem cluster.

Custo = compute + transferência + fila, tudo escalado por saúde. Menor é melhor.

- **compute** — flops da unidade (2 por elemento de saída) sobre a taxa do nó
  (`threads × compute_score`). O `compute_score` vem do benchmark de capability
  (`phase-2/capability-benchmark.md`): é a taxa por core que o worker mediu no próprio kernel
  das unidades. Um nó que ainda não mediu cai para 1.0 por core — exatamente o que o fallback
  de contagem de threads sempre assumiu — então ligar o benchmark não reordena um cluster onde
  ninguém mediu.
- **transferência** — bytes movidos (A e B lidos, C escrito) sobre a taxa de rede do nó. Cai
  para 1.0 quando não medida.
- **fila** — peso por unidade já em voo para o nó; uma unidade enfileirada espera. É o termo
  que impede um nó ocupado mas nominalmente mais rápido de vencer sempre.
- **saúde** — multiplica o total. Um nó degradado perde empates contra um saudável de igual
  capability. `health_score = 0` significa "não reportado" e é ignorado, não lido como péssimo.

A pontuação é relativa, não um tempo. O termo de compute já usa a taxa que o benchmark de
capability mediu; os pesos (compute, transferência, fila, saúde) e a taxa de rede continuam
placeholders — só a *ordem* entre nós é significativa hoje. Os testes unitários do módulo
cobrem as direções: mais threads custa menos, unidade maior custa mais, nó com fila custa
mais, saúde degradada custa mais, e saúde não reportada não penaliza.

## Scheduler global (G3)

`WorkStore::claim_placed` (em `code/crates/controller/src/work.rs`) usa o custo para decidir
quem recebe a próxima unidade. O `request_work` monta o conjunto de candidatos com
`Registry::candidates` — os nós disponíveis com sua capability e saúde — e passa para o claim
junto com o requisito da unidade.

Regra: o nó que pergunta só leva a unidade quando é o **candidato ocioso mais barato** para
ela — ou **quase** isso. Se outro nó ocioso tem custo menor, o nó que pergunta não recebe
nada e volta a fazer poll; a unidade fica na fila e o nó que encaixa melhor a pega. É isso
que vira o dispatch de primeiro-que-pergunta em decisão de placement.

Duas escolhas deliberadas:

- **Só candidatos ociosos entram no ranking.** Um nó com unidade em voo não está fazendo
  poll, então não pode pegar esta unidade agora e não deve segurá-la. Um nó ocupado porém
  mais rápido não bloqueia um nó ocioso mais fraco.
- **Histerese (G5): o nó só cede quando o outro é mais barato por mais que a banda.** Ceder
  não é de graça — a unidade espera até um intervalo de poll para o outro nó perguntar —
  então recusar ceder por uma diferença de arredondamento troca uma ociosidade certa por uma
  economia que pode não existir. A banda também substitui o antigo desempate por id: nós
  quase iguais não se recusam mais, então uma unidade não pode ficar presa entre eles. Banda
  zero significa "estritamente mais barato", a política anterior à G5.

`AI_CLUSTER_PLACEMENT_TOLERANCE` (padrão `0.10`) ajusta a banda de histerese;
`AI_CLUSTER_PLACEMENT=off` desliga o scheduler e volta ao primeiro-que-pergunta, para o
benchmark comparar as duas políticas na mesma carga sem recompilar.

## Teto conhecido

**Precisão, aceleradores e armazenamento ainda não filtram.** O worker reporta esses campos e
o registry os guarda, mas nenhuma unidade pode exigi-los. `placement::fits` é o lugar onde a
próxima dimensão entra.

**Trust não tem estado próprio.** A filtragem de trust hoje é a exclusão por estado: um nó
`QUARANTINED`, `FAILED` ou `OFFLINE` não é candidato (`registry::eligible`). Os níveis de trust
do `docs/06` (`UNKNOWN` → `TRUSTED`) dependem do fluxo de enrollment do `docs/19`, que ainda
não existe; implementá-los antes disso deixaria todo nó inelegível ou contradiria a spec.
Fechar quando o provisionamento existir.

**Uma unidade cujo requisito nenhum nó atende fica pendente.** Sem erro, sem fail-fast:
o cliente faz poll até seu próprio deadline. O placement não consegue detectar "isto nunca vai
rodar" porque só vê o nó que está perguntando, não o cluster inteiro. Fechar
isso significa resolver requisitos contra o registry quando a unidade é enfileirada,
ou quando o último nó capaz sai.

**Modelo de custo sem locality nem térmica.** A função cobre compute, transferência, fila e
saúde; localidade de dados, folga térmica e custo de mover estado não são modelados porque o
registry ainda não carrega esses dados.

**Um nó ocioso que para de fazer poll pode segurar uma unidade.** O controller infere "ocioso"
de não ter unidade em voo, e um nó parado continua mandando heartbeat, então nunca é marcado
offline. A janela é curta porque um nó ocioso faz poll a cada 200ms; um nó travado de verdade
ainda não é distinguido. Revisitar com idade do último poll ou um contador de recusas.

**Só a primeira unidade elegível é avaliada.** O ranking decide *quem* pega a unidade, não
*qual* das unidades prontas encaixa melhor no nó que pergunta. Com unidades de tamanhos
diferentes, escolher a de melhor encaixe pode adiantar mais — é a G4.

**Evidência.** `bash code/tests/phase-g-placement.sh`. Leg A: só o nó fraco (2 threads)
presente e `AI_CLUSTER_PLACEMENT=off`; a unidade roda nele. Leg B: nó fraco e nó forte
(8 threads) presentes e ociosos, placement ligado; o fraco cede e a unidade roda no forte. O
nó fraco leva 2000ms de atraso (`AI_CLUSTER_WORK_DELAY_MS`) para representar uma máquina
lenta, já que os dois rodam no mesmo host e compartilhariam a CPU.

```text
--- A) baseline primeiro-a-chegar: apenas o nó fraco (2 threads, 2000ms de atraso) ---
w-0        node-weak              7043      2012957      2020000
--- B) placement: nó fraco + nó forte (8 threads), ambos ociosos ---
w-0        node-strong            7098        10902        18000
```

As duas linhas são a mesma unidade: sob primeiro-que-pergunta ela rodou no nó fraco e levou
2.020.000us; sob placement foi para o nó capaz e levou 18.000us. O mecanismo em si é coberto
pelos testes unitários `placement_gives_a_unit_to_the_cheapest_idle_node`,
`a_busy_cheaper_node_does_not_hold_up_an_idle_node`, `the_only_idle_candidate_still_takes_work`
e `a_near_equal_node_takes_the_unit_instead_of_yielding`, além de `placement_prefers_the_cheaper_idle_node` no
serviço.
