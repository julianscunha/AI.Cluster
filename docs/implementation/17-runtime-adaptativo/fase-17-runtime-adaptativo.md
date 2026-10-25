---
title: "F17 — Runtime adaptativo"
aliases: ["Fase P — Adaptive Runtime", "M-adaptativo"]
tags: [implementacao/fase/17, implementacao/estado/parcial]
type: implementacao/fase
status: parcial
codigos-antigos: ["EP P1–P6", "roadmap 8"]
sistema: ["[[modelo-de-desempenho]]", "[[scheduler]]", "[[execution-planner]]", "[[observabilidade]]"]
---
# F17 — Runtime adaptativo

## Objetivo

Aprender com o desempenho observado para melhorar a decisão do cluster: modelo de desempenho aprendido, placement preditivo, prefetch, execução especulativa, inferência de capacidade e rebalanceamento automático. Tudo com prova estatística de que a previsão melhora em carga repetida.

## Depende de

- [[fase-05-observabilidade]]: previsto vs real, já medido em H5.
- [[fase-12-model-runtime]]: cargas de modelo repetidas, onde a previsão rende.

## Entrega

- Modelo de desempenho aprendido (P1).
- Placement preditivo (P2).
- Prefetch (P3) e execução especulativa (P4).
- Inferência de capacidade (P5).
- Rebalanceamento automático (P6).

## Checklist

Verificado em `code/crates/controller/src/placement.rs`.

- [~] P1 Modelo de desempenho aprendido. Razão real/previsto aprendida por nó (média móvel) e aplicada às previsões seguintes; o erro cai de 33,5 para 0,08 num nó mal calibrado ([[f17-modelo-aprendido]]). Não depende do tamanho da unidade nem persiste.
- [ ] P2 Placement preditivo. Não existe. O placement é função pura dos scores atuais.
- [ ] P3 Prefetch. Não existe.
- [ ] P4 Execução especulativa. Não existe.
- [ ] P5 Inferência de capacidade. Não existe.
- [ ] P6 Rebalanceamento automático. Não existe. A reatribuição de faixas em falha é reativa.
- [ ] Critério de saída: previsão melhora estatisticamente em workload repetido e o sistema prova expected vs actual.

> [!note] Base existente
> O erro de previsão já é medido (H5: de ~32× para poucos por cento). Isso dá a régua contra a qual P1 deve melhorar.

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): melhora estatística em workload repetido, com expected vs actual provado pela telemetria.

## Slices

Notas desta pasta (esta nota é o MOC):

- [[f17-modelo-aprendido]]: P1, correção por nó aprendida do real contra o previsto.

Slices a criar:

- `f17-placement-preditivo`: P2.
- `f17-prefetch`: P3.
- `f17-execucao-especulativa`: P4.
- `f17-inferencia-de-capacidade`: P5.
- `f17-rebalanceamento-automatico`: P6.

## Códigos antigos

- EP P1–P6.
- Roadmap 8 (learned performance model, dynamic rebalancing, speculative execution, predictive prefetch, capacity inference).

## Specs do sistema

- [[modelo-de-desempenho]]: modelo de custo e previsão.
- [[scheduler]]: decisão de placement.
- [[execution-planner]]: planejamento de execução.
- [[observabilidade]]: telemetria de previsto vs real.
