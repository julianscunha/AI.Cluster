---
title: "Marcos"
aliases: ["Milestones", "implementation/milestones", "Marcos M0–M7"]
tags: [implementacao/transversal, implementacao/marcos]
type: implementacao/transversal
status: em-andamento
---

# Marcos

Marcos são provas de sistema: cada um reúne critérios de várias fases e só fecha quando o critério é executado. A fase que entrega cada marco está ao lado do título. Estado por fase em [[status]]; visão de dependências em [[implementation]].

Legenda: `[x]` executado e validado · `[~]` parcial · `[ ]` não iniciado.

## M0 — Prova de compute em dois nós

Fase: [[fase-00-fundacao]]

- [x] dois nós Linux;
- [x] descobertos pelo Controller;
- [x] um envia uma operação de matriz para o outro;
- [x] telemetria registrada — eventos e métricas agregadas ([[f05-eventos]], [[f05-metricas]]).

Ressalva: validado com dois containers no mesmo host. Duas máquinas físicas ainda não foram rodadas ([[f00-runbook-duas-maquinas]]).

## M1 — DAG tolerante a falhas em três nós

Fase: [[fase-03-execucao-distribuida]]

- [x] DAG com branches paralelas ([[f03-grafo-de-dependencias]]);
- [x] um nó pode sair de forma planejada (SIGTERM, `code/tests/phase-1-leave.sh`);
- [x] um nó pode falhar e a execução recupera ([[f03-tolerancia-a-falhas]], `code/tests/phase-2-failure.sh`).

## M2 — Provisionamento automático

Fase: [[fase-15-ai-cluster-os]]

- [ ] máquina de teste limpa faz bootstrap;
- [ ] imagem instalada automaticamente;
- [ ] nó reingressa como READY.

Não iniciado. O AI Cluster OS (EP J1–J6) não tem código além da fronteira em `code/os/README.md`.

## M3 — Inteligência de storage

Fase: [[fase-10-inteligencia-de-storage]]

- [x] HDD/SSD/NVMe diferenciados — classes em bloco por nó ([[f10-classes-de-armazenamento]], `code/tests/phase-i-storage.sh`);
- [ ] políticas aplicadas dinamicamente;
- [ ] dashboard mostra saúde do storage.

Parcial: a classificação existe; medição de latência, SMART e alocação por classe não.

## M4 — Protótipo LLM em CPU

Fase: [[fase-12-model-runtime]] (com trabalho já feito em [[fase-08-inferencia]])

- [~] modelo dividido entre vários nós CPU — só as projeções do GPT-2 e da SmolLM2 são distribuídas em workers ([[f08-canal-residente-de-fatias]], `code/tests/phase-j-shards.sh`);
- [ ] métricas de inferência esperado/real coletadas — não há TTFT, ITL nem tokens/s;
- [~] overhead de comunicação visível — o gateway mede o tempo por chamada (rede + relay + cálculo), sem decompor no trace.

## M5 — Otimização de borda

Fase: [[fase-14-edge-optimizer]]

- [ ] benchmark de redução de tokens;
- [ ] benchmark de cache hit;
- [ ] benchmark de recuperação de grafo;
- [ ] overhead do otimizador medido.

Não iniciado. Depende de F11, F12 e F13.

## M6 — Protótipo heterogêneo

Fase: [[fase-16-aceleradores]]

- [ ] cluster x86 + ARM;
- [ ] capacidades diferentes;
- [ ] scheduler usa scores por workload.

Não iniciado. O placement por custo já existe ([[f04-placement]]); faltam ARM64 e scores por workload.

## M7 — Worker móvel

Fase: [[fase-18-worker-movel]]

- [ ] celular compatível reimaged;
- [ ] conectado via USB;
- [ ] contribui com compute mensurável sem comprometer a estabilidade do cluster.

Não iniciado. Depende de F09 (USB), F15 (reimage) e F16.
