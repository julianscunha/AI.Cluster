---
title: "Requisitos"
aliases: ["REQUIREMENTS", "Requisitos funcionais e não funcionais"]
tags: [sistema/visao, requisitos]
type: sistema/visao
---

# Requisitos

Voltar para [[visao]]. Esta página lista o que o sistema deve fazer (FR) e as qualidades que deve ter (NFR).

A coluna **Disponibilidade** indica o que existe hoje para cada requisito:

- **disponível**: existe e funciona no escopo descrito em [[visao-geral]];
- **parcial**: existe uma parte; o que falta está descrito na nota indicada pelos limites;
- **planejado**: ainda não existe.

## Requisitos funcionais

### Cluster

| ID | Requisito | Disponibilidade |
|---|---|---|
| FR-001 | Descobrir novos workers automaticamente. | planejado |
| FR-002 | Identificar hardware e arquitetura. | parcial |
| FR-003 | Medir capacidade real por benchmark. | parcial |
| FR-004 | Recalcular capacidade do cluster após entrada/saída de workers. | parcial |
| FR-005 | Suportar workers heterogêneos. | parcial |
| FR-006 | Suportar hot-plug e hot-remove planejado. | disponível |
| FR-007 | Recuperar estado após falha de node. | disponível |
| FR-008 | Excluir workers comprometidos via quarantine/revoke. | disponível |

### SO / Provisioning

| ID | Requisito | Disponibilidade |
|---|---|---|
| FR-010 | Aceitar Linux temporário como bootstrap. | disponível |
| FR-011 | Reimaging automático para AI Cluster OS. | parcial |
| FR-012 | Instalar imagem adequada a x86_64 ou ARM64. | parcial |
| FR-013 | Detectar hardware antes do provisionamento definitivo. | planejado |
| FR-014 | Suportar rollback e recovery. | disponível |

### Inferência

| ID | Requisito | Disponibilidade |
|---|---|---|
| FR-020 | Executar Work Units distribuídas. | disponível |
| FR-021 | Suportar pipeline parallelism. | planejado |
| FR-022 | Suportar tensor parallelism. | parcial |
| FR-023 | Suportar estratégia híbrida. | planejado |
| FR-024 | Replanejar placement. | disponível |
| FR-025 | Suportar retry. | disponível |
| FR-026 | Suportar checkpoint/restore. | parcial |
| FR-027 | Suportar speculative execution onde justificável. | planejado |

### Otimização de edge

| ID | Requisito | Disponibilidade |
|---|---|---|
| FR-030 | Exact cache. | disponível |
| FR-031 | Semantic cache. | planejado |
| FR-032 | Graph retrieval. | planejado |
| FR-033 | Context selection. | planejado |
| FR-034 | Deduplicação. | planejado |
| FR-035 | Compressão contextual. | planejado |
| FR-036 | Skill/tool resolution. | planejado |
| FR-037 | Prompt compilation. | planejado |
| FR-038 | Token budgeting. | parcial |
| FR-039 | Model routing por complexidade. | planejado |

### Observabilidade

| ID | Requisito | Disponibilidade |
|---|---|---|
| FR-050 | Mostrar nodes em tempo real. | disponível |
| FR-051 | Mostrar health, capacity e trust separadamente. | disponível |
| FR-052 | Detectar thermal throttling. | disponível |
| FR-053 | Detectar degradação de storage. | planejado |
| FR-054 | Mostrar métricas de rede. | parcial |
| FR-055 | Mostrar tokens input/output/cached/avoided. | disponível |
| FR-056 | Mostrar TTFT, ITL, tokens/s e latência. | disponível |
| FR-057 | Mostrar expected vs actual. | disponível |
| FR-058 | Mostrar trace completo de request. | parcial |
| FR-059 | Mostrar eventos e histórico. | disponível |

## Requisitos não funcionais

| ID | Requisito | Disponibilidade |
|---|---|---|
| NFR-001 | Linux-first. | disponível |
| NFR-002 | Arquitetura multi-arquitetura. | parcial |
| NFR-003 | Control plane independente do tipo de hardware. | disponível |
| NFR-004 | Worker Runtime orientado a capabilities. | disponível |
| NFR-005 | Falha de um node não deve exigir reinício do cluster. | disponível |
| NFR-006 | Telemetria deve ter overhead baixo em nodes pequenos. | parcial |
| NFR-007 | Métricas e estados devem ser explicáveis. | parcial |
| NFR-008 | Protocolos devem possuir versionamento. | disponível |
| NFR-009 | Modelo e estado de execução devem ser desacoplados. | planejado |
| NFR-010 | Cache não deve ser tratado como fonte de verdade. | parcial |
| NFR-011 | Segurança deve impedir node desconhecido de receber modelos/estado. | disponível |
| NFR-012 | Rebalancing deve usar hysteresis para evitar oscilação. | disponível |
