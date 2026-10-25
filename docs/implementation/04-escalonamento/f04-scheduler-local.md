---
title: "F04 — Scheduler local"
aliases: ["phase-2/local-scheduler", "G4"]
tags: [implementacao/fase/04, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-04-escalonamento]]"
codigos-antigos: ["G4"]
sistema: ["[[scheduler]]", "[[no-e-hardware]]"]
evidencia: ["code/tests/phase-g-cores.sh", "code/tests/phase-g-priority.sh", "code/tests/phase-g-slots.sh"]
---
# F04 — Scheduler local

- [ ] Superado

## Objetivo

O scheduler global ([[f04-placement]]) decide **qual nó** recebe uma unidade. O scheduler local decide **quanto da máquina** a unidade recebe. Esta fatia implementa a primeira decisão local (quantos cores a unidade pode usar) e faz o worker realmente usá-los.

## O que foi feito

- `worker::core_budget` e `grant_cores`: o worker decide quantos cores concede a uma unidade. É `min(threads reportadas, cores do host)`, nunca zero. As threads reportadas são o teto (um nó que anuncia 2 threads não roda 8 em silêncio), e o paralelismo real do host também limita (um nó que anuncia mais threads do que tem não termina mais lento que serial).
- `Matrix::multiply_with_threads`: o produto é dividido por linhas de saída entre até N threads (`std::thread::scope`, sem dependência nova). `Matrix::multiply` continua sendo o caminho serial e delega a este com 1 thread, então os dois não divergem.
- O worker executa a unidade na concessão e registra quantos cores recebeu (`compute_done threads=N`).

## Por que importa

Antes, o worker anunciava N threads no `CapabilityReport` e computava com **uma só**: o `cpu_threads` que o placement usa era decorativo. Agora o número que o controller usa para decidir é o número que o worker usa para executar. Sem isso, o modelo de custo rankeia nós por uma capacidade que não existe.

## O resultado não muda

A divisão é por linhas e cada linha continua sendo acumulada na mesma ordem i-k-j por uma única thread. O produto é bit a bit igual ao serial, então o checksum entre nós (a verificação de corretude do cluster) não muda. Só o tempo muda.

Cobertura: `threading_the_product_does_not_change_it` (compara com 2, 3, 4, 8 e 64 threads contra o serial, com um último chunk parcial), `zero_threads_means_one_thread` e `threaded_path_still_rejects_mismatched_shapes`.

## Teto de mensagem gRPC

Esta fatia exigiu elevar o teto de mensagem do gRPC. Uma unidade viaja como uma mensagem com operandos em `repeated double`: uma unidade 512x512 são duas matrizes f64, 4.194.304 bytes, acima do padrão de 4MB do gRPC. O cliente do worker recusava a decodificar, o processo morria, e o controller reenfileirava a unidade, que nunca completava. `metadata::MAX_MESSAGE_BYTES` (64MB) é aplicado no cliente (`connect_controller`) e no servidor do controller.

Isto não é o filtro de placement: se a unidade **deve** ir para um nó é `placement::fits`. O teto de mensagem só impede que o transporte rejeite uma unidade que o placement já permitiu.

## Evidência

`bash code/tests/phase-g-cores.sh`. A mesma unidade 512x512, um worker por vez para não disputar CPU.

```text
--- fase narrow: o nó reporta 1 threads ---
trabalho w-0 shape=512x512x512 compute_done threads=1 aceito=true
w-0        node-narrow          226607        64393       291000
--- fase wide: o nó reporta 8 threads ---
trabalho w-0 shape=512x512x512 compute_done threads=4 aceito=true
w-0        node-wide             32497        70503       103000
OK: checksum idêntico com 1 e com 8 threads reportadas
OK: compute caiu de 226607us para 32497us ao conceder mais cores
```

`threads=1` e `threads=4` são a concessão: o nó largo anuncia 8 threads e o host tem 4, então recebe 4. O compute cai ~7x; o checksum (`-2641.789268`) é o mesmo nas duas fases.

> [!warning] Host ocioso
> A comparação só vale com o host ocioso: é tempo de parede, e carga concorrente tira cores justamente do lado que depende de paralelismo.

## Prioridade na fila (G4)

A decisão de **qual unidade sai primeiro** vive na fila do controller, não no worker (que roda uma unidade por vez). `WorkUnit.priority` (campo 12; também em `EnqueueWorkRequest` e `EnqueueGraphRequest`; CLI `benchmark --priority N`) é um inteiro: maior despacha antes, 0 é a classe padrão.

