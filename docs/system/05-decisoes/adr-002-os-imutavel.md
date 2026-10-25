---
title: "ADR-002 — OS imutável do AI Cluster"
aliases: ["ADR-002", "ADR-002 — Immutable AI Cluster OS"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
status: proposto
implementacao: nao-iniciado
fases: ["[[fase-15-ai-cluster-os]]", "[[fase-19-hardening-de-producao]]"]
---

# ADR-002 — OS imutável do AI Cluster

Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

Um nó que acumula configuração manual diverge dos outros (configuration drift), e corrigir isso dá trabalho. Imagens versionadas tornam o estado de cada nó reproduzível e a reversão simples.

## Decisão

Usar imagem versionada e imutável, com reimage inicial e atualização A/B.

## Consequências

Evita configuration drift e facilita rollback e recuperação.

## Alternativas

- Configuração mutável por nó: não registrada no ADR original; a justificativa é o drift que a decisão evita.

> [!info] O que já existe
> Nada. `code/os/` contém só o README.

> [!todo] Reimage, atualização A/B e rollback ficam em [[fase-15-ai-cluster-os]] e [[fase-19-hardening-de-producao]]. Ver [[provisionamento]].
