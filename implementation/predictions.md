---
title: "Previsto vs real — erro do modelo de custo"
type: implementation
status: implemented
tags: [implementation, scheduler, observability, prediction]
---

# Previsto vs real — erro do modelo de custo

O scheduler do G2 escolhe o nó por um custo previsto. Esse custo decide o placement, mas
até aqui ninguém comparava a previsão com o que a unidade de fato custou. Um modelo que
rankeia certo pode ainda assim errar o valor por uma ordem de grandeza — e enquanto o erro
não é medido, ele não é visto. Isto é o H5 da fase de Observability: expor o erro de
previsão, para o modelo de custo ter como ser calibrado em vez de ajustado no escuro.

## O que mudou

- `predicted_compute_us` no `TraceEntry` (`code/proto/cluster.proto`): o tempo de compute
  que o modelo previu para a unidade, no nó que a recebeu.
- `placement::predicted_compute_us` (`code/crates/controller/src/placement.rs`): a função
  que vira a previsão de tempo. Zero quando o nó não reportou compute score.
- `REFERENCE_CORE_GFLOPS` mudou de lugar: vivia só no worker (`worker/src/main.rs`), agora
  fica em `ai_cluster_common::matrix`, ao lado de `flops`. É o mesmo número dos dois lados —
  o worker normaliza o rate medido por ele, e o controlador **inverte** essa normalização
  para voltar a tempo. Se cada lado tivesse a sua cópia, uma mudança num lado só escalaria
  toda previsão errado, sem sintoma visível até alguém comparar.
- A previsão é capturada no despacho (`Dispatched`) e sobrevive até a conclusão
  (`CompletedWork`), porque a capability do nó só está em mãos na hora do claim, não quando
  o resultado volta.
- `ai-cluster request trace <id>`: coluna `PREDITO_US` por unidade e uma linha `PREVISTO`
  com o somado e o erro percentual.

## Como ler

Saída real de `bash code/tests/phase-h-traces.sh` (nó sem score forçado):

```
REQUISIÇÃO req-0 UNIDADES 2
TRABALHO   NÓ             ESTADO        COMPUTE_US   PREDITO_US  TRANSFER_US     TOTAL_US
w-0        node-t         completed            583           18         1417         2000
w-1        node-t         completed            616           18          384         1000
unidades=2 compute_somado_us=1199 transfer_somado_us=1801 latencia_somada_us=3000
PREVISTO somado_us=36 erro_pct=3230.6
```

- **`erro_pct` é a distância do real ao previsto**, positivo quando o real é maior (o modelo
  subestimou). Aqui 3230.6% é a leitura honesta de "o modelo prevê ~18us e a máquina gasta
  ~600us": o modelo de custo sempre foi uma **ordenação**, e a previsão em µs expõe o
  tamanho real da distância entre a ordenação e o relógio. Trinta e poucas vezes é o erro
  medido nesta máquina para unidades pequenas (64³), e é o teto de escalonamento linear
  abaixo se manifestando — não uma surpresa.
- **Zero em `PREDITO_US` é "não previsto", não "instantâneo".** Um nó sem compute score
  (benchmark desligado ou falho) reporta zero, e uma unidade não concluída também. A linha
  `PREVISTO` só aparece quando há pelo menos uma previsão na requisição.

## Por que dá para prever agora

O termo de compute do modelo é `flops / compute_rate`, em unidades arbitrárias — mas a
normalização do worker dá sentido físico a elas. O worker mede GFLOP/s por core e reporta
`compute_score = medido / REFERENCE_CORE_GFLOPS`. Então `compute_rate * REFERENCE_CORE_GFLOPS`
é a vazão do nó em GFLOP/s, e 1 GFLOP/s é 1e3 flops por microssegundo:

```
previsto_us = flops / (compute_rate * REFERENCE_CORE_GFLOPS * 1e3)
```

Com `flops = 2 * rows * shared * cols`, o mesmo cálculo que o worker usa no benchmark.

## Teto conhecido

- **Compute só.** A previsão cobre o tempo de compute, não a transferência nem a espera na
  fila — que o modelo de custo também pondera, mas que não têm uma medição simétrica do lado
  do worker. O `erro_pct` compara compute com compute de propósito.
- **Assume compute linear em threads.** É a mesma suposição do benchmark do worker, e é a
  origem do erro grande medido: o score é calibrado **single-core** num kernel de 256³ (33.5
  MFLOP), onde o custo por chamada se dilui; uma unidade de 64³ é pequena o bastante para
  clone de operandos e criação de threads dominarem. O modelo então extrapola 32 threads como
  32× um core e prevê 18us onde a máquina gasta ~600us. Turbo e SMT contribuem, mas o efeito
  principal é o tamanho do kernel de calibração. É exatamente o erro que esta fatia existe
  para tornar visível; calibrar por faixa de tamanho é a próxima fatia, não esta.
- **Não persiste.** A previsão vive no dispatch, que não vai para o snapshot — depois de um
  restart ela é recalculada no próximo despacho. Coerente com as unidades concluídas, que
  também não sobrevivem a um restart.
- **O roll-up do cluster ainda não agrega previsto.** `GetMetrics` (H1) soma compute e
  latência, não a previsão; o `dashboard` (H4) por isso ainda não mostra "expected vs actual".
  Somar a previsão no `MetricsResponse` é a próxima fatia, não esta.

## Evidência

E2E: `bash code/tests/phase-h-predictions.sh` — com o nó reportando um score fixo
(`AI_CLUSTER_COMPUTE_SCORE=1.0`) em 4 threads a previsão vira aritmética conhecida e o script
a pina: `PREVISTO somado_us=44` para duas unidades de 64³ (22us cada) e `175` para uma de 128³,
com `erro_pct > 0`. É a mesma execução que valida a leitura de "não previsto" numa requisição
desconhecida. `bash code/tests/phase-h-traces.sh` mostra a coluna `PREDITO_US` no trace de uma
requisição real.

Testes unitários: em `placement.rs`, `the_prediction_matches_the_reference_rate` (pina a
cadeia inteira: 64³ na taxa de referência = 87us), `more_threads_predicts_less_time`,
`a_bigger_unit_predicts_more_time` e `an_unmeasured_node_predicts_nothing`; em `work.rs`,
`a_predicted_compute_time_flows_from_claim_to_trace` e
`an_unmeasured_node_leaves_the_prediction_unset_in_the_trace`.

O caminho "nó sem score" é coberto por teste unitário e não por E2E: o worker não tem
ambiente para forçar score zero — `AI_CLUSTER_COMPUTE_SCORE` só aceita valor positivo e
qualquer outra coisa cai no benchmark real.
