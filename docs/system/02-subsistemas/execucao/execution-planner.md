---
title: "Execution Planner"
aliases: ["Planejador de execução", "docs/10", "10 — Execution Planner"]
tags: [sistema/subsistema/execucao, subsistema/execution-planner]
type: sistema/subsistema
status: proposto
implementacao: nao-iniciado
fases: ["[[fase-04-escalonamento]]", "[[fase-12-model-runtime]]"]
---

# Execution Planner

Voltar para [[execucao]].

## Objetivo

Produzir um plano de execução que minimize o custo total sob as restrições do cluster. O plano diz, para o Model Graph de uma requisição, como as operações se dividem entre os nós e em que ordem.

## Entradas

- Model Graph ([[modelo-de-grafo]]) e perfil da requisição.
- Hardware Graph e Network Graph ([[no-e-hardware]], [[camada-de-transporte]]).
- Localidade de estado e de modelo: onde já estão os pesos e o estado da requisição.
- Saúde e confiança dos nós ([[saude-e-confianca]]).
- Modelo de desempenho ([[modelo-de-desempenho]]).

## Estratégias de paralelismo

- Paralelismo de pipeline: camadas em nós diferentes, em sequência.
- Paralelismo de tensor: uma operação dividida entre nós.
- Paralelismo híbrido: combinação das duas, conforme o modelo e o cluster.

## Função de custo

Conceitualmente:

```text
minimize(
  latency + communication + memory_pressure + failure_risk + thermal_risk + rebalancing_cost
)
```

Os pesos dependem do perfil da requisição.

## Perfis

- latência;
- vazão (throughput);
- confiabilidade;
- eficiência;
- equilibrado.

## Princípio de planejamento

Um nó disponível não precisa ser usado. Adicionar hardware pode piorar o plano quando o custo de comunicação supera o ganho de computação.

## Replanejamento

Um novo plano versionado é gerado em eventos relevantes (entrada ou saída de nó, falha, mudança de desempenho medida). A troca de plano usa histerese para evitar oscilação. Mudança de plano não altera o significado lógico do [[execution-graph]].

> [!info] O que já existe
> - Não há planejador de plano por requisição. O que existe é a decisão **por unidade**, no [[scheduler]]: `placement_cost` em `code/crates/controller/src/placement.rs` soma compute, transferência, fila e saúde, e `placement::decide` escolhe o nó de cada unidade a cada poll.
> - A histerese existe só nessa decisão por unidade (banda de 10% em `AI_CLUSTER_PLACEMENT_TOLERANCE`). Ver [[f04-placement]].
> - A previsão de tempo de compute que alimenta o custo vem de `predicted_compute_us`. Ver [[modelo-de-desempenho]].
> - Capacidade por nó: o nó reporta threads e memória, e `placement::fits` filtra as unidades que não cabem. A concessão de cores é decidida no worker. Ver [[f04-scheduler-local]].

> [!todo] Não implementado
> - Planejador de plano para o Model Graph: pipeline, tensor e híbrido. Depende de [[fase-12-model-runtime]].
> - Perfis de requisição (latência, vazão, confiabilidade, eficiência). Não há perfil; toda unidade recebe o mesmo tratamento.
> - Planos versionados e replanejamento em evento. Hoje não existe plano: o cluster reavalia a cada poll.
> - Termos do custo sem dado: localidade de estado e de modelo, pressão de memória, risco de falha, risco térmico, custo de rebalanceamento. O registro não carrega esses dados ainda.
> - Ganho esperado antes de replanejar (ver [[hotplug-e-recuperacao]]).

> [!warning] Divergência do MVP
> A arquitetura-alvo põe o planejador no controller, produzindo o plano a partir do Model Graph. No MVP, quem decide a divisão de camadas da inferência é o gateway, que recebe os nós por variável de ambiente (`AI_CLUSTER_<MODELO>_SHARDS`). Migração prevista: a divisão passa a ser decisão do planejador, com o [[request-manager]] recebendo a requisição antes dele.
