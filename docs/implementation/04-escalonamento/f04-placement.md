---
title: "F04 — Placement"
aliases: ["phase-2/placement", "F3", "G1", "G2", "G3", "G5"]
tags: [implementacao/fase/04, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-04-escalonamento]]"
codigos-antigos: ["F3", "G1", "G2", "G3", "G5", "roadmap-2"]
sistema: ["[[scheduler]]", "[[capability-engine]]", "[[saude-e-confianca]]"]
evidencia: ["code/tests/phase-2-placement.sh", "code/tests/phase-g-memory.sh", "code/tests/phase-g-placement.sh"]
---
# F04 — Placement

- [ ] Superado

## Objetivo

Filtrar candidatos: o controller se recusa a entregar uma unidade a um nó que não a possa rodar (filtro duro por threads e memória). Depois, custar os candidatos aptos e decidir quem leva a unidade, com histerese para não trocar ociosidade certa por economia incerta. O dispatch deixa de ser por ordem de chegada e vira decisão.

## O que foi feito

- `WorkUnit.min_cpu_threads` (campo 10 do proto) declara o requisito. `0` significa sem requisito. `EnqueueWorkRequest.min_cpu_threads` (campo 6) permite ao cliente defini-lo.
- `WorkStore::claim` e `claim_placed` recebem o `CapabilityReport` do nó que pede e pulam unidades que ele não roda. O nó continua fazendo poll e pega a próxima unidade que pode assumir.
- `placement::fits` é o único lugar onde a aptidão é decidida: `min_cpu_threads` contra threads e footprint contra memória. Os dois caminhos de despacho o usam, então o filtro não diverge.
- `placement::unit_footprint_bytes` deriva o requisito de memória da própria unidade: A e B lidos, C escrito, 8 bytes por elemento. Nenhum campo novo no proto; o requisito é calculado.
- `Registry::capability` expõe o último `CapabilityReport` de cada nó; `request_work` o passa a `claim`/`claim_placed`.
- `Registry::candidates` monta o conjunto de candidatos (nós disponíveis, com capability e saúde) para o ranking.
- Worker reporta menos threads e memória que o host quando `AI_CLUSTER_CPU_THREADS` / `AI_CLUSTER_MEMORY_BYTES` estão definidas. Sem elas, reporta a realidade. Permite que um único host faça as vezes de máquinas de tamanhos diferentes.
- `ai-cluster benchmark --min-threads N` define o requisito nas unidades enfileiradas.
- **F3 — `placement::decide`.** A política de ranking saiu de `WorkStore::claim_placed` para `placement::decide` (`code/crates/controller/src/placement.rs`), uma função pura: recebe a unidade, o nó que pergunta, os candidatos, a contagem de unidades em voo por nó, o nó ao qual cada requisição está reservada, e a banda de histerese. O store mantém lock, fila e despacho, e passa duas consultas (`in_flight`, `held`), então a decisão atômica não mudou. `fits` (incluindo `min_cpu_threads`) já vivia em `placement.rs`. Sem trait de política: há uma só. Coberto por testes unitários de `decide` (`decide_lets_the_cheapest_node_take_the_unit_and_makes_a_worse_one_yield`, `decide_ignores_busy_nodes_as_candidates`, `decide_gives_a_held_unit_only_to_the_held_node`); os de `work.rs` e `phase-g-placement`/`phase-g-reservations` passam sem alteração.

## Semântica do filtro

O requisito é **duro**, não preferência. Uma unidade que precisa de mais threads ou memória do que um nó tem não é entregue a ele, mesmo que a unidade espere. Um nó que não comporta a unidade recebe a próxima que *pode* rodar, ou nada.

O modelo de dispatch é pull (workers fazem poll em `RequestWork`), então o placement é um filtro na fila, não uma escolha do controller sobre o nó. A primeira unidade pronta, com dependências satisfeitas e que cabe no solicitante, é a que sai.

- Unidade cujo requisito nenhum nó sobrevivente atende fica pendente, sem erro. É o mesmo caso de "o cluster não consegue rodar isto".
- O filtro roda por poll: um nó fraco em laço apertado não pega unidade pesada.

## Evidência — threads

