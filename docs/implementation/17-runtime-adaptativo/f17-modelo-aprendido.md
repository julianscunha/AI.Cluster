---
title: "F17 — Modelo de desempenho aprendido"
aliases: ["P1", "calibração por nó"]
tags: [implementacao/fase/17, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-17-runtime-adaptativo]]"
codigos-antigos: ["P1"]
sistema: ["[[modelo-de-desempenho]]", "[[scheduler]]"]
evidencia: ["code/tests/phase-q-learning.sh"]
---
# F17 — Modelo de desempenho aprendido

- [ ] Superado

## Objetivo

A previsão de tempo de compute de uma unidade vinha só do benchmark de registro e da taxa típica do nó ([[f05-previsto-vs-real]]). Se o nó entrega menos (ou mais) do que anunciou, o erro se repete em toda unidade. Esta fatia (P1) faz o controller aprender, por nó, a razão entre o real e o previsto, e corrigir as previsões seguintes.

## O que foi feito

- `WorkStore` guarda, por nó, uma razão `real / previsto-pelo-modelo` (`learned`). A primeira unidade concluída a define; as seguintes a movem 30% do caminho em direção ao que foi observado (`LEARNING_RATE`). Razões fora de 0,05 a 20 são tratadas como travada ou erro de relógio e cortadas.
- No despacho, a previsão do modelo de custo (`raw`) é multiplicada pela razão do nó. A unidade carrega os dois valores até a conclusão: o corrigido aparece no trace e na métrica, o bruto alimenta o aprendizado, para a razão não aprender sobre a própria correção.
- Unidade sem previsão (nó sem score) ou sem tempo medido não ensina nada.

## Evidência

`bash code/tests/phase-q-learning.sh`: um worker que anuncia compute score 4,0 (fixado, cerca de 4 vezes o que entrega) recebe 40 unidades de 64×64 uma por vez. O erro relativo médio da previsão cai de **33,5** nas 5 primeiras unidades (a primeira, fria, é a pior) para **0,08** nas 15 últimas. Unitários: `the_learned_ratio_moves_toward_what_the_node_really_takes` (a razão chega a 4 quando o nó leva 4 vezes o previsto) e `a_completed_unit_corrects_the_next_prediction_for_that_node`.

## Limites

> [!warning] Só calibra, não prevê
> É uma média móvel por nó, não um modelo que depende do tamanho ou do tipo da unidade: se a razão real muda com o tamanho da matriz, uma razão única erra. Uma amostra fria muito fora (a primeira unidade) pesa na razão até as seguintes a diluírem. Não persiste: um restart reaprende. O **placement** continua usando a taxa do baseline, de propósito, para não oscilar por ruído (ver [[f02-capability-corrente]]); a razão só corrige a previsão de tempo, não decide qual nó recebe a unidade.

## Próximos passos

- P2: usar a previsão corrigida no custo do placement, com guarda contra oscilação.
- Razão por faixa de tamanho de unidade, e mediana em vez de média para tolerar amostras frias.
