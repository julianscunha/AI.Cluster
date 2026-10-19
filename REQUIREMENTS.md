# Requirements

Status: DECIDED baseline / v0.1

## Functional requirements

### Cluster

- FR-001 — Descobrir novos nós automaticamente.
- FR-002 — Identificar hardware e arquitetura.
- FR-003 — Medir capacidade real por benchmark.
- FR-004 — Recalcular capacidade do cluster após entrada/saída de nós.
- FR-005 — Suportar nós heterogêneos.
- FR-006 — Suportar hot-plug e hot-remove planejado.
- FR-007 — Recuperar estado após falha de node.
- FR-008 — Excluir nós comprometidos via quarantine/revoke.

### OS / Provisioning

- FR-010 — Aceitar Linux temporário como bootstrap.
- FR-011 — Reimaging automático para AI Cluster OS.
- FR-012 — Instalar imagem adequada a x86_64 ou ARM64.
- FR-013 — Detectar hardware antes do provisionamento definitivo.
- FR-014 — Suportar rollback e recovery.

### Inference

- FR-020 — Executar Work Units distribuídas.
- FR-021 — Suportar pipeline parallelism.
- FR-022 — Suportar tensor parallelism.
- FR-023 — Suportar estratégia híbrida.
- FR-024 — Replanejar placement.
- FR-025 — Suportar retry.
- FR-026 — Suportar checkpoint/restore.
- FR-027 — Suportar speculative execution onde justificável.

### Edge optimization

- FR-030 — Exact cache.
- FR-031 — Semantic cache.
- FR-032 — Graph retrieval.
- FR-033 — Context selection.
- FR-034 — Deduplicação.
- FR-035 — Compressão contextual.
- FR-036 — Skill/tool resolution.
- FR-037 — Prompt compilation.
- FR-038 — Token budgeting.
- FR-039 — Model routing por complexidade.

### Observability

- FR-050 — Mostrar nodes em tempo real.
- FR-051 — Mostrar health, capacity e trust separadamente.
- FR-052 — Detectar thermal throttling.
- FR-053 — Detectar degradação de storage.
- FR-054 — Mostrar métricas de rede.
- FR-055 — Mostrar tokens input/output/cached/avoided.
- FR-056 — Mostrar TTFT, ITL, tokens/s e latência.
- FR-057 — Mostrar expected vs actual.
- FR-058 — Mostrar trace completo de request.
- FR-059 — Mostrar eventos e histórico.

## Non-functional requirements

- NFR-001 — Linux-first.
- NFR-002 — Arquitetura multi-arquitetura.
- NFR-003 — Control plane independente do tipo de hardware.
- NFR-004 — Worker Runtime orientado a capabilities.
- NFR-005 — Falha de um node não deve exigir reinício do cluster.
- NFR-006 — Telemetria deve ter overhead baixo em nodes pequenos.
- NFR-007 — Métricas e estados devem ser explicáveis.
- NFR-008 — Protocolos devem possuir versionamento.
- NFR-009 — Modelo e estado de execução devem ser desacoplados.
- NFR-010 — Cache não deve ser tratado como fonte de verdade.
- NFR-011 — Segurança deve impedir node desconhecido de receber modelos/estado.
- NFR-012 — Rebalancing deve usar hysteresis para evitar oscilação.
