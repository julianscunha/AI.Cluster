---
title: "Graph Engine"
aliases: ["Motor de grafo", "docs/16", "16 — Graph Engine"]
tags: [sistema/subsistema/dados]
type: sistema/subsistema
status: proposto
implementacao: nao-iniciado
fases: ["[[fase-13-graph-engine]]"]
---

# Graph Engine

O Graph Engine explora relações estruturadas para reduzir buscas, localizar dependências e orientar execução e escalonamento. É uma **projeção** consultável sobre os dados operacionais, não a fonte primária ([[adr-004-grafo-como-projecao]]). Voltar para [[dados]].

## Domínios de grafo

| Grafo | Nós e arestas |
|---|---|
| Conhecimento | Documento, chunk, entidade, fato, incidente, serviço |
| Hardware | Nó, CPU, RAM, GPU, NPU, armazenamento |
| Rede | Enlaces e suas métricas |
| Execução | Operações, dependências, tensores, Work Units |
| Observabilidade | Request, plano, nó, evento, métrica, incidente |

## Projeção

A fonte primária continua nos stores operacionais. O grafo pode ser reconstruído a partir deles, o que torna a projeção descartável e evita um segundo sistema de verdade.

## Papel da ideia de grafo

O projeto adota retrieval baseado em grafo quando relações e travessias reduzem o custo de busca. A tecnologia específica é validada em benchmark; não se presume que todo search seja melhor por grafo. A adoção só acontece com ganho medido.

## Consultas úteis

- Nó elegível mais próximo.
- Nós que hospedam um shard.
- Incidentes relacionados e requests afetadas.
- Skills e tools candidatas ([[edge-optimizer]]).
- Caminho entre nós.
- Réplicas de estado ([[state-manager]]).

> [!info] O que já existe (MVP)
> Nenhum Graph Engine. Não há dependência de banco de grafo no `code/`. O único grafo em execução é o DAG de Work Units do controller, em memória, usado para dependências e retry ([[execution-graph]]); ele não é uma projeção consultável nem alimenta busca. O modelo de grafo de dados está em [[modelo-de-grafo]].

> [!todo] Falta
> - Escolha da tecnologia de grafo e benchmark de travessia contra busca comum.
> - Projeção reconstruível a partir dos stores operacionais.
> - Consultas de placement e de observabilidade.
> - Integração com o [[edge-optimizer]] para retrieval.
> - Critério de adoção: só entra com ganho medido ([[fase-13-graph-engine]]).
