---
title: "Execução"
aliases: ["Execução distribuída", "Execution", "docs/09", "docs/10", "docs/11", "docs/20", "docs/21"]
tags: [sistema/moc, sistema/subsistema/execucao]
type: sistema/moc
status: vivo
implementacao: parcial
---

# Execução

Voltar para [[subsistemas]].

Como uma requisição vira trabalho executado em vários nós: o grafo de unidades com dependências, o planejamento, o escalonamento global e local, o modelo que prevê custo e capacidade, e o que acontece quando um nó entra, sai ou falha. Fases de implementação: [[fase-03-execucao-distribuida]], [[fase-04-escalonamento]], [[fase-06-estado-e-recuperacao]].

## Notas

- [[execution-graph]]: grafo de operações (Work Units) com dependências explícitas e ligações de dados entre elas.
- [[execution-planner]]: plano que minimiza custo total sob as restrições do cluster; estratégias de paralelismo e replanejamento versionado.
- [[scheduler]]: atribuição de Work Units a nós (global) e a recursos locais (cores); filtros, custo, reservas, rebalanceamento.
- [[modelo-de-desempenho]]: previsão de custo e capacidade e comparação com o resultado observado.
- [[hotplug-e-recuperacao]]: entrada, saída planejada e perda de nó; recuperação de posse de trabalho e de estado.

## Ordem de leitura

[[execution-graph]] → [[scheduler]] → [[execution-planner]] → [[modelo-de-desempenho]] → [[hotplug-e-recuperacao]].

## Situação por nota

| Nota | Implementação hoje |
| --- | --- |
| [[execution-graph]] | Parcial: dependências e ligações entre unidades, sem grafo de modelo |
| [[execution-planner]] | Não iniciado: decisão é por unidade, dentro do scheduler |
| [[scheduler]] | Parcial: placement global por custo e reservas; sem prioridades, rebalanceamento nem especulação |
| [[modelo-de-desempenho]] | Parcial: previsão de tempo de compute por unidade; sem capacidade agregada |
| [[hotplug-e-recuperacao]] | Parcial: saída e perda de nó com requeue; sem drenagem e sem estado de falha |

> [!note] Escopo
> Estas notas descrevem a arquitetura-alvo completa. O que o código faz hoje está no fim de cada uma, em `> [!info] O que já existe`.
