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

Esta é a primeira peça do scheduler. Ela filtra candidatos; o modelo de custo (G2) já os
custa, mas a escolha final entre candidatos ainda é do nó que faz poll — ver o modelo abaixo.

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

## Modelo de custo (G2)

`code/crates/controller/src/placement.rs` estima o custo de rodar uma unidade num nó antes do
dispatch, para que o scheduler possa ordenar candidatos capazes em vez de aceitar o primeiro
que faz poll. É uma função pura — mesmas entradas, mesma pontuação — então a ordenação é
testável sem cluster.

Custo = compute + transferência + fila, tudo escalado por saúde. Menor é melhor.

- **compute** — flops da unidade (2 por elemento de saída) sobre a taxa do nó. Usa
  `compute_score` quando o worker reportou um; cai para a contagem de threads enquanto isso,
  porque o worker deixa `compute_score` em 0 até o benchmark de capability rodar.
- **transferência** — bytes movidos (A e B lidos, C escrito) sobre a taxa de rede do nó. Cai
  para 1.0 quando não medida.
- **fila** — peso por unidade já em voo para o nó; uma unidade enfileirada espera. É o termo
  que impede um nó ocupado mas nominalmente mais rápido de vencer sempre.
- **saúde** — multiplica o total. Um nó degradado perde empates contra um saudável de igual
  capability. `health_score = 0` significa "não reportado" e é ignorado, não lido como péssimo.

A pontuação é relativa, não um tempo. As constantes são placeholders até o benchmark reportar
taxas reais; só a *ordem* entre nós é significativa hoje. Os testes unitários do módulo cobrem
as direções: mais threads custa menos, unidade maior custa mais, nó com fila custa mais, saúde
degradada custa mais, e saúde não reportada não penaliza.

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

**Modelo de custo sem locality nem térmica.** A função cobre compute, transferência, fila e
saúde; localidade de dados, folga térmica e custo de mover estado não são modelados porque o
registry ainda não carrega esses dados. Além disso, o custo ordena candidatos, mas o dispatch
continua por poll: o modelo já existe e é testado, mas o controller ainda não o usa para
escolher — ligá-lo ao dispatch é a G3.