`bash code/tests/phase-2-placement.sh`. Um nó fraco (2 threads) sobe sozinho; uma unidade exigindo 4 threads é enfileirada; nada termina; então um nó forte (8 threads) entra e pega a unidade.

```text
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

`w-0 node-strong` é a linha que importa: o nó fraco era o único presente e nunca rodou a unidade. Por ordem de chegada, ela teria ido ao nó fraco no primeiro poll.

Cobertura unitária: `a_weak_node_is_skipped_for_a_unit_that_requires_more_threads`.

## Evidência — memória

`bash code/tests/phase-g-memory.sh`. Um nó com 1.000.000 bytes sobe sozinho; uma unidade 256x256 (footprint de 1.572.864 bytes) é enfileirada; nada termina; então um nó com 2.000.000.000 bytes entra e pega a unidade.

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

`w-0 node-large`: o nó pequeno fazia poll e nunca pegou a unidade, porque o footprint não cabia nos bytes reportados. As threads são iguais (4): o discriminante aqui é só a memória.

Cobertura unitária: `a_node_that_cannot_hold_the_unit_is_skipped` (limite exato: 95 bytes não cabe, 96 cabe), `footprint_is_the_three_matrices_in_bytes`, `a_unit_that_does_not_fit_the_reported_memory_is_excluded`. `unreported_memory_does_not_filter_a_unit_out` fixa a convenção de que `memory_bytes = 0` é "não reportado", não "sem RAM".

## Modelo de custo (G2)

`code/crates/controller/src/placement.rs` estima o custo de rodar uma unidade num nó antes do dispatch. É função pura: mesmas entradas, mesma pontuação, então a ordenação é testável sem cluster.

Custo = compute + transferência + fila, tudo escalado por saúde. Menor é melhor.

- **compute**: flops da unidade (2 por elemento de saída) sobre a taxa do nó (`threads × compute_score`). O `compute_score` vem do benchmark de capability ([[f02-benchmark-de-capability]]), a taxa por core medida no próprio kernel da unidade. Nó não medido cai para 1.0 por core, o fallback de contagem de threads de sempre; ligar o benchmark não reordena um cluster onde ninguém mediu.
- **transferência**: bytes movidos (A e B lidos, C escrito) sobre a taxa de rede do nó. Cai para 1.0 quando não medida.
- **fila**: peso por unidade já em voo para o nó. Impede que um nó ocupado, nominalmente mais rápido, vença sempre.
- **saúde**: multiplica o total. Nó degradado perde empates contra saudável de igual capability. `health_score = 0` é "não reportado" e é ignorado, não lido como péssimo.

A pontuação é relativa, não um tempo. Pesos e taxa de rede continuam placeholders; só a *ordem* entre nós é significativa. Testes unitários cobrem as direções: mais threads custa menos, unidade maior custa mais, fila custa mais, saúde degradada custa mais, saúde não reportada não penaliza.

## Scheduler global (G3) e histerese (G5)

`WorkStore::claim_placed` (`code/crates/controller/src/work.rs`) usa `placement::decide` para escolher quem recebe a unidade.

Regra: o nó que pergunta leva a unidade quando é o **candidato ocioso mais barato** para ela, ou **quase** isso. Se outro nó ocioso tem custo menor além da banda, o nó que pergunta não recebe nada e volta a fazer poll; a unidade fica na fila e o nó que encaixa melhor a pega.

- **Só candidatos ociosos entram no ranking.** Nó com unidade em voo não está fazendo poll, não pode pegar a unidade agora e não deve segurá-la. Nó ocupado porém mais rápido não bloqueia nó ocioso mais fraco.
- **Histerese (G5): o nó só cede quando o outro é mais barato por mais que a banda.** Ceder não é de graça: a unidade espera até um intervalo de poll para o outro nó perguntar. Recusar ceder por diferença de arredondamento troca ociosidade certa por economia incerta. A banda também substitui o antigo desempate por id: nós quase iguais não se recusam, então a unidade não fica presa entre eles. Banda zero significa "estritamente mais barato", a política anterior à G5.

`AI_CLUSTER_PLACEMENT_TOLERANCE` (padrão `0.10`) ajusta a banda; valor inválido é normalizado por `clamp_tolerance` (uma banda negativa faria todo candidato ceder). `AI_CLUSTER_PLACEMENT=off` desliga o scheduler e volta ao primeiro-que-pergunta, para comparar as duas políticas na mesma carga sem recompilar.

Testes de histerese: `a_near_equal_node_takes_the_unit_instead_of_yielding`, `a_clearly_worse_node_still_yields_with_the_band_on`, `a_zero_band_means_strictly_cheapest`, `a_wide_band_lets_a_worse_node_take_the_unit` e `clamps_the_placement_tolerance_band`. Testes de ranking: `placement_gives_a_unit_to_the_cheapest_idle_node`, `a_busy_cheaper_node_does_not_hold_up_an_idle_node`, `the_only_idle_candidate_still_takes_work`, além de `placement_prefers_the_cheaper_idle_node` no serviço.

## Evidência — ranking

`bash code/tests/phase-g-placement.sh`. Perna A: só o nó fraco (2 threads) presente e `AI_CLUSTER_PLACEMENT=off`; a unidade roda nele. Perna B: nó fraco e nó forte (8 threads) presentes e ociosos, placement ligado; o fraco cede e a unidade roda no forte. O nó fraco leva 2000ms de atraso (`AI_CLUSTER_WORK_DELAY_MS`) para representar uma máquina lenta, já que os dois rodam no mesmo host.

```text
--- A) baseline primeiro-a-chegar: apenas o nó fraco (2 threads, 2000ms de atraso) ---
w-0        node-weak              7043      2012957      2020000
--- B) placement: nó fraco + nó forte (8 threads), ambos ociosos ---
w-0        node-strong            7098        10902        18000
```

Mesma unidade: sob primeiro-que-pergunta, 2.020.000us no nó fraco; sob placement, 18.000us no nó capaz.

> [!info] Baseline da medição
> A comparação de ranking mede contra primeiro-que-pergunta (o despacho sem decisão), não contra round-robin literal. O critério do EP §8 diz "melhor que round-robin"; a evidência atual cobre o despacho sem decisão.

## Limites

> [!warning] Precisão, aceleradores e armazenamento não filtram
> O worker reporta esses campos e o registry os guarda, mas nenhuma unidade pode exigi-los. `placement::fits` é o lugar onde a próxima dimensão entra.

> [!warning] Trust sem estado próprio
> A filtragem de trust hoje é exclusão por estado: nó `QUARANTINED`, `FAILED` ou `OFFLINE` não é candidato (`registry::eligible`). Os níveis de trust (`UNKNOWN` → `TRUSTED`) dependem do fluxo de enrollment de [[seguranca]], ainda não existe. Implementá-los antes deixaria todo nó inelegível. Fechar quando o provisionamento existir.

> [!warning] Unidade sem nó capaz fica pendente
> Sem erro, sem fail-fast: o cliente faz poll até o próprio deadline. O placement só vê o nó que pergunta, não o cluster inteiro. Fechar isso exige resolver requisitos contra o registry no enfileiramento, ou quando o último nó capaz sai.

> [!warning] Modelo de custo sem locality nem térmica
> Localidade de dados, folga térmica e custo de mover estado não são modelados porque o registry ainda não carrega esses dados.

> [!warning] Nó ocioso que para de fazer poll segura unidade
> O controller infere "ocioso" de não ter unidade em voo. Um nó parado continua mandando heartbeat, então nunca é marcado offline. A janela é curta (nó ocioso faz poll a cada 200ms), mas um nó travado de verdade não é distinguido. Revisitar com idade do último poll ou contador de recusas.

> [!warning] Só a primeira unidade elegível é avaliada
> O ranking decide *quem* pega a unidade, não *qual* das unidades prontas encaixa melhor no nó que pergunta. Com unidades de tamanhos diferentes, escolher a de melhor encaixe pode adiantar mais; é a G4 ([[f04-scheduler-local]]).

## Próximos passos

- Filtrar por precisão, aceleradores e armazenamento em `placement::fits`.
- Resolver requisitos contra o registry no enfileiramento, para detectar unidade sem nó capaz.
- Modelar locality e térmica quando o registry carregar esses dados.
- Revisitar nó ocioso que parou de fazer poll.
