---
title: "Modelo de dados"
aliases: ["data-model", "Modelo de dados"]
tags: [sistema/moc]
type: sistema/moc
status: vivo
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-06-estado-e-recuperacao]]", "[[fase-13-graph-engine]]"]
---

# Modelo de dados

Entidades, relações e o modelo de grafo do cluster, com a representação atual de cada uma no controller e no protocolo. Pasta do [[system]].

## Notas

- [[entidades]]: nó, capacidade, saúde, unidade de trabalho, fatia, estado, reserva, métrica, evento e trace, com onde cada uma vive hoje e os estados de nó e de unidade.
- [[relacoes]]: como as entidades se referem umas às outras (por campo, por lista ou só conceitualmente), com cardinalidade, posse e timestamps.
- [[modelo-de-grafo]]: tipos de nó e de aresta do grafo-projeção (alvo, ainda sem implementação) e o mapeamento de cada tipo para sua origem.

Decisões relacionadas: [[adr-004-grafo-como-projecao]], [[adr-005-camadas-de-storage]].
