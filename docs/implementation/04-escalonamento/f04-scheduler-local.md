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
evidencia: ["code/tests/phase-g-cores.sh"]
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

## Limites

> [!todo] Filas de recursos e prioridades
> O worker roda uma unidade por vez, então a concessão é sempre o orçamento inteiro do nó. Não há fila por recurso, prioridade (prioridades CRITICAL..CACHE, ver [[worker-runtime]]) nem admissão local. Unidade que chega enquanto outra roda espera no controller, não numa fila local.

> [!todo] GPU e NPU
> Só CPU. GPU/NPU não existem no runtime; começar por CPU x86_64/ARM64. Buffers e memória local não são decididos aqui.

> [!warning] A concessão não mede desempenho
> O número de cores vem de `available_parallelism`, não de benchmark; o `compute_score` do modelo de custo continua caindo para a contagem de threads. Medir é a fase de benchmark de capability ([[fase-02-capability-e-saude]]).

## Próximos passos

- Filas de recursos e prioridades locais.
- Concessão de cores baseada em benchmark, não só em `available_parallelism`.
- Dispositivos de aceleração (GPU/NPU), em [[fase-16-aceleradores]].
