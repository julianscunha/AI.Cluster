---
title: "Equivalência de fases"
aliases: ["Mapa de nomes antigos", "Fases antigas"]
tags: [implementacao/moc, implementacao/transversal]
type: implementacao/transversal
status: vivo
superado: false
---

# Equivalência de fases

Tabela de tradução dos nomes antigos (roadmap, plano de execução, marcos e séries de segurança/inferência) para as fases `F00`–`F19`. Índice: [[implementation]].

- [ ] Superado

## Roadmap antigo (Fase 0–9)

| Antigo | Novo |
|---|---|
| Fase 0 — Arquitetura e harness de benchmark | [[fase-00-fundacao]] |
| Fase 1 — Cluster mínimo | [[fase-01-nucleo-do-cluster]] + [[fase-02-capability-e-saude]] |
| Fase 2 — Execução distribuída | [[fase-03-execucao-distribuida]] + [[fase-04-escalonamento]] |
| Fase 3 — AI Cluster OS | [[fase-15-ai-cluster-os]] |
| Fase 4 — Storage/State | [[fase-06-estado-e-recuperacao]] + [[fase-10-inteligencia-de-storage]] |
| Fase 5 — Graph + Edge Optimizer | [[fase-13-graph-engine]] + [[fase-14-edge-optimizer]] |
| Fase 6 — Model Runtime | [[fase-12-model-runtime]] |
| Fase 7 — Aceleração heterogênea | [[fase-16-aceleradores]] + [[fase-18-worker-movel]] |
| Fase 8 — Otimização adaptativa | [[fase-17-runtime-adaptativo]] |
| Fase 9 — Hardening de produção | [[fase-19-hardening-de-producao]] |

## Plano de execução antigo (Fases 0, A–Q)

| Antigo | Novo |
|---|---|
| Fase 0 (A1–A3) — Fundação | [[fase-00-fundacao]] |
| B — Cluster Core | [[fase-01-nucleo-do-cluster]] |
| C — Capability / Health | [[fase-02-capability-e-saude]] |
| D — Work Unit Runtime | [[fase-03-execucao-distribuida]] |
| E — Transport | [[fase-09-transporte]] (E4, USB, em [[fase-18-worker-movel]]) |
| F — Execution Graph | [[fase-03-execucao-distribuida]] (F3, placement, em [[fase-04-escalonamento]]) |
| G — Scheduler | [[fase-04-escalonamento]] |
| H — Observability | [[fase-05-observabilidade]] |
| I — State Manager | [[fase-06-estado-e-recuperacao]] (I2, alocação de storage, em [[fase-10-inteligencia-de-storage]]) |
| J — AI Cluster OS | [[fase-15-ai-cluster-os]] |
| K — Storage Intelligence | [[fase-10-inteligencia-de-storage]] |
| L — Graph Engine | [[fase-13-graph-engine]] |
| M — Edge Optimizer | [[fase-14-edge-optimizer]] |
| N — Model Runtime | [[fase-12-model-runtime]] (N4 e N6 adiantados em [[fase-08-inferencia]]) |
| O — Accelerators | [[fase-16-aceleradores]] |
| P — Adaptive Runtime | [[fase-17-runtime-adaptativo]] |
| Q — Mobile Worker | [[fase-18-worker-movel]] |

## Séries de slices

| Antigo | Novo |
|---|---|
| S1 (token de join) | [[f07-token-de-join]] |
| S2 e S5 (aprovação e revogação de nó) | [[f07-aprovacao-de-no]] |
| S3a e S3b (operador e credencial por nó) | [[f07-operador-e-credencial-por-no]] |
| S4 (TLS) | [[f07-tls]] |
| J1 (gateway e API) | [[f08-gateway-e-api]] |
| J2 e J4 (engine, tokenizador, KV cache) | [[f08-engine-local]] |
| J3 (chat completions) | [[f08-chat-completions]] |
| J5 (canal residente de fatias) | [[f08-canal-residente-de-fatias]] |

## Marcos

| Marco | Fase |
|---|---|
| M0 — compute em dois nós | [[fase-00-fundacao]] |
| M1 — DAG tolerante a falhas | [[fase-03-execucao-distribuida]] |
| M2 — provisionamento automático | [[fase-15-ai-cluster-os]] |
| M3 — inteligência de storage | [[fase-10-inteligencia-de-storage]] |
| M4 — protótipo LLM em CPU | [[fase-12-model-runtime]] |
| M5 — otimização de borda | [[fase-14-edge-optimizer]] |
| M6 — protótipo heterogêneo | [[fase-16-aceleradores]] |
| M7 — worker móvel | [[fase-18-worker-movel]] |

Detalhe dos marcos em [[marcos]].

> [!note] J4
> O código J4 não aparece nos documentos antigos; o conteúdo foi atribuído por inferência ao histórico de commits da sequência J (KV cache, segunda família de modelo e streaming) e está documentado nos slices da [[fase-08-inferencia]].
