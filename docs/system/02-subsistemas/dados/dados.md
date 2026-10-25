---
title: "Dados"
aliases: ["Camada de dados", "Grupo dados"]
tags: [sistema/moc]
type: sistema/moc
status: vivo
implementacao: parcial
---

# Dados

Subsistemas que guardam o estado do cluster, decidem onde o dado vive e relacionam entidades. Depende da execução ([[execucao]]) para o estado de Work Units e dá suporte à IA ([[ia]]) com o KV cache e as referências de estado. Voltar para [[subsistemas]].

## Notas

- [[state-manager]]: preserva o estado para retomar a execução quando placement ou hardware mudam. Hoje, a fila do controller tem snapshot e cópia num nó.
- [[storage-intelligence]]: usa o disco como recurso, com classes HDD, SSD e NVMe, alocação e políticas por saúde. Hoje, só a classificação existe.
- [[graph-engine]]: projeção em grafo para navegação, dependências e retrieval. Não implementado.

Decisões relacionadas: [[adr-004-grafo-como-projecao]], [[adr-005-camadas-de-storage]].
