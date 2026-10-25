---
title: "Varredura de benchmark — compute vs transfer"
type: implementation
status: implemented
tags: [implementation, phase-0, benchmark, scaling]
---

# Varredura de benchmark — compute vs transferência

O primeiro experimento do plano (§20): a partir de que tamanho de unidade distribuir uma
operação de matriz compensa o custo de movê-la? O compute cresce com o cubo da aresta;
a transferência cresce com o quadrado. Em algum ponto entre eles está o tamanho abaixo do
qual nós extras não se pagam. Este é o critério de saída da Fase 0 — "dois nós Linux
trocam uma Work Unit e comparam compute vs transfer".

Harness: `code/tests/scaling-sweep.sh`. Roda a mesma carga em vários tamanhos com
1, 2 e 3 workers. Reproduza com
`NODES="1 2 3" SIZES="32 64 128 256" REPS=6 bash code/tests/scaling-sweep.sh`.

## Achado 1 — a cadência de poll era o gargalo, não o compute nem a transferência

A primeira execução mostrou um tempo de parede fixo de ~36s para seis unidades em um worker,
independente do tamanho. Seis unidades, uma por intervalo de heartbeat, são seis vezes cinco
segundos: o laço de trabalho do worker compartilhava o ticker do heartbeat, então ele pedia
trabalho uma vez a cada 5s por mais rápido que fosse o compute. O compute era microssegundos;
a espera era segundos.

O poll de trabalho agora roda na sua própria cadência — imediatamente após terminar uma
unidade, e a cada `WORK_POLL_IDLE_MS` (200ms) quando não achou nada.

| unidades | workers | antes | depois |
|-------|---------|--------|-------|
| 6 | 1 | 54s | 9s |
| 6 | 2 | 36s | 8s |
| 6 | 3 | 19s | 9s |

A coluna "antes" caía com o número de workers só porque mais workers fazem poll com mais
frequência no agregado. Isso não é speedup paralelo; é a mesma espera amostrada de mais
lugares. Esta é a armadilha que a varredura foi construída para expor, e é por isso que a
varredura roda antes de qualquer trabalho de scheduler: um número de throughput medido num
laço de poll faminto não diz nada sobre placement.

Depois da correção o tempo de parede é aproximadamente constante (~8s) entre contagens de
nós e tamanhos, então agora é dominado por overhead fixo — start do container mais o poll de
relatório de 2s do CLI —, não pelo cluster.

## Achado 2 — dentro do teto do transporte, a transferência domina o compute

No tamanho 256, seis unidades em um worker: compute 6–15,5ms, transferência 8,6–17,6ms. A
transferência ainda empata ou supera o compute. Em tamanhos menores o compute é
sub-milissegundo e a transferência lê no seu piso de resolução, então 256 é o maior tamanho
onde a comparação é significativa.

O compute cresce como o cubo da aresta e a transferência como o quadrado, então existe um
tamanho onde o compute supera a transferência. Esse tamanho está acima de 256. Uma unidade
512x512 carrega duas matrizes f64, ~4MB, que é o teto de mensagem padrão do gRPC. Então o
cruzamento fica logo depois da maior unidade que o transporte atual consegue carregar, e
alcançá-lo exige ou um limite de mensagem maior ou um transporte diferente. Até lá, a
leitura honesta é: em todo tamanho que este cluster consegue de fato despachar, mover a
unidade custa mais do que computá-la.

## Ressalvas de medição

- `total_latency_us` deriva de timestamps em milissegundos, então total e transferência têm
  resolução de ~1ms. Ok no tamanho 256, inútil abaixo de 128, onde o compute é sub-ms. As
  linhas `transfer=0` na saída bruta são unidades que completaram no mesmo milissegundo em
  que foram despachadas, não unidades com transferência zero.
- O relatório por unidade é cumulativo entre execuções, então o `avg_*` de uma execução inclui
  todo tamanho anterior. Leia as linhas por unidade, não as médias da execução.
- `transfer_time_us` é `total_latency - compute_time`, então é todo custo não-compute na
  janela de dispatch até conclusão: round trip, serialização e jitter de escalonamento. É um
  limite superior da transferência de rede, não uma medição dela.

## O que isto não responde

Se o cluster supera um nó único em escala. O tempo de parede agora é limitado por overhead em
seis unidades, então a varredura não consegue separar compute de custo de escalonamento. Mais
unidades por execução, ou unidades maiores assim que o teto do transporte se mover, resolveriam.
