---
title: "Sistema — MOC"
aliases: ["System", "DIRECTORY-MAP", "Directory Map", "Mapa do repositório", "MOC"]
tags: [sistema/moc]
type: sistema/moc
status: vivo
implementacao: parcial
---

# Sistema — MOC

Voltar para [[inicio]]. Esta área descreve **o que o sistema é e como ele funciona**, inclusive o alvo que ainda não existe. Cronograma, fases e status de entrega ficam em [[implementation]].

O sistema vive em `code/` (workspace Rust). A verdade sobre o que está implementado é o código; as notas de sistema linkam para os slices `fNN-*` que o descrevem.

## Ordem de leitura por camada

### Camada 1 — Visão e decisões

1. [[visao]] — escopo, princípios e requisitos.
2. [[visao-geral]] — propósito, ideia central e escopo da v0.1.
3. [[principios]] — P-001 a P-015.
4. [[requisitos]] — FR e NFR com a fase de entrega.
5. [[arquitetura]] — documento mestre da arquitetura.
6. [[estado-das-decisoes]] — decidido, proposto, experimental e aberto.

### Camada 2 — Contrato e modelo de dados

7. [[entidades]], [[relacoes]] e [[modelo-de-grafo]] — modelo de dados.
8. [[versionamento]] — evolução e compatibilidade do protocolo.
9. [[mensagens]] — mensagens de registro, trabalho e saúde.
10. [[protocolo-controller-worker]] — ciclo de vida Controller ↔ Worker.
11. [[canal-de-fatias]] — canal residente de fatias do Inference Engine.

### Camada 3 — Substrato e execução

12. [[no-e-hardware]] — node × device e fingerprint de hardware.
13. [[capability-engine]] — benchmark e capacidade multidimensional.
14. [[saude-e-confianca]] — saúde, confiança e diagnóstico.
15. [[worker-runtime]] — execução local e backends de hardware.
16. [[work-unit]] — unidade de trabalho e ciclo de vida.
17. [[camada-de-transporte]] — abstração de transporte e métricas de link.
18. [[execution-graph]] — DAG, dependências e recuperação.
19. [[execution-planner]] — estratégias, custo e replanejamento.
20. [[scheduler]] — filtragem de candidatos, scoring e reservas.
21. [[observabilidade]] — métricas, eventos, traces e CLI.
22. [[modelo-de-desempenho]] — previsão, expected vs actual e capacidade.
23. [[state-manager]] — estado, replicação, checkpoint e recuperação.
24. [[storage-intelligence]] — detecção HDD/SSD/NVMe e políticas.
25. [[hotplug-e-recuperacao]] — entrada, saída planejada, falha e replanejamento.

### Camada 4 — Dados e runtime de IA

26. [[graph-engine]] — domínios de grafo, projeção e consultas.
27. [[edge-optimizer]] — cache, retrieval, compressão e orçamento de tokens.
28. [[model-manager]] — registry, variantes e shards.
29. [[inference-api]] — fronteira do Application Plane.
30. [[request-manager]] — admissão, filas, quotas e orçamento.
31. [[inference-engine]] — execução do Transformer, KV cache e paralelismo.

### Camada 5 — Plataforma e operação

32. [[ai-cluster-os]] — sistema operacional imutável.
33. [[provisionamento]] — descoberta, enrollment e reimage.
34. [[seguranca]] — identidade, tokens, credenciais e papéis.
35. [[cli-tui]] — comandos operacionais e dashboard.

### Camada 6 — Implementação

36. [[implementation]] — fases, status, marcos e evidências. Leia depois do sistema.

## MOCs desta área

- [[visao]] — visão, princípios e requisitos.
- [[arquitetura]] — arquitetura mestre e estado das decisões.
- [[subsistemas]] — especificação de cada subsistema, com MOCs por grupo.
- [[protocolo]] — protocolo Controller ↔ Worker.
- [[modelo-de-dados]] — entidades, relações e projeção em grafo.
- [[decisoes]] — ADRs (001 a 010).
