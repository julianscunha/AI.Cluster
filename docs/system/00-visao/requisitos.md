---
title: "Requisitos"
aliases: ["REQUIREMENTS", "Requisitos funcionais e não funcionais"]
tags: [sistema/visao, requisitos]
type: sistema/visao
status: decidido
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-01-nucleo-do-cluster]]", "[[fase-02-capability-e-saude]]", "[[fase-03-execucao-distribuida]]", "[[fase-04-escalonamento]]", "[[fase-05-observabilidade]]", "[[fase-06-estado-e-recuperacao]]", "[[fase-07-seguranca-base]]", "[[fase-08-inferencia]]", "[[fase-09-transporte]]", "[[fase-10-inteligencia-de-storage]]", "[[fase-11-request-manager]]", "[[fase-12-model-runtime]]", "[[fase-13-graph-engine]]", "[[fase-14-edge-optimizer]]", "[[fase-15-ai-cluster-os]]", "[[fase-16-aceleradores]]", "[[fase-17-runtime-adaptativo]]", "[[fase-18-worker-movel]]", "[[fase-19-hardening-de-producao]]"]
---

# Requisitos

Voltar para [[visao]]. Baseline DECIDIDO, v0.1. A coluna **Fase** indica a fase planejada que entrega o requisito, conforme [[implementation]]; `sem fase` marca o que ainda não tem fase atribuída. O estado de implementação de cada fase está em [[status]].

## Requisitos funcionais

### Cluster

| ID | Requisito | Fase |
|---|---|---|
| FR-001 | Descobrir novos nós automaticamente. | [[fase-02-capability-e-saude]] |
| FR-002 | Identificar hardware e arquitetura. | [[fase-02-capability-e-saude]] |
| FR-003 | Medir capacidade real por benchmark. | [[fase-02-capability-e-saude]] |
| FR-004 | Recalcular capacidade do cluster após entrada/saída de nós. | [[fase-02-capability-e-saude]] |
| FR-005 | Suportar nós heterogêneos. | [[fase-02-capability-e-saude]] |
| FR-006 | Suportar hot-plug e hot-remove planejado. | [[fase-01-nucleo-do-cluster]] |
| FR-007 | Recuperar estado após falha de node. | [[fase-06-estado-e-recuperacao]] |
| FR-008 | Excluir nós comprometidos via quarantine/revoke. | [[fase-07-seguranca-base]] |

### SO / Provisioning

| ID | Requisito | Fase |
|---|---|---|
| FR-010 | Aceitar Linux temporário como bootstrap. | [[fase-15-ai-cluster-os]] |
| FR-011 | Reimaging automático para AI Cluster OS. | [[fase-15-ai-cluster-os]] |
| FR-012 | Instalar imagem adequada a x86_64 ou ARM64. | [[fase-15-ai-cluster-os]] |
| FR-013 | Detectar hardware antes do provisionamento definitivo. | [[fase-15-ai-cluster-os]] |
| FR-014 | Suportar rollback e recovery. | [[fase-19-hardening-de-producao]] |

### Inferência

| ID | Requisito | Fase |
|---|---|---|
| FR-020 | Executar Work Units distribuídas. | [[fase-03-execucao-distribuida]] |
| FR-021 | Suportar pipeline parallelism. | [[fase-12-model-runtime]] |
| FR-022 | Suportar tensor parallelism. | [[fase-12-model-runtime]] |
| FR-023 | Suportar estratégia híbrida. | [[fase-12-model-runtime]] |
| FR-024 | Replanejar placement. | [[fase-03-execucao-distribuida]] |
| FR-025 | Suportar retry. | [[fase-03-execucao-distribuida]] |
| FR-026 | Suportar checkpoint/restore. | [[fase-06-estado-e-recuperacao]] |
| FR-027 | Suportar speculative execution onde justificável. | sem fase (experimental) |

### Otimização de edge

| ID | Requisito | Fase |
|---|---|---|
| FR-030 | Exact cache. | [[fase-14-edge-optimizer]] |
| FR-031 | Semantic cache. | [[fase-14-edge-optimizer]] |
| FR-032 | Graph retrieval. | [[fase-13-graph-engine]] |
| FR-033 | Context selection. | [[fase-14-edge-optimizer]] |
| FR-034 | Deduplicação. | [[fase-14-edge-optimizer]] |
| FR-035 | Compressão contextual. | [[fase-14-edge-optimizer]] |
| FR-036 | Skill/tool resolution. | [[fase-14-edge-optimizer]] |
| FR-037 | Prompt compilation. | [[fase-14-edge-optimizer]] |
| FR-038 | Token budgeting. | [[fase-14-edge-optimizer]] |
| FR-039 | Model routing por complexidade. | [[fase-14-edge-optimizer]] |

### Observabilidade

| ID | Requisito | Fase |
|---|---|---|
| FR-050 | Mostrar nodes em tempo real. | [[fase-05-observabilidade]] |
| FR-051 | Mostrar health, capacity e trust separadamente. | [[fase-02-capability-e-saude]] |
| FR-052 | Detectar thermal throttling. | [[fase-02-capability-e-saude]] |
| FR-053 | Detectar degradação de storage. | [[fase-10-inteligencia-de-storage]] |
| FR-054 | Mostrar métricas de rede. | [[fase-05-observabilidade]] |
| FR-055 | Mostrar tokens input/output/cached/avoided. | [[fase-05-observabilidade]] |
| FR-056 | Mostrar TTFT, ITL, tokens/s e latência. | [[fase-05-observabilidade]] |
| FR-057 | Mostrar expected vs actual. | [[fase-05-observabilidade]] |
| FR-058 | Mostrar trace completo de request. | [[fase-05-observabilidade]] |
| FR-059 | Mostrar eventos e histórico. | [[fase-05-observabilidade]] |

## Requisitos não funcionais

| ID | Requisito | Fase |
|---|---|---|
| NFR-001 | Linux-first. | [[fase-00-fundacao]] |
| NFR-002 | Arquitetura multi-arquitetura. | [[fase-16-aceleradores]] |
| NFR-003 | Control plane independente do tipo de hardware. | [[fase-00-fundacao]] |
| NFR-004 | Worker Runtime orientado a capabilities. | [[fase-02-capability-e-saude]] |
| NFR-005 | Falha de um node não deve exigir reinício do cluster. | [[fase-03-execucao-distribuida]] |
| NFR-006 | Telemetria deve ter overhead baixo em nodes pequenos. | [[fase-05-observabilidade]] |
| NFR-007 | Métricas e estados devem ser explicáveis. | [[fase-05-observabilidade]] |
| NFR-008 | Protocolos devem possuir versionamento. | [[fase-00-fundacao]] |
| NFR-009 | Modelo e estado de execução devem ser desacoplados. | [[fase-12-model-runtime]] |
| NFR-010 | Cache não deve ser tratado como fonte de verdade. | [[fase-14-edge-optimizer]] |
| NFR-011 | Segurança deve impedir node desconhecido de receber modelos/estado. | [[fase-07-seguranca-base]] |
| NFR-012 | Rebalancing deve usar hysteresis para evitar oscilação. | [[fase-04-escalonamento]] |
