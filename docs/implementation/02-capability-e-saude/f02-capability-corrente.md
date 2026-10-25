---
title: "Capability corrente do nó"
type: implementation
status: implemented
tags: [implementation, capability, placement, phase-c]
---

# Capability corrente do nó

O perfil de capability era uma fotografia do registro e só: um `compute_score` medido uma vez,
quando o nó entrou, e nada sobre o que ele fazia depois. Um nó que ficou mais lento ao longo da
vida — vizinho barulhento no host compartilhado, limite de energia — seguia previsto pela
velocidade do primeiro dia. Esta é a fatia C3: *baseline + current + confidence*, como o
`docs/05` pede.

## O que mudou

- `CapabilityReport.current_compute_score` e `compute_confidence` no proto. `compute_score`
  continua sendo o baseline; os dois novos campos carregam a última medida e o quanto confiar
  nela.
- `CapabilityReport capability` no `HealthStreamRequest`. O perfil deixa de ser só do
  registro: o health stream passa a carregá-lo, então o controller vê o nó mudar sem esperar
  um re-registro.
- `worker::measure_compute` devolve `ComputeMeasurement { score, confidence }` em vez de só o
  score. O benchmark já rodava N vezes e descartava tudo menos a melhor — agora o espalhamento
  entre as execuções vira confiança.
- `worker::measurement_confidence`: melhor sobre a mediana. Toda perturbação só torna uma
  execução mais lenta, então a mais rápida é a confiável e a distância até a mediana é
  exatamente quanto de perturbação houve.
- `worker::refresh_current_compute` / `capability_with_current` / `apply_current`: remedem o
  compute no laço de trabalho, quando não há nada para computar, e dobram o resultado no perfil
  que o heartbeat manda.
- `controller::Registry::report_health` aceita o perfil opcional e o guarda. `service::report_health`
  repassa. Nada de re-registro: identidade e instante de registro ficam onde estavam.
- `controller::placement::predicted_compute_us` passa a preferir o current. O ranking do
  placement **continua** usando o baseline.
- `ai-cluster node show <id>` ganhou `current_compute_score` e `compute_confidence`.
- `CapabilityReport.compute_median_score` e `compute_deviation`: a distribuição do benchmark de
  registro — o valor central, para calibrar uma previsão de tempo, e o quanto as execuções
  discordaram. Ver `benchmark-stability.md`; é o número estável que faltava para calibrar.

Sem dependência nova: um `Vec`, um `sort` e uma divisão.

## Semântica

**A remedida roda só com o nó ocioso.** O benchmark de 256³ leva dezenas de milissegundos e
disputa CPU com quem estiver computando. Rodá-lo no tick do heartbeat roubaria core de uma
unidade em voo e, pior, inflaria o `compute_time_us` que o worker reporta — a remedida
corromperia a própria medida que o H5 compara. Então ela mora no ramo ocioso do laço de
trabalho, que é onde a CPU está livre por definição. Um nó que nunca fica ocioso nunca reporta
current, e isso é honesto: não houve janela para medir.

**Ranking no baseline, previsão no current.** São perguntas diferentes. O placement pergunta
"qual é o melhor lugar para esta unidade?", e responder isso com um instantâneo ruidoso
reordenaria candidatos a cada tick. A previsão pergunta "quanto esta unidade vai demorar
agora?", e aí a leitura recente é a resposta certa. Por isso `compute_rate` segue no baseline
e só `predicted_compute_us` lê o current.

**Confiança é concordância, não recência.** `best / median` sobre as execuções da mesma
passada: se todas levaram o mesmo tempo, 1.0; se a mediana ficou muito acima da melhor, houve
perturbação e o número vale menos. Uma execução só não pode discordar de si mesma — vale 1.0.
Nenhuma execução vale 0.0, e uma melhor zero ou não-finita vale 0.0, nunca 1.0.

**Zero é "não remedido", nunca "compute zero".** Mesma convenção do score de health não
reportado e das classes de armazenamento vazias. Quem lê cai no baseline; a CLI imprime `-`.

