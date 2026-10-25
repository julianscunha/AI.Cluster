---
title: "Estabilidade do benchmark de compute"
type: implementation
status: implemented
tags: [implementation, capability, benchmark, observability, phase-c]
---

# Estabilidade do benchmark de compute

O H5 mediu o erro do modelo de custo e o número veio grande: ~30× de subestimação em unidades
pequenas. Grande demais para saber de bate-pronto se o problema era o **modelo** ou a
**medição**. O benchmark de compute reportava só o melhor caso — a execução mais sortuda entre
N — e uma previsão calibrada contra um extremo é um ajuste no escuro: não dá para dizer se o
número mudou porque a máquina mudou ou porque aquela passada deu sorte. Esta fatia faz o
benchmark reportar a distribuição inteira: a **mediana** (a execução típica) e o **desvio
relativo** (o quanto as execuções discordaram). É o pré-requisito que o usuário pediu antes de
calibrar por faixa de tamanho — primeiro um número estável, depois as bandas.

## O que mudou

- `CapabilityReport.compute_median_score` e `CapabilityReport.compute_deviation` no proto: a
  mediana do benchmark de registro e o desvio relativo das suas execuções, ao lado de
  `compute_score`.
- `worker::ComputeMeasurement` carrega `median_score` e `deviation` além de `score` e
  `confidence`. O benchmark já rodava N vezes; antes descartava tudo menos a melhor e a
  confiança; agora guarda também o centro e o espalhamento.
- `worker::median_time` e `worker::relative_deviation`: as duas estatísticas, puras e testáveis
  sem rodar o kernel.
- `worker::baseline_measurement` (era `compute_score`, que devolvia só um `f64`): devolve a
  distribuição inteira do registro. Um score fixado por `AI_CLUSTER_COMPUTE_SCORE` continua
  curto-circuitando o benchmark.
- `ai-cluster node show <id>` mostra `compute_median_score` e `compute_deviation`.
- `placement::typical_ratio` e `placement::UNIT_SETUP_US` consomem a distribuição: a previsão de
  tempo usa a mediana como taxa típica — com `compute_deviation` como **guarda** — e soma o setup
  por unidade que um termo de `flops / rate` não vê (`predictions.md`).

Sem dependência nova: um `Vec`, um `sort` e uma divisão.

## Semântica

**Melhor caso é capability; mediana é o que calibrar.** O `compute_score` continua sendo o
melhor caso, porque é ele que **ordena** nós: a execução menos perturbada é a mais reprodutível
e não muda de um poll para o outro. Mas prever tempo com o melhor caso subestima **sempre** —
a máquina raramente repete a execução mais sortuda. A mediana é o valor central e é contra ela
que uma calibração deve ajustar. São as duas perguntas do C3 outra vez: ordenar e prever.

**Desvio é concordância ao redor do centro.** `desvio-padrão / média` dos tempos da passada — o
coeficiente de variação. Zero é uma passada perfeitamente estável; um número alto diz que a
medição foi contaminada e que a mediana ao lado vale menos. É a contraparte do
`compute_confidence` (`best / median`), mas na família do baseline e no eixo do espalhamento,
não da razão entre dois extremos.

**A distribuição é do baseline, não do current.** `compute_median_score` e `compute_deviation`
descrevem o **mesmo benchmark de registro** que produziu `compute_score`, então
`apply_current` não os toca: ele dobra a família *current* (`current_compute_score`,
`compute_confidence`) ao lado. Isso é deliberado — a calibração precisa de um número **estável**,
e um valor que se move a cada heartbeat seria exatamente o que ela não deve perseguir. O que
varia de verdade entre medições já é coberto pela família current.

**Zero é "não medido".** Um nó com o score fixado não roda benchmark nenhum, então reporta
`compute_median_score = 0` e `compute_deviation = 0`; a CLI imprime `-` e quem lê cai no
`compute_score`. Mesma convenção da mediana de zero execuções e do score de health não
reportado. Zero no desvio junto com zero na mediana é "não medido", não "perfeitamente estável".

