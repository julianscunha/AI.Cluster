# Directory Map

## Root

- `README.md` — entrada do projeto.
- `ARCHITECTURE.md` — fonte mestre da arquitetura.
- `REQUIREMENTS.md` — requisitos.
- `PRINCIPLES.md` — princípios.
- `EXECUTION-PLAN.md` — sequência de implementação.
- `STATUS.md` — decisões, propostas, experimentos e questões abertas.

## docs/

- `01-overview.md` — visão geral.
- `02-ai-cluster-os.md` — sistema operacional.
- `03-provisioning.md` — reimage/provisioning.
- `04-node-and-hardware.md` — hardware/node model.
- `05-capability-engine.md` — benchmark/capability.
- `06-health-and-trust.md` — health/trust.
- `07-worker-runtime.md` — execução local.
- `08-work-unit.md` — unidade de trabalho.
- `09-execution-graph.md` — DAG/execution graph.
- `10-execution-planner.md` — planejamento.
- `11-scheduler.md` — scheduling.
- `12-transport-layer.md` — transporte.
- `13-state-manager.md` — estado/recovery.
- `14-model-manager.md` — modelos/shards.
- `15-edge-optimizer.md` — otimização de borda.
- `16-graph-engine.md` — grafos/retrieval.
- `17-observability.md` — métricas/eventos/traces/TUI.
- `18-storage-intelligence.md` — storage/swap/cache.
- `19-security.md` — identidade/segurança.
- `20-hotplug-and-recovery.md` — entrada/saída/falhas.
- `21-performance-model.md` — previsão/capacidade.
- `22-cli-tui.md` — interface operacional.
- `23-inference-api.md` — fronteira de consumo externo e contrato de inferência.
- `24-request-manager.md` — admissão, filas, quotas, orçamento e ciclo de vida da request.
- `25-inference-engine.md` — execução do Transformer, KV cache e paralelismo de inferência.

## protocol/

Schemas conceituais e regras do Controller ↔ Worker.

## data-model/

Entidades, relações e projeção no grafo.

## implementation/

Roadmap, fases, milestones, testes e MVP.

## adr/

Decisões arquiteturais relevantes.

## code/phase-0/

Workspace Rust executável da Fase 0. Contém `crates/`, `proto/`, `tests/`, `scripts/`, `config/` e documentação operacional específica em `code/phase-0/docs/`.
