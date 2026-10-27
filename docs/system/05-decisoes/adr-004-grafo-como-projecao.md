---
title: "ADR-004 — Grafo como projeção"
aliases: ["ADR-004", "ADR-004 — Graph as Projection"]
tags: [sistema/adr, subsistema/dados]
type: sistema/adr
---

# ADR-004 — Grafo como projeção

Status: proposto · Data: 2026-10-19 · Voltar para [[decisoes]].

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

## Limites

- Hoje não há grafo projetado. A única estrutura de grafo é o DAG de dependências de execução no controller. O modelo de grafo está descrito em [[modelo-de-grafo]].
- A decisão de adotar o grafo como projeção depende de ganho medido, e a tecnologia segue aberta ([[graph-engine]]).
