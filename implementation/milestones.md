---
title: "Milestones"
type: implementation
status: living
tags: [implementation, milestones]
---

# Milestones

Legenda: `[x]` executado e validado · `[~]` parcial · `[ ]` não iniciado.

## M0 — Prova de compute em dois nós

Aceite:

- [x] dois nós Linux;
- [x] descobertos pelo Controller;
- [x] um envia uma operação de matriz para o outro;
- [x] telemetria registrada — eventos (`telemetry.md`) e métricas agregadas (`metrics.md`).

Ressalva: validado com dois containers no mesmo host. Duas máquinas físicas ainda não
foram rodadas (`phase-0/two-machine-validation.md`).

## M1 — DAG tolerante a falhas em três nós

Aceite:

- [x] DAG com branches paralelas;
- [x] um nó pode drenar;
- [x] um nó pode falhar e a execução recupera.

Evidência: `phase-2/dependency-graph.md`, `phase-2/fault-tolerance.md`,
E2E `phase-2-failure.sh`.

## M2 — Provisionamento automático

Aceite:

- [ ] máquina de teste limpa faz bootstrap;
- [ ] imagem instalada automaticamente;
- [ ] nó reingressa como READY.

— não iniciado (Fase 3 do roadmap).

## M3 — Inteligência de storage

Aceite:

- [ ] HDD/SSD/NVMe diferenciados;
- [ ] políticas aplicadas dinamicamente;
- [ ] dashboard mostra saúde do storage.

— não iniciado (Fase 4 do roadmap; `persistence.md` é só o começo).

## M4 — Protótipo LLM em CPU

Aceite:

- [ ] modelo dividido entre vários nós CPU;
- [ ] métricas de inferência esperado/real coletadas;
- [ ] overhead de comunicação visível.

— não iniciado (Fase 6 do roadmap).

## M5 — Otimização de borda

Aceite:

- [ ] benchmark de redução de tokens;
- [ ] benchmark de cache hit;
- [ ] benchmark de recuperação de grafo;
- [ ] overhead do otimizador medido.

— não iniciado (Fase 5 do roadmap).

## M6 — Protótipo heterogêneo

Aceite:

- [ ] cluster x86 + ARM;
- [ ] capacidades diferentes;
- [ ] scheduler usa scores por workload.

— não iniciado. Depende do scheduler (Fase 2, `placement`).

## M7 — Worker móvel

Aceite:

- [ ] celular compatível reimaged;
- [ ] conectado via USB;
- [ ] contribui com compute mensurável sem comprometer a estabilidade do cluster.

— não iniciado (Fase 7 do roadmap).
