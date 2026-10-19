---
title: "AI Cluster — Distributed AI Runtime"
type: system
status: draft
tags: [system, readme]
---

# AI Cluster — Distributed AI Runtime

Status: Architecture Draft v0.1

Este repositório contém a especificação arquitetural inicial do **AI Cluster**, uma plataforma Linux-first para transformar hardware heterogêneo reaproveitado em um pool distribuído de computação para IA.

## Visão

O sistema deve:

- executar uma única inferência distribuindo um modelo robusto entre múltiplos dispositivos;
- aceitar hardware heterogêneo: x86_64, ARM64, CPU, GPU, NPU, PCs, servidores, notebooks, SBCs e dispositivos móveis compatíveis;
- provisionar nós automaticamente e convertê-los para um AI Cluster OS próprio;
- suportar hot-plug, drain, falha e replanejamento sem derrubar o cluster;
- medir capacidade real, saúde, temperatura, armazenamento, rede e desempenho;
- otimizar tokens e computação na borda antes de acionar modelos grandes;
- usar grafos para topologia, conhecimento, dependências, execução e observabilidade;
- tratar comunicação, memória e armazenamento como recursos de primeira classe;
- fornecer uma TUI/CLI operacional em shell;
- expor capacidade de inferência somente por uma Inference API;
- manter Application Plane e Compute Plane desacoplados.
- aprender com desempenho observado e recalibrar previsões do cluster.

## Fonte de verdade

`ARCHITECTURE.md` é o documento mestre. Os demais arquivos aprofundam cada subsistema.

As decisões usam os estados:

- **DECIDED** — decisão adotada para a arquitetura atual.
- **PROPOSED** — proposta ainda sujeita a validação.
- **OPEN** — questão em aberto.
- **EXPERIMENTAL** — precisa de benchmark/prova de conceito.
- **IMPLEMENTED** — implementado e validado.

## Documentos principais

- [Mapa do repositório (MOC)](DIRECTORY-MAP.md) — classificação, sequência de leitura otimizada e convenção Obsidian.
- `ARCHITECTURE.md` — arquitetura geral.
- `REQUIREMENTS.md` — requisitos funcionais e não funcionais.
- `PRINCIPLES.md` — princípios de projeto.
- `docs/` — especificação de cada subsistema, incluindo o Inference Engine e a Inference API.
- `protocol/` — protocolo Controller/Worker.
- `data-model/` — entidades, relações e projeção em grafo.
- `implementation/` — roadmap, fases e estratégia de testes.
- `adr/` — decisões arquiteturais.
- `code/phase-0/` — workspace Rust executável da Fase 0; seus documentos operacionais ficam em `code/phase-0/docs/`.

## Regra operacional

Nenhum detalhe de implementação deve ser tratado como definitivo apenas por ter sido proposto. Quando uma decisão for validada por benchmark, teste ou experimento, atualize o documento correspondente e registre o ADR.
