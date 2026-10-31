---
title: "Graph Engine"
aliases: ["Graph Engine", "Motor de grafo"]
tags: [sistema/subsistema/dados]
type: sistema/subsistema
---

# Graph Engine

O graph engine explora relações estruturadas para reduzir buscas e localizar dependências. Seria uma **projeção** consultável sobre os dados operacionais, não a fonte primária ([[adr-004-grafo-como-projecao]]). Voltar para [[dados]].

## Situação

Não existe graph engine. Não há banco de grafo no código. O único grafo em execução é o DAG de Work Units do controller, em memória, usado para dependências e novas tentativas (ver [[execution-graph]]). Ele não é consultável nem alimenta busca.

## Domínios previstos

| Grafo | Workers e arestas |
|---|---|
| Conhecimento | Documento, trecho, entidade, fato, incidente, serviço |
| Hardware | Worker, CPU, RAM, GPU, NPU, armazenamento |
| Rede | Enlaces e suas métricas |
| Execução | Operações, dependências, tensores, Work Units |
| Observabilidade | Requisição, plano, worker, evento, métrica, incidente |

O modelo de dados desses grafos está em [[modelo-de-grafo]].

## Por que ainda não existe

A adoção de busca por grafo só acontece com ganho medido contra a busca comum. Para medir isso é preciso uma coleção de documentos e perguntas de recuperação, e o cluster ainda não tem nenhuma. Consultas de topologia que o cluster precisa hoje (quais workers estão no mesmo host, qual é o link entre dois workers) são respondidas pelo registro e pelo modelo de custo, sem grafo.

## Limites conhecidos

- Não há escolha de tecnologia de grafo nem benchmark de travessia.
- Não há projeção reconstruível a partir dos stores operacionais.
- Não há consultas de placement ou de observabilidade sobre grafo.

> Planejado: projeção reconstruível dos dados operacionais, consultas de worker elegível mais próximo, workers que hospedam um shard, incidentes relacionados, réplicas de estado e caminho entre workers, e integração com o [[edge-optimizer]] para recuperação. Entra só com ganho medido em benchmark de recuperação.