**O pin manda.** `AI_CLUSTER_COMPUTE_SCORE` fixa o score para um teste não depender da
velocidade do host; remedir contradiria o pin e tornaria não-determinístico um botão de
determinismo. Um nó fixado reporta `current_compute_score=-` e a previsão usa o baseline, que
é exatamente o que o pin pediu.

## Evidência

`bash code/tests/phase-c-current.sh`.

```text
=== A) nó ocioso remede o compute enquanto espera trabalho ===
  threads=4 memoria_bytes=4046016512 compute_score=1.0715
  current_compute_score=0.7222 compute_confidence=0.71
  compute_median_score=0.4612 compute_deviation=0.410
baseline=1.0715 current=0.7222 confidence=0.71 median=0.4612 desvio=0.410
OK: o baseline do registro está preenchido
OK: o current foi remedido com o nó ocioso
OK: a confiança está em (0,1] (veio '0.71')
OK: a mediana do benchmark foi reportada (veio '0.4612')
OK: a mediana (0.4612) não passa do melhor caso (1.0715)
OK: o desvio é não-negativo (veio '0.410')
=== B) score fixado: o nó não remede, então current fica '-' ===
  threads=4 memoria_bytes=4046016512 compute_score=2.0000
  current_compute_score=- compute_confidence=-
  compute_median_score=- compute_deviation=0.000
OK: o baseline fixado foi reportado como veio
OK: o pin impede a remedida, então current é 'não remedido'
OK: sem benchmark, a mediana também é 'não medido'
=== C3 OK ===
```

O passo A não fixa o score, então as leituras são reais e do mesmo host. Que best case (1.07),
mediana (0.46) e remedida (0.72) **difiram** é o ponto — cada um responde uma pergunta
diferente e nenhum é cópia do outro. O desvio de 0.41 diz que aquela passada de registro foi
muito perturbada; a mediana é o número que sobra para calibrar. Os detalhes são de
`benchmark-stability.md`.

Coberto por `confidence_is_full_when_the_runs_agree`, `confidence_falls_as_the_runs_disagree`,
`a_single_run_cannot_disagree_with_itself`, `a_broken_measurement_carries_no_confidence`,
`the_median_sits_between_the_runs_not_at_their_average`,
`the_deviation_is_zero_for_steady_runs_and_grows_with_the_spread` e
`a_remeasured_capability_carries_the_current_numbers_beside_the_baseline` no worker;
`a_current_score_overrides_the_baseline_in_the_prediction`, `a_decayed_node_predicts_more_than_its_baseline_suggests`
e `a_current_score_alone_still_predicts` no placement; e `a_health_report_can_refresh_the_capability`
no registry.

## Teto conhecido

**Confiança de uma passada, não de um histórico.** O `docs/05` pede confiança "derivada da
quantidade/qualidade dos dados históricos". A quantidade é o N de execuções da passada; não há
série temporal guardada, então uma passada limpa vota 1.0 mesmo sendo a primeira. Uma janela
de amostras com desvio-padrão é o degrau seguinte, se o número se mostrar instável na prática.

**A confiança é carregada, não ponderada.** `predicted_compute_us` usa o current com confiança
0.5 e com confiança 1.0 do mesmo jeito. Ponderar a previsão pela confiança — ou recusar um
current de baixa confiança — é uma mudança no modelo de custo e não tem dado para calibrar.

**Só compute.** Current e confiança existem para o compute; memória, storage e rede não têm
medida corrente — a rede é descoberta como velocidade nominal do link (`network-discovery.md`),
que não se refaz como o current do compute se refaz. É o mesmo teto do
`capability-benchmark.md`.

**Medida em ocioso, não sob carga.** O current diz o que o nó faz quando está livre. Um nó com
unidades longas passa o tempo computando e nunca entra no ramo ocioso: reporta só o baseline,
sem current e sem confiança. Carga já é tratada pelo termo de fila do modelo de custo, não por
este campo.

**Uma passada, não um histórico.** `compute_deviation` descreve o espalhamento dentro de uma
única janela de cinco execuções; não há série temporal. Alargar a janela é o degrau seguinte se
o desvio se mostrar grande — ver `benchmark-stability.md`.