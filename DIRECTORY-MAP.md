---
title: "Mapa do repositório (MOC)"
aliases: ["MOC", "Map of Content", "Directory Map"]
type: system
status: living
tags: [system, navigation, moc]
---

# Mapa do repositório — MOC

Ponto de entrada para navegação. `ARCHITECTURE.md` continua sendo a fonte de
verdade da arquitetura; este documento diz **onde cada coisa vive** e **em que
ordem ler**.

## 1. Classificação em três camadas

| Camada | Papel | Onde vive |
|---|---|---|
| **Documento de sistema** | Visão, princípios, requisitos, arquitetura, decisões e especificação de subsistemas. Responde *o quê* e *por quê*. | raiz, `docs/`, `adr/`, `data-model/`, `protocol/` |
| **Documento de implementação** | Como e quando construir: roadmap, fases, milestones, MVP, testes, plano de execução e documentação operacional da Fase 0. | `implementation/` (inclui `implementation/phase-0/` e `implementation/EXECUTION-PLAN.md`) |
| **Arquivo de sistema** | O sistema em si: código, contrato de wire, configuração e CI. | `code/crates/`, `code/proto/`, `code/Cargo.*`, `.github/workflows/`, `.gitignore` |

## 2. Sequência otimizada

A numeração dos arquivos (`docs/01` … `docs/25`) é **ordem de escrita, não ordem
de dependência**. A sequência abaixo foi validada contra a cadeia de dependências
de `EXECUTION-PLAN.md` §1 e `implementation/roadmap.md`.

### Camada 1 — Visão e decisões

1. [README.md](README.md) — entrada do projeto.
2. [docs/01-overview.md](docs/01-overview.md) — escopo, ideia central e cargas de trabalho.
3. [PRINCIPLES.md](PRINCIPLES.md) — princípios de projeto (P-001…P-015).
4. [REQUIREMENTS.md](REQUIREMENTS.md) — requisitos funcionais e não funcionais.
5. [ARCHITECTURE.md](ARCHITECTURE.md) — fonte mestra da arquitetura.
6. [STATUS.md](STATUS.md) — decidido / proposto / experimental / aberto.

### Camada 2 — Contrato e modelo de dados

7. [data-model/entities.md](data-model/entities.md) — entidades e regras de identidade.
8. [data-model/relationships.md](data-model/relationships.md) — relações entre entidades.
9. [protocol/versioning.md](protocol/versioning.md) — evolução e compatibilidade.
10. [protocol/messages.md](protocol/messages.md) — mensagens de registro, trabalho e saúde.
11. [protocol/controller-worker.md](protocol/controller-worker.md) — lifecycle Controller ↔ Worker.
12. [code/proto/cluster.proto](code/proto/cluster.proto) — contrato de wire real (system file).

### Camada 3 — Substrato de execução

13. [docs/04-node-and-hardware.md](docs/04-node-and-hardware.md) — node vs device, fingerprint de hardware.
14. [docs/05-capability-engine.md](docs/05-capability-engine.md) — benchmark e scoring multidimensional.
15. [docs/06-health-and-trust.md](docs/06-health-and-trust.md) — saúde, confiança e diagnóstico.
16. [docs/07-worker-runtime.md](docs/07-worker-runtime.md) — execução local e backends de hardware.
17. [docs/08-work-unit.md](docs/08-work-unit.md) — unidade de trabalho, requisitos e ciclo de vida.
18. [docs/12-transport-layer.md](docs/12-transport-layer.md) — abstração de transporte e métricas de link.
19. [docs/09-execution-graph.md](docs/09-execution-graph.md) — DAG, dependências e recovery.
20. [docs/10-execution-planner.md](docs/10-execution-planner.md) — estratégias, custo e replanejamento.
21. [docs/11-scheduler.md](docs/11-scheduler.md) — filtragem de candidatos, scoring e reservas.
22. [docs/17-observability.md](docs/17-observability.md) — métricas, eventos, traces e TUI.
23. [docs/21-performance-model.md](docs/21-performance-model.md) — previsão, expected vs actual, capacidade.
24. [docs/13-state-manager.md](docs/13-state-manager.md) — estado, replicação, checkpoint e recovery.
25. [docs/18-storage-intelligence.md](docs/18-storage-intelligence.md) — detecção HDD/SSD/NVMe e políticas.
26. [docs/20-hotplug-and-recovery.md](docs/20-hotplug-and-recovery.md) — join, drain, falha e replanejamento.