## Como ler

Saída real de `bash code/tests/phase-c-current.sh` (nó sem score fixado, host compartilhado):

```text
CAPABILITY
  threads=4 memoria_bytes=4046016512 compute_score=1.0715
  current_compute_score=0.7222 compute_confidence=0.71
  compute_median_score=0.4612 compute_deviation=0.410
```

- O **melhor caso** do registro é 1.07 e a **mediana** é 0.46: a execução típica corre a menos
  da metade da mais sortuda. É exatamente o abismo que fazia o H5 prever 18µs onde a máquina
  gastava ~600µs — parte do erro de 30× não era o modelo, era o número que o alimentava.
- O **desvio de 0.41** é a resposta à pergunta que motivou a fatia: a medição **é** instável.
  Quarenta e um por cento de variação relativa numa passada de cinco execuções é o host
  compartilhado se manifestando. Reportar a mediana sem o desvio ao lado esconderia isso.
- `compute_median_score` nunca passa de `compute_score`: mesmo conjunto de execuções, o melhor
  caso é o máximo e a mediana é o centro. O E2E pina esse invariante.

## Teto conhecido

**Uma passada, não um histórico.** As cinco execuções são de um instante. Não há série temporal:
um nó ocioso vota com o que mediu naquela janela. Alargar a janela (mais execuções, mediana
móvel) é o degrau seguinte **se** o desvio se mostrar grande o bastante para valer o custo. A
fatia de calibração decidiu não esperar: com 0.41, uma constante global (taxa típica + setup) já
corta o erro do H5 para poucos por cento, e as bandas por faixa ficam para quando o desvio cair.

**A distribuição já alimenta a previsão.** `predicted_compute_us` passou a usar a mediana como
taxa típica — `typical_ratio = compute_median_score / compute_score` — mais um termo de setup por
unidade, em vez do melhor caso puro (`predictions.md`). O `compute_deviation` virou a **guarda**
dessa razão: acima de `DEVIATION_TRUST_LIMIT` a mediana e o melhor caso discordaram demais para a
razão valer, e a previsão volta ao melhor caso em vez de confiar num número ruidoso. O 0.41 medido
aqui está abaixo do limite, então a razão é usada — mas o limite existe porque um 0.41 de host
compartilhado pode não ser o de um nó real.

**Bandas por faixa de tamanho continuam a próxima fatia.** A taxa típica e o setup são constantes
globais; uma banda por tamanho usaria o mesmo par mediana/desvio para escolher a taxa de cada
faixa. Agora há número estável para mirar.

**Só compute.** Mediana e desvio existem para o compute; memória, storage e rede continuam sem
medida. É o mesmo teto do `capability-benchmark.md`.

**O desvio é do host, não do nó.** Numa máquina dedicada os cinco números seriam quase iguais e
o desvio ficaria perto de zero. O 0.41 medido aqui diz mais sobre o host compartilhado do
ambiente de teste do que sobre um nó real — mas é honesto: é a variação que existe.

## Evidência

`bash code/tests/phase-c-current.sh` — o passo A mostra a distribuição de um nó ocioso e pina
que a mediana existe, que ela não passa do melhor caso e que o desvio é não-negativo; o passo B
prova que um score fixado reporta `compute_median_score=-`, "não medido" e não o pin copiado
para um segundo campo.

Testes unitários: `the_median_sits_between_the_runs_not_at_their_average` (mediana de contagem
ímpar é a execução do meio, não a média; de contagem par é a média das duas do meio; uma
execução é sua própria mediana; nenhuma é "não medido") e
`the_deviation_is_zero_for_steady_runs_and_grows_with_the_spread` (zero quando todas as
execuções levam o mesmo tempo, crescente com o espalhamento, zero com menos de duas execuções)
em `worker/src/main.rs`; `a_remeasured_capability_carries_the_current_numbers_beside_the_baseline`
passa a pinar também que a distribuição do baseline **não** se move quando a família current é
dobrada por cima.
