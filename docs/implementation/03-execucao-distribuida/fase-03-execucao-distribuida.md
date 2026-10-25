---
title: "F03 — Execução distribuída"
aliases: ["Fase 2 — Execução distribuída", "Fase D — Work Unit Runtime", "Fase F — Execution Graph", "roadmap 2", "phase-2"]
tags: [implementacao/fase/03, implementacao/estado/implementado]
type: implementacao/fase
status: implementado
codigos-antigos: ["D1", "D2", "D3", "F1", "F2", "F4", "roadmap-2", "M1", "EP §20"]
sistema: ["[[work-unit]]", "[[execution-graph]]", "[[hotplug-e-recuperacao]]", "[[camada-de-transporte]]", "[[modelo-de-desempenho]]"]
---

# F03 — Execução distribuída

## Objetivo

Executar uma Work Unit de ponta a ponta entre controller e workers, com grafo de dependências (DAG), paralelismo entre branches e recuperação quando um nó morre. A decisão de placement (F3 do plano antigo) foi separada e está em [[fase-04-escalonamento]].

## Depende de

- [[fase-01-nucleo-do-cluster]] (controller, registry, worker agent)
- [[fase-02-capability-e-saude]] (capability e saúde usadas pelo placement)

## Entrega

| Item | Status | Onde |
|---|---|---|
| Work Unit (schema, execução local, integridade do resultado) | [x] | [[f03-grafo-de-dependencias]] |
| Execution Graph (schema, DAG executor com branches paralelas) | [x] | [[f03-grafo-de-dependencias]] |
| Retry e recovery (reatribuição limitada, cascata de falha) | [x] | [[f03-tolerancia-a-falhas]] |
| Medição compute vs transferência (primeiro alvo experimental) | [x] | [[f03-varredura-compute-vs-transferencia]] |
| Placement (filtro, custo, histerese) | [x] | [[fase-04-escalonamento]] |
| Transport abstraction | [~] | gRPC direto; abstração é [[fase-09-transporte]] |

## Checklist

Fonte: EP §5 (D1–D3), §7 (F1, F2, F4), roadmap Fase 2, M1 e EP §20.

- [x] D1 Work Unit schema: estado, requirements, dependencies, retries.
- [x] D2 Execução local de operações CPU simples.
- [x] D3 Integridade do resultado: checksum, status e observabilidade.
- [x] Critério D: uma Work Unit circula entre Controller e Worker e devolve resultado validado.
- [x] F1 Graph schema: nodes, edges, dependencies, barriers.
- [x] F2 DAG executor: branches em paralelo.
- [x] F4 Retry e recovery: reatribuir Work Unit falha.
- [x] Critério F: perder um worker não corrompe o DAG.
- [x] Roadmap 2: Work Unit, Execution Graph e retries.
- [x] M1 aceite: DAG com branches paralelas; um nó pode drenar; um nó pode falhar e a execução recupera (`phase-2-failure.sh`).
- [x] EP §20: varredura 1/2/3 nós × 32–256 executada.
- [~] Transport abstraction: só gRPC/TCP.

## Gate

Gate padrão (EP §19): código, benchmark, telemetria, teste de falha, doc e ADR.

- [x] Código: `code/crates/controller/src/work.rs`, `code/proto/cluster.proto`.
- [x] Benchmark: `code/tests/scaling-sweep.sh`.
- [x] Telemetria: eventos e traces de [[fase-05-observabilidade]] (`work_enqueued`, `work_completed`, `GetTrace`).
- [x] Teste de falha: `code/tests/phase-2-failure.sh` (worker morto no meio da unidade).
- [x] Doc: os quatro slices desta pasta.
- [ ] ADR: nenhum ADR específico da fase identificado em [[decisoes]]; confirmar se algum é necessário.

> [!warning] Gate parcial no item de transporte
> A abstração de transporte não existe ainda. A fase fecha sem ela porque o critério de saída (DAG em vários nós com recuperação) não depende dela; a abstração pertence a [[fase-09-transporte]].

## Slices

- [[f03-grafo-de-dependencias]]: unidades com `depends_on` e bindings de input; EnqueueGraph; diamante com fluxo de dados.
- [[f03-tolerancia-a-falhas]]: orçamento de tentativas, cascata de falha, heartbeat desacoplado do laço de trabalho.
- [[f03-varredura-compute-vs-transferencia]]: varredura de tamanhos e nós; achado da cadência de poll; teto do transporte.

## Códigos antigos

- D1, D2, D3 (Fase D — Work Unit Runtime)
- F1, F2, F4 (Fase F — Execution Graph). F3 virou [[fase-04-escalonamento]].
- roadmap 2 (parte: Work Unit, Execution Graph, retries)
- M1 (DAG tolerante a falhas em três nós, em [[marcos]])
- EP §20 (primeiro alvo experimental; antigo `phase-0/benchmark-sweep`)

## Specs do sistema

- [[work-unit]]
- [[execution-graph]]
- [[hotplug-e-recuperacao]]
- [[camada-de-transporte]]
- [[modelo-de-desempenho]]
