---
title: "Substrato"
aliases: ["Substrato", "Camada de hardware", "docs/04-08", "docs/12"]
tags: [sistema/moc, subsistema/substrato]
type: sistema/moc
status: vivo
implementacao: parcial
fases: ["[[fase-02-capability-e-saude]]", "[[fase-03-execucao-distribuida]]", "[[fase-08-inferencia]]"]
---

# Substrato

O substrato é a camada que transforma máquinas heterogêneas em workers com capacidade medida, saúde observada, confiança explícita e um caminho de dados até o controller. É a base sobre a qual [[execucao]], [[dados]] e [[ia]] rodam. Voltar para [[subsistemas]].

## Notas

- [[no-e-hardware]]: separação entre `Device` e `Node`, descoberta de hardware, classes de equipamento e a interface de worker comum.
- [[capability-engine]]: baseline, capacidade corrente e confiança das medidas que alimentam o placement.
- [[saude-e-confianca]]: saúde operacional (domínios e estados) separada de confiança (autorização e integridade).
- [[worker-runtime]]: componentes do worker, backends de hardware, escalonamento local e o canal residente de fatias.
- [[work-unit]]: a menor unidade atribuível pelo scheduler, com campos, requisitos, ciclo de vida e idempotência.
- [[camada-de-transporte]]: transportes (gRPC/TCP hoje), plano de controle × plano de dados, métricas de link e decisão de transferência.