### Camada 4 — Otimização e runtime de IA

27. [docs/16-graph-engine.md](docs/16-graph-engine.md) — domínios de grafo, projeção e queries.
28. [docs/15-edge-optimizer.md](docs/15-edge-optimizer.md) — cache, retrieval, compressão e token budget.
29. [docs/14-model-manager.md](docs/14-model-manager.md) — registry, variantes e shards.
30. [docs/23-inference-api.md](docs/23-inference-api.md) — fronteira do Application Plane.
31. [docs/24-request-manager.md](docs/24-request-manager.md) — admissão, filas, quotas e orçamento.
32. [docs/25-inference-engine.md](docs/25-inference-engine.md) — execução do Transformer, KV cache e paralelismo.

### Camada 5 — Ambiente e operação

33. [docs/02-ai-cluster-os.md](docs/02-ai-cluster-os.md) — sistema operacional imutável.
34. [docs/03-provisioning.md](docs/03-provisioning.md) — descoberta, enrollment e reimage.
35. [docs/19-security.md](docs/19-security.md) — identidade, attestation e quarantine.
36. [docs/22-cli-tui.md](docs/22-cli-tui.md) — comandos operacionais e dashboard.

### Camada 6 — Implementação

37. [implementation/roadmap.md](implementation/roadmap.md) — fases 0–9 e critérios de saída.
38. [implementation/phases.md](implementation/phases.md) — pacotes de trabalho por fase.
39. [implementation/milestones.md](implementation/milestones.md) — M0–M7 e critérios de aceitação.
40. [implementation/mvp.md](implementation/mvp.md) — primeiro build e critérios de sucesso.
41. [implementation/testing-strategy.md](implementation/testing-strategy.md) — camadas de teste e matriz de benchmark.
42. [EXECUTION-PLAN.md](implementation/EXECUTION-PLAN.md) — plano mestre de execução.
43. [code/README.md](code/README.md) — escopo do workspace executável.
44. [implementation/phase-0/phase-0-acceptance.md](implementation/phase-0/phase-0-acceptance.md) — critérios A1–A5.
45. [implementation/phase-0/two-machine-validation.md](implementation/phase-0/two-machine-validation.md) — runbook de duas máquinas.
46. [implementation/phase-0/next-tests.md](implementation/phase-0/next-tests.md) — testes verificados e pendentes.
47. [implementation/phase-0/current-stack.md](implementation/phase-0/current-stack.md) — stack inicial.
48. [implementation/phase-0/repository-structure.md](implementation/phase-0/repository-structure.md) — regras do workspace Rust.
49. [code/os/README.md](code/os/README.md) — notas de OS do workspace.
50. [implementation/phase-1/join-and-leave.md](implementation/phase-1/join-and-leave.md) — entrada e saída de nós.
51. [implementation/phase-2/dependency-graph.md](implementation/phase-2/dependency-graph.md) — grafo de dependências.
52. [implementation/telemetry.md](implementation/telemetry.md) — eventos de ciclo de vida e incidentes (H2).
53. [implementation/metrics.md](implementation/metrics.md) — roll-up de métricas do cluster (H1).
54. [implementation/persistence.md](implementation/persistence.md) — snapshot da fila, dos outputs, do dono das unidades em voo e a cópia (I1).
55. [implementation/phase-0/benchmark-sweep.md](implementation/phase-0/benchmark-sweep.md) — varredura 1/2/3 nós × tamanhos.
56. [implementation/phase-2/placement.md](implementation/phase-2/placement.md) — filtro por capability/memória e escolha de nó.
57. [implementation/phase-2/local-scheduler.md](implementation/phase-2/local-scheduler.md) — concessão de cores e execução paralela.
58. [implementation/phase-2/fault-tolerance.md](implementation/phase-2/fault-tolerance.md) — retries e recuperação de nó.
59. [implementation/phase-2/capability-benchmark.md](implementation/phase-2/capability-benchmark.md) — compute medido no worker (C2).
60. [implementation/traces.md](implementation/traces.md) — correlação requisição ↔ unidades (H3).
61. [implementation/dashboard.md](implementation/dashboard.md) — painel de uma tela (H4).
62. [implementation/reservations.md](implementation/reservations.md) — reserva de nó para execução futura (G6).
63. [implementation/predictions.md](implementation/predictions.md) — previsto vs real: o erro do modelo de custo (H5).
64. [implementation/storage-tiers.md](implementation/storage-tiers.md) — classificação HOT/WARM/COLD do armazenamento por nó (I2).
65. [implementation/health-model.md](implementation/health-model.md) — score de saúde do nó: pressão de memória, swap e térmica (C4).

