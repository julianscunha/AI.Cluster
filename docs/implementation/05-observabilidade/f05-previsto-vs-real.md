---
title: "F05 — Previsto vs real: erro do modelo de custo"
aliases: ["predictions", "H5"]
tags: [implementacao/fase/05, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-05-observabilidade]]"
codigos-antigos: ["H5", "G2"]
sistema: ["[[modelo-de-desempenho]]", "[[observabilidade]]"]
evidencia: ["code/tests/phase-h-predictions.sh", "code/tests/phase-h-traces.sh"]
---
# F05 — Previsto vs real: erro do modelo de custo

- [ ] Superado

## Objetivo

O scheduler ([[f04-placement]], G2) escolhe o nó por um custo previsto. Até aqui ninguém comparava a previsão com o que a unidade custou de fato. Um modelo que rankeia certo pode errar o valor por uma ordem de grandeza, e enquanto o erro não é medido ele não é visto. Expor o erro de previsão, para o modelo ser calibrado em vez de ajustado no escuro.

## O que foi feito

- `predicted_compute_us` no `TraceEntry` (`code/proto/cluster.proto`): o tempo de compute previsto para a unidade, no nó que a recebeu.
- `placement::predicted_compute_us` (`code/crates/controller/src/placement.rs`): converte o custo em previsão de tempo. Zero quando o nó não reportou compute score. Duas correções sobre `flops / rate` puro, ambas vindas do benchmark de registro ([[f02-benchmark-de-capability]]):
  - **taxa típica, não melhor.** `typical_ratio` multiplica a taxa por `compute_median_score / compute_score`, porque o melhor caso é um extremo que a máquina raramente repete. Fica de fora quando o benchmark discordou demais (`compute_deviation` acima de `DEVIATION_TRUST_LIMIT`, 0.25); aí o melhor caso é usado como estava.
  - **setup por unidade somado.** `UNIT_SETUP_US` (600us) é o custo que não escala com o tamanho (clonar operandos, criar threads). `AI_CLUSTER_UNIT_SETUP_US` ajusta sem rebuild, porque é propriedade do host.
- `REFERENCE_CORE_GFLOPS` mudou de lugar: vivia só no worker, agora fica em `ai_cluster_common::matrix`, ao lado de `flops`. O worker normaliza o rate com ele; o controlador **inverte** a normalização para voltar a tempo. Duas cópias escalariam toda previsão errado sem sintoma visível.
- A previsão é capturada no despacho (`Dispatched`) e sobrevive até a conclusão (`CompletedWork`), porque a capability do nó só está em mãos no claim.
- `ai-cluster request trace <id>`: coluna `PREDITO_US` por unidade e linha `PREVISTO` com somado e erro percentual.
- `MetricsResponse.total_predicted_compute_us` e `metrics::summarize`: o roll-up do cluster soma a previsão das unidades concluídas. `ai-cluster metrics`, o exportador (`ai_cluster_predicted_compute_time_us_total`) e a linha `TEMPO ... previsto_total_us=` do dashboard leem o mesmo número.

## Como ler

Saída real de `bash code/tests/phase-h-traces.sh` (nó sem score forçado):

```text
REQUISIÇÃO req-0 UNIDADES 2
TRABALHO   NÓ             ESTADO        COMPUTE_US   PREDITO_US  TRANSFER_US     TOTAL_US
w-0        node-t         completed            598          619         2402         3000
w-1        node-t         completed            593          619         1407         2000
unidades=2 compute_somado_us=1191 transfer_somado_us=3809 latencia_somada_us=5000
PREVISTO somado_us=1238 erro_pct=-3.8
```

- **`erro_pct` é a distância do real ao previsto**: positivo quando o real é maior (o modelo subestimou), negativo quando o modelo passou do relógio. Antes do termo de setup, esta mesma execução dava `erro_pct=3230.6`: o modelo previa ~18us e a máquina gastava ~600us. O modelo de custo sempre foi uma **ordenação**; a previsão em µs expõe a distância entre ordenação e relógio, trinta e poucas vezes nesta máquina para unidades de 64³. Agora é de poucos por cento, e o que sobra é ruído do host.
- **Zero em `PREDITO_US` é "não previsto", não "instantâneo".** Nó sem compute score (benchmark desligado ou falho) reporta zero, e unidade não concluída também. A linha `PREVISTO` só aparece quando há pelo menos uma previsão.