- `work.rs::best_position` escolhe, entre as unidades prontas, que cabem no nó e que a reserva permite, a de maior `priority * 64 + passed`, com FIFO no empate. `claim` e `claim_placed` usam o mesmo ranking; todas com prioridade 0 reproduzem a ordem FIFO anterior.
- Envelhecimento: `passed` conta quantos despachos passaram por cima da unidade enquanto ela esperava (`take_locked`). Uma classe vale 64 passagens, então uma unidade baixa ultrapassa um fluxo contínuo de altas depois de ~64 despachos. Não persiste: um restart zera a idade, e uma unidade devolvida por queda de nó volta com idade zero. A classe é limitada a 9 no enfileiramento (`MAX_PRIORITY`), para o envelhecimento sempre alcançar.
- Sem bloqueio de cabeça de fila: `claim_placed` percorre as unidades por ranking e leva a primeira que o `decide()` aceita para este nó, então uma unidade alta que espera por um nó reservado não segura as baixas (`a_high_priority_unit_that_yields_does_not_block_a_lower_one`).
- Teste: `a_higher_priority_unit_is_claimed_before_an_older_lower_one`, `equal_priority_stays_fifo`, `aging_lets_an_old_low_priority_unit_overtake_a_stream_of_high_ones`; E2E `code/tests/phase-g-priority.sh` (w-4, prioridade 5 enfileirada por último, completa primeiro).

> [!todo] Classes nomeadas e autorização
> É um número cru, não as classes `REALTIME`..`BACKGROUND`. Quem pode pedir prioridade alta é decisão da Request Manager ([[fase-11-request-manager]]); hoje só o papel de operador enfileira.

## Fila local por recurso (slots)

O worker roda até `AI_CLUSTER_WORK_SLOTS` unidades ao mesmo tempo (padrão 1, teto 64). Um semáforo é o recurso: o worker só pede trabalho quando há slot livre, segura o slot até o resultado ser entregue, e cada unidade recebe `core_budget / slots` núcleos (no mínimo 1). O nó anuncia `work_slots` no `CapabilityReport`; o controller o conta como ocupado só quando todos os slots têm unidade (`decide`), em vez de ao primeiro despacho. Remedições de compute, storage e rede só rodam com o nó vazio, para não roubar núcleos de uma unidade em voo. Na saída planejada o worker espera os slots devolvidos antes de avisar o controller.

Evidência: `phase-g-slots.sh` — o mesmo lote de 8 unidades (cada uma segurada 600 ms) leva 6016 ms com 1 slot e 2014 ms com 4. Unitário: `a_node_with_free_slots_is_still_a_candidate`.

> [!warning] Slots dividem os núcleos
> Com unidades que já ocupam o nó inteiro, mais slots só reduzem o paralelismo de cada uma. O ganho aparece quando a unidade é curta frente à transferência e ao poll (esconde latência).

## Limites

> [!todo] Filas de recursos e prioridades
> O worker roda uma unidade por vez, então a concessão é sempre o orçamento inteiro do nó. A prioridade vive na fila do controller e os slots limitam a concorrência, mas não há fila local ordenada por classe nem admissão por memória (classes CRITICAL..CACHE: [[worker-runtime]]). Unidade que chega enquanto outra roda espera no controller, não numa fila local.

> [!todo] GPU e NPU
> Só CPU. GPU/NPU não existem no runtime; começar por CPU x86_64/ARM64. Buffers e memória local não são decididos aqui.

> [!warning] A concessão não mede desempenho
> O número de cores vem de `available_parallelism`, não de benchmark; o `compute_score` do modelo de custo continua caindo para a contagem de threads. Medir é a fase de benchmark de capability ([[fase-02-capability-e-saude]]).

## Próximos passos

- Fila local ordenada por classe e admissão por memória; classes nomeadas de prioridade.
- Concessão de cores baseada em benchmark, não só em `available_parallelism`.
- Dispositivos de aceleração (GPU/NPU), em [[fase-16-aceleradores]].

## Falhas com slots

Se o resultado de uma unidade não chega ao controller (3 tentativas) ou o cálculo entra em pânico, o worker marca-se quebrado e sai: `leave_node` devolve a unidade em voo a outro nó, em vez de ela ficar presa ocupando um slot. Um erro em `request_work` também sai pelo caminho limpo. A espera pelos slots na saída tem teto de 30 s. No custo do placement, a taxa de compute e a fila são divididas por `work_slots` (cada unidade vê só a fatia dos núcleos).
