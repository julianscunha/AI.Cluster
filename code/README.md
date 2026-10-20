---
title: "AI Cluster — Esqueleto da Fase 0"
type: operational
status: living
tags: [operational, readme]
---

# AI Cluster — Esqueleto da Fase 0

Este é o esqueleto de implementação da Fase 0 do projeto AI Cluster.

O primeiro objetivo não é inferência de LLM. É validar o substrato de execução distribuída:

- protocolo Controller/Worker
- identidade de nó
- heartbeat
- reporte de capabilities
- ciclo de vida da Work Unit
- abstração de transporte
- harness de benchmark
- fronteira CLI/TUI

Nenhum runtime de LLM de produção está incluído nesta fase.