### Referência — decisões arquiteturais

[ADR-001](adr/ADR-001-linux-base.md) · [ADR-002](adr/ADR-002-immutable-os.md) ·
[ADR-003](adr/ADR-003-controller-worker.md) · [ADR-004](adr/ADR-004-graph-as-projection.md) ·
[ADR-005](adr/ADR-005-storage-tiering.md) · [ADR-006](adr/ADR-006-hotplug.md) ·
[ADR-007](adr/ADR-007-inference-api-boundary.md)

## 3. Por que a numeração não é a sequência

Cinco divergências entre a numeração atual e a cadeia de dependências:

1. `docs/12-transport-layer.md` é dependência de `docs/09-execution-graph.md`
   (Work Unit → Transport → Execution Graph), mas está numerado depois.
2. `docs/02-ai-cluster-os.md` e `docs/03-provisioning.md` estão no início, enquanto
   roadmap e plano de execução colocam OS/provisioning na Fase 3, depois do substrato.
3. `docs/17-observability.md` e `docs/21-performance-model.md` estão depois de
   `docs/14`–`docs/16`, mas telemetria é pré-requisito para validar capability,
   health e scheduling.
4. `docs/18-storage-intelligence.md` está longe de `docs/13-state-manager.md`,
   embora storage e estado pertençam à mesma fase (Phase 4 — Storage/State).
5. `docs/14-model-manager.md` precede `docs/15-edge-optimizer.md` e
   `docs/16-graph-engine.md`, mas o roadmap ordena Graph + Edge antes do Model Runtime.

Nenhum arquivo foi renomeado: os nomes são âncoras estáveis. A sequência vive aqui.

## 4. Convenção Obsidian

- **Vault root** = raiz do repositório. `code/` faz parte do vault.
- **Frontmatter** obrigatório em todo documento novo:

```yaml
---
title: "Nome do documento"
type: spec
status: draft
tags: [spec, topico]
---
```

- **Valores de `type`:** `system`, `spec`, `implementation`, `protocol`, `data-model`, `adr`, `operational`.
- **Valores de `status`:** `decided`, `proposed`, `experimental`, `open`, `implemented`, `draft`, `living`.
- **Links:** markdown relativo (`[texto](caminho.md)`). Renderiza no GitHub e o Obsidian
  resolve como link interno (backlinks e grafo funcionam). `[[wikilink]]` também é aceito.
- **Regra:** documento novo = arquivo com frontmatter + link a partir deste MOC.
- `.obsidian/` é ignorado no git (estado local do aplicativo, não do repositório).

## 5. Árvore

```text
Real.AI.Cluster/
├── README.md  ARCHITECTURE.md  PRINCIPLES.md  REQUIREMENTS.md  STATUS.md
├── DIRECTORY-MAP.md (este MOC)  AGENTS.md
├── adr/            decisões arquiteturais (ADR-001…007)
├── data-model/     entidades, relações e modelo de grafo
├── docs/           especificação de subsistemas (01…25)
├── implementation/ roadmap, fases, milestones, MVP, testes
│   ├── EXECUTION-PLAN.md   plano mestre de execução
│   └── phase-0/    documentação operacional da Fase 0
├── protocol/       protocolo Controller ↔ Worker
└── code/           sistema — workspace Rust, diretório final
    ├── crates/     controller, worker, common, protocol, transport, benchmark, cli
    ├── proto/      cluster.proto (contrato de wire)
    └── os/         notas de OS
```

O sistema vive em `code/` desde o início. Fases são **incrementos aplicados ao
mesmo diretório**, não pastas novas — por isso não existe `code/phase-0/`.
