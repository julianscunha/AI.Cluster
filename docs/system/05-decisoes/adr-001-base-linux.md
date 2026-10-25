---
title: "ADR-001 — Base Linux"
aliases: ["ADR-001", "ADR-001 — Linux Base"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
status: proposto
implementacao: nao-iniciado
fases: ["[[fase-15-ai-cluster-os]]"]
---

# ADR-001 — Base Linux

Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

O AI Cluster OS precisa de um sistema operacional-base. Justificativas registradas: drivers, suporte a ARM64 e x86_64, ecossistema de hardware, boot e recuperação, e menor custo inicial.

## Decisão

Construir o AI Cluster OS sobre uma base Linux madura, sem escrever um kernel ou sistema inteiro do zero.

## Consequências

O diferencial fica no runtime, no provisionamento, no scheduler, no storage intelligence e na observabilidade, e não no kernel.

## Alternativas

- Escrever kernel e sistema do zero: descartado pela decisão e pelo custo inicial.

> [!info] O que já existe
> Nada. `code/os/` contém só o README.

> [!todo] Imagem base, provisionamento e boot ficam em [[fase-15-ai-cluster-os]] e [[ai-cluster-os]].
