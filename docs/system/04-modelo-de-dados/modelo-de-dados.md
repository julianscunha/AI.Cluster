---
title: "Modelo de dados"
aliases: ["data-model", "Modelo de dados"]
tags: [sistema/moc]
type: sistema/moc
---

# Modelo de dados

Entidades, relações e o modelo de grafo do cluster, com a representação de cada uma no controller e no protocolo. Pasta do [[system]].

## Notas

- [[entidades]]: nó, capacidade, saúde, release, unidade de trabalho, fatia, estado, reserva, métrica, evento e trace, com onde cada uma vive e os estados de nó e de unidade.
- [[relacoes]]: como as entidades se referem umas às outras, com cardinalidade, posse, timestamps e a réplica de estado.
- [[modelo-de-grafo]]: tipos de nó e de aresta do grafo-projeção, planejado, e a origem de cada tipo nas entidades atuais.

Decisões relacionadas: [[adr-004-grafo-como-projecao]], [[adr-005-camadas-de-storage]], [[adr-013-estado-do-controller-e-recuperacao]].