Para conferir a consistência sem o ruído do host, `bash code/tests/phase-h-predictions.sh` fixa o score do nó e pina a aritmética: `PREVISTO somado_us=1244` em duas unidades de 64³ (622us cada) e `775` em uma de 128³.

## Por que dá para prever agora

O termo de compute é `flops / compute_rate`, em unidades arbitrárias, mas a normalização do worker dá sentido físico a elas. O worker mede GFLOP/s por core e reporta `compute_score = medido / REFERENCE_CORE_GFLOPS`. Então `compute_rate * REFERENCE_CORE_GFLOPS` é a vazão do nó em GFLOP/s, e 1 GFLOP/s é 1e3 flops por microssegundo:

```text
previsto_us = setup_us + flops / (compute_rate * typical_ratio * REFERENCE_CORE_GFLOPS * 1e3)
```

Com `flops = 2 * rows * shared * cols`, o mesmo cálculo que o worker usa no benchmark.

## Evidência

E2E `bash code/tests/phase-h-predictions.sh`: com o nó reportando score fixo (`AI_CLUSTER_COMPUTE_SCORE=1.0`, sem benchmark, portanto sem mediana) em 4 threads, a previsão vira aritmética conhecida: `PREVISTO somado_us=1244` para duas unidades de 64³ e `775` para uma de 128³, com `|erro_pct| < 1000`. Um score fixado não tem razão típica/melhor, então é o setup sozinho que entra. `bash code/tests/phase-h-traces.sh` mostra a coluna `PREDITO_US` numa requisição real, onde o erro calibrado aparece: `erro_pct=-3.8` contra `3230.6` antes do setup. O mesmo script fecha lendo `ai-cluster metrics`: `total_predicted_compute_us=2019` (1244 do A + 775 do B).

Testes unitários em `placement.rs`: `the_prediction_matches_the_reference_rate` (64³ na taxa de referência = 87us de compute, mais o setup), `a_trustworthy_median_pulls_the_prediction_longer`, `an_untrustworthy_median_is_ignored`, `a_node_without_a_median_predicts_from_the_best_case`, `the_setup_clamps_to_something_sane`, `more_threads_predicts_less_time`, `a_bigger_unit_predicts_more_time` e `an_unmeasured_node_predicts_nothing`; em `work.rs`, `a_predicted_compute_time_flows_from_claim_to_trace` e `an_unmeasured_node_leaves_the_prediction_unset_in_the_trace`.

O caminho "nó sem score" é coberto só por teste unitário: `AI_CLUSTER_COMPUTE_SCORE` só aceita valor positivo, e qualquer outra coisa cai no benchmark real.

## Limites

> [!warning] Compute só
> A previsão cobre compute, não transferência nem espera na fila. O `erro_pct` compara compute com compute de propósito.

> [!warning] Compute linear em threads
> Mesma suposição do benchmark do worker. Turbo e SMT deixam a previsão um pouco otimista em nós largos.

> [!warning] Setup é constante, não banda
> Os três tamanhos medidos (64³/128³/256³) deram 626/433/313us de setup implícito, espalhados demais por ruído para sustentar mais de um número. Bandas por faixa de tamanho são o degrau seguinte **quando** o desvio do benchmark mostrar que a medição estabilizou ([[f02-estabilidade-do-benchmark]]).

> [!warning] Taxa típica fica dormente em host barulhento
> A guarda (`DEVIATION_TRUST_LIMIT`, 0.25) desliga a razão mediana/melhor quando o benchmark discordou demais. Num host compartilhado o desvio medido (0.41) passa do limite, então o ganho de `-3.8%` vem do **setup**, não da razão. O caminho da razão é coberto por teste unitário (`a_trustworthy_median_pulls_the_prediction_longer`), não por E2E; num nó dedicado o desvio cai abaixo do limite e a razão entra.

> [!warning] Não persiste
> A previsão vive no dispatch, que não vai para o snapshot. Após restart, é recalculada no próximo despacho. Coerente com as unidades concluídas, que também não sobrevivem.

> [!warning] Unidade sem previsão entra como zero no somado
> Nó sem compute score soma ao `total_compute_time_us` real mas não ao `total_predicted_compute_us`; o agregado subestima a previsão quando parte do cluster não tem score.

## Próximos passos

- Bandas de setup por faixa de tamanho, quando o desvio estabilizar.
- Previsão de transferência e de fila, com medição simétrica no worker.
