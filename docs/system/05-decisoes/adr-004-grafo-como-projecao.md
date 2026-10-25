---
title: "ADR-004 — Grafo como projeção"
aliases: ["ADR-004", "ADR-004 — Graph as Projection"]
tags: [sistema/adr, subsistema/dados]
type: sistema/adr
status: proposto
implementacao: nao-iniciado
fases: ["[[fase-13-graph-engine]]"]
---

# ADR-004 — Grafo como projeção

Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

Topologia, conhecimento e execução formam relações que se consultam bem como grafo. Ao mesmo tempo, tornar o grafo a única fonte de verdade aumenta o risco operacional, porque uma falha nele perderia o estado.

## Decisão

Usar o grafo como camada de relações, travessia e índice, sem obrigar que ele seja a única fonte de verdade.

## Consequências

- Reconstrução do grafo é possível a partir da fonte de verdade.
- Consultas relacionais complexas ficam disponíveis.
- Modela topologia, conhecimento e execução em uma só estrutura.
- Menor risco operacional.

## Alternativas

- Grafo como única fonte de verdade: não adotada, pelo risco operacional acima.

> [!info] O que já existe
> O DAG de dependências de execução existe no controller ([[f03-grafo-de-dependencias]]), mas não é um grafo projetado. O modelo de grafo ([[modelo-de-grafo]]) não foi iniciado.

> [!todo] Projeção, travessia e a decisão de adotar o grafo só com ganho medido ficam em [[fase-13-graph-engine]] e [[graph-engine]].
