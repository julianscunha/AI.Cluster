# 01 — Overview

## Purpose

Definir o escopo do AI Cluster como um runtime distribuído e adaptativo para IA.

## Core idea

Uma coleção heterogênea de equipamentos Linux deve se comportar como um pool lógico de recursos, capaz de executar uma única requisição distribuindo um modelo e suas operações entre diferentes dispositivos.

## Primary workloads

- LLM inference;
- embeddings;
- reranking;
- context processing;
- multimodal preprocessing;
- compression;
- deterministic compute;
- custom AI graphs.

## Operational properties

- zero/low-touch onboarding;
- reimaging;
- hot-plug;
- health-aware scheduling;
- performance-aware scheduling;
- graph-assisted retrieval;
- adaptive token optimization;
- detailed TUI.

## Out of scope for v0.1

- arbitrary mobile hardware support;
- universal NPU support;
- transparent migration of arbitrary process memory;
- guarantee of linear scaling;
- replacing mature LLM kernels with a brand-new kernel compiler on day one.
