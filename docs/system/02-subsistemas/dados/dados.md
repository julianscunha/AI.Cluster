---
title: "Dados"
aliases: ["Camada de dados", "Dados"]
tags: [sistema/moc]
type: sistema/moc
---

# Dados

Subsistemas que guardam o estado do cluster, decidem onde o dado vive e relacionam entidades. Depende da execução ([[execucao]]) para o estado de Work Units e dá suporte à inferência ([[ia]]) com as referências de estado. Voltar para [[subsistemas]].

## Notas

- [[state-manager]]: o que é preservado para retomar a execução quando o nó ou o controller falham, e onde fica.
- [[storage-intelligence]]: classificação dos discos dos nós (NVMe, SSD, HDD) e o que ela significa hoje.
- [[graph-engine]]: projeção em grafo para navegação e dependências. Ainda não existe.

Decisões relacionadas: [[adr-004-grafo-como-projecao]] e [[adr-005-camadas-de-storage]].
