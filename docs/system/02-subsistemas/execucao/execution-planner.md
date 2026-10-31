---
title: "Planejador de execução"
aliases: ["Execution Planner", "Planejador de execução"]
tags: [sistema/subsistema/execucao]
type: sistema/subsistema
---

# Planejador de execução

Voltar para [[execucao]].

Hoje não existe planejador de plano por requisição. A decisão de onde cada unidade roda é tomada por unidade, no momento em que um worker pede trabalho, pelo [[scheduler]]. Essa decisão usa o custo estimado pelo [[modelo-de-desempenho]].

## O que existe

- A decisão de placement por unidade. Ela soma custo de compute, de transferência, de fila e o fator de saúde, e escolhe o candidato ocioso mais barato, com uma faixa de tolerância de 10% (`AI_CLUSTER_PLACEMENT_TOLERANCE`).
- Um filtro que impede o worker de receber unidade que não cabe nele, por threads ou por memória.

## Limites conhecidos

- Não há perfil de requisição. Toda unidade recebe o mesmo tratamento, sem preferência por latência, vazão ou confiabilidade.
- Os pesos do custo (compute, transferência, fila e saúde) são valores fixos e não calibrados. Só a ordem entre workers tem significado, não o valor absoluto.
- O custo não considera localidade de dados, pressão de memória, risco de falha, risco térmico nem custo de mover estado. O registro não traz esses dados.

> Planejado: planejador que, a partir do modelo, divide as camadas entre os workers (pipeline, tensor ou híbrido), com perfis de requisição, planos versionados, replanejamento em evento (entrada ou saída de worker, falha, mudança de desempenho medida) e histerese para evitar oscilação. Depende do [[request-manager]] e do [[model-manager]].
