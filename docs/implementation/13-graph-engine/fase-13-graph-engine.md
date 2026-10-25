---
title: "F13 — Graph engine"
aliases: ["Fase L — Graph Engine", "docs/16 (graph engine)"]
tags: [implementacao/fase/13, implementacao/estado/planejado]
type: implementacao/fase
status: planejado
codigos-antigos: ["EP L1–L4", "roadmap 5 (graph representation, graph retrieval)", "M5 (parte de recuperação de grafo)"]
sistema: ["[[graph-engine]]", "[[modelo-de-grafo]]", "[[adr-004-grafo-como-projecao]]", "[[entidades]]", "[[relacoes]]"]
---
# F13 — Graph engine

## Objetivo

Representar hardware, rede, conhecimento, execução e observabilidade como um grafo consultável, projetado a partir dos objetos operacionais. A travessia só entra onde o retrieval medido ganhar do baseline.

> [!note] Não confundir com o Execution Graph
> O DAG de Work Units de [[fase-03-execucao-distribuida]] já existe e não é o graph engine desta fase. Aqui é o modelo de grafo de topologia e conhecimento (ADR-004: grafo como projeção).

## Depende de

- [[fase-05-observabilidade]]: eventos e métricas que alimentam a projeção.
- [[fase-06-estado-e-recuperacao]]: estado que o grafo projeta.

## Entrega

- Modelo de dados com as cinco famílias: hardware, rede, conhecimento, execução e observabilidade (L1).
- Projeção que sincroniza os objetos operacionais (L2).
- Consultas de topologia, localidade e relação (L3).
- Benchmark de retrieval com grafo contra o baseline (L4).

## Checklist

Verificado em `code/crates`: não há modelo de grafo além do DAG de execução de [[fase-03-execucao-distribuida]] e do grafo sintético de `/v1/embeddings`.

- [ ] L1 Modelo de dados de grafo (hardware, rede, conhecimento, execução, observabilidade). Não existe.
- [ ] L2 Projeção que sincroniza objetos operacionais. Não existe.
- [ ] L3 Travessia com queries de topologia, localidade e relação. Não existe.
- [ ] L4 Benchmark de retrieval com grafo contra baseline. Não existe.
- [ ] Critério de saída: só adotar travessia onde houver ganho mensurável. **Hoje não há como medir**: o sistema não tem corpus de conhecimento nem tarefa de recuperação (RAG, contexto de longo prazo) para o L4 comparar o grafo com um baseline. A fase fica **não iniciada de propósito**, e não porque falte tempo.

> [!warning] Decisão: não construir antes de existir o que recuperar
> O gate da fase proíbe adotar travessia sem ganho medido, e o ganho de recuperação por grafo só se mede contra uma coleção de documentos e perguntas. Nenhuma fase anterior produz isso. Construir L1–L3 sem L4 seria o oposto do gate: infraestrutura sem prova. O que as consultas de topologia que o cluster já precisa (quais nós estão no mesmo host, qual link entre dois nós) fazem hoje o registry e o modelo de custo, sem grafo. Reabrir quando houver um caso de recuperação real, junto de [[fase-14-edge-optimizer]] (M3).

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): só adotar com ganho medido. Sem L4 positivo, a travessia não entra no runtime.

## Slices

Notas desta pasta (esta nota é o MOC): nenhum slice criado.

Slices a criar:

- `f13-modelo-de-grafo`: L1.
- `f13-projecao`: L2.
- `f13-travessia`: L3.
- `f13-benchmark-de-retrieval`: L4.

## Códigos antigos

- EP L1–L4.
- Roadmap 5, itens "graph representation" e "graph retrieval".
- Marco M5 (benchmark de recuperação de grafo), em conjunto com [[fase-14-edge-optimizer]].
- Pacote da Fase 5 em `phases.md`: graph ingestion, graph queries, retrieval evaluator.

## Specs do sistema

- [[graph-engine]]: subsistema-alvo.
- [[modelo-de-grafo]]: modelo de grafo.
- [[adr-004-grafo-como-projecao]]: decisão de grafo como projeção.
- [[entidades]] e [[relacoes]]: objetos que o grafo projeta.
