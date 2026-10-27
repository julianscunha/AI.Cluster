---
title: "Execução"
aliases: ["Execução distribuída", "Execution"]
tags: [sistema/moc, sistema/subsistema/execucao]
type: sistema/moc
---

# Execução

Como uma requisição vira trabalho executado em vários nós: o grafo de unidades com dependências, o escalonamento global e local, o modelo que prevê custo e o que acontece quando um nó entra, sai ou falha. Voltar para [[subsistemas]].

## Notas

- [[execution-graph]]: operações (Work Units) com dependências explícitas e ligações de dado entre elas.
- [[scheduler]]: escolha do nó de cada unidade (global) e uso dos núcleos (local); filtros, custo, reservas e prioridade.
- [[modelo-de-desempenho]]: previsão de tempo de compute por unidade e como ela se corrige com o uso.
- [[hotplug-e-recuperacao]]: entrada, saída planejada, queda de nó, reinício do controller e revogação.
- [[execution-planner]]: planejamento de execução por requisição (ainda não existe).

## Como uma unidade anda

1. O cliente enfileira unidades, opcionalmente com dependências e prioridade.
2. Cada worker pede trabalho. O controller decide, naquele pedido, se entrega a unidade àquele nó.
3. O nó executa e devolve o resultado. O controller confere o resultado e libera os dependentes.
4. Se o nó sai ou cai, as unidades em voo voltam à fila. Se as tentativas acabam, a unidade falha junto com seus dependentes.

O despacho é por demanda. O placement é um filtro e um ranking sobre o nó que pergunta, não uma escolha de empurrar trabalho para um nó.

## Limites conhecidos

- Não existe planejador de plano por requisição. A divisão do trabalho é decidida unidade a unidade.
- A inferência por fatias não passa pela fila de unidades nem pelo placement. Os nós que recebem as fatias são escolhidos por configuração (ver [[inference-engine]]).
- O sistema reavalia o cluster a cada pedido de trabalho. Não há planos versionados nem replanejamento por evento.

> Planejado: planejador que monta o grafo de execução a partir do modelo, com estratégias de pipeline e de tensor, perfis de requisição (latência, vazão, confiabilidade) e replanejamento versionado. Depende do [[request-manager]] e do [[model-manager]].
