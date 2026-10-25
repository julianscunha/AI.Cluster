---
title: "F03 — Varredura compute vs transferência"
aliases: ["phase-0/benchmark-sweep", "EP §20"]
tags: [implementacao/fase/03, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-03-execucao-distribuida]]"
codigos-antigos: ["EP §20", "roadmap-0"]
sistema: ["[[modelo-de-desempenho]]", "[[camada-de-transporte]]"]
evidencia: ["code/tests/scaling-sweep.sh"]
---
# F03 — Varredura compute vs transferência

- [ ] Superado

## Objetivo

Primeiro experimento do plano (EP §20): a partir de que tamanho de unidade distribuir uma operação de matriz compensa o custo de movê-la? O compute cresce com o cubo da aresta; a transferência, com o quadrado. Em algum ponto entre eles está o tamanho abaixo do qual nós extras não se pagam. É o critério de saída da fase 0 original: "dois nós Linux trocam uma Work Unit e comparam compute vs transfer".

## O que foi feito

Harness `code/tests/scaling-sweep.sh`: roda a mesma carga em vários tamanhos com 1, 2 e 3 workers. Reproduzir com:

```bash
NODES="1 2 3" SIZES="32 64 128 256" REPS=6 bash code/tests/scaling-sweep.sh
```

## Achado 1 — a cadência de poll era o gargalo

A primeira execução mostrou um tempo de parede fixo de ~36s para seis unidades em um worker, independente do tamanho. Seis unidades, uma por intervalo de heartbeat, são seis vezes cinco segundos: o laço de trabalho do worker compartilhava o ticker do heartbeat, então pedia trabalho uma vez a cada 5s, por mais rápido que fosse o compute. O compute era microssegundos; a espera era segundos.

O poll de trabalho agora roda na própria cadência: imediatamente após terminar uma unidade, e a cada `WORK_POLL_IDLE_MS` (200ms) quando não achou nada.

| unidades | workers | antes | depois |
|-------|---------|--------|-------|
| 6 | 1 | 54s | 9s |
| 6 | 2 | 36s | 8s |
| 6 | 3 | 19s | 9s |

A coluna "antes" caía com o número de workers só porque mais workers fazem poll com mais frequência no agregado. Não é speedup paralelo; é a mesma espera amostrada de mais lugares. Esta é a armadilha que a varredura foi construída para expor, e por isso ela roda antes de qualquer trabalho de scheduler: um número de throughput medido num laço de poll faminto não diz nada sobre placement.

Depois da correção, o tempo de parede é aproximadamente constante (~8s) entre contagens de nós e tamanhos, dominado por overhead fixo (start do container mais o poll de relatório de 2s do CLI), não pelo cluster.

## Achado 2 — dentro do teto do transporte, a transferência domina o compute

No tamanho 256, seis unidades em um worker: compute 6–15,5ms, transferência 8,6–17,6ms. A transferência ainda empata ou supera o compute. Em tamanhos menores o compute é sub-milissegundo e a transferência lê no piso de resolução, então 256 é o maior tamanho onde a comparação é significativa.

Existe um tamanho onde o compute supera a transferência, mas ele está acima de 256. Uma unidade 512x512 carrega duas matrizes f64, ~4MB, que é o teto de mensagem padrão do gRPC. O cruzamento fica logo depois da maior unidade que o transporte atual consegue carregar; alcançá-lo exige limite de mensagem maior ou outro transporte. Leitura honesta: em todo tamanho que este cluster consegue despachar, mover a unidade custa mais do que computá-la.

## Ressalvas de medição

- `total_latency_us` deriva de timestamps em milissegundos: total e transferência têm resolução de ~1ms. Ok em 256, inútil abaixo de 128. Linhas `transfer=0` são unidades que completaram no mesmo milissegundo do dispatch, não unidades com transferência zero.
- O relatório por unidade é cumulativo entre execuções: o `avg_*` de uma execução inclui todo tamanho anterior. Ler as linhas por unidade, não as médias.
- `transfer_time_us` é `total_latency - compute_time`: todo custo não-compute na janela de dispatch até conclusão (round trip, serialização, jitter de escalonamento). É limite superior da transferência de rede, não medição dela.

## Limites

> [!warning] O que isto não responde
> Se o cluster supera um nó único em escala. O tempo de parede é limitado por overhead em seis unidades, então a varredura não separa compute de custo de escalonamento. Mais unidades por execução, ou unidades maiores quando o teto do transporte se mover, resolveriam.

## Próximos passos

- Mais unidades por execução, ou unidades maiores após o teto de mensagem ser elevado ou o transporte mudar ([[fase-09-transporte]]).
