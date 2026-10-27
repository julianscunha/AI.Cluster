---
title: "ADR-001 — Base Linux"
aliases: ["ADR-001", "ADR-001 — Linux Base"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
---

# ADR-001 — Base Linux

Status: proposto · Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

O AI Cluster OS precisa de um sistema operacional-base. Justificativas: drivers, suporte a ARM64 e x86_64, ecossistema de hardware, boot e recuperação, e menor custo inicial.

## Decisão

Construir o AI Cluster OS sobre uma base Linux madura, sem escrever um kernel ou sistema inteiro do zero.

## Consequências

O diferencial fica no runtime, no provisionamento, no scheduler, no storage intelligence e na observabilidade, e não no kernel.

## Alternativas

- Escrever kernel e sistema do zero: descartado pela decisão e pelo custo inicial.

## Limites

- A distribuição exata segue aberta. A imagem armv7 atual usa Alpine ([[adr-017-instalacao-a-b-e-boot-de-rede]]), sem decisão formal sobre a base geral.
- Imagem base, provisionamento e boot: ver [[ai-cluster-os]] e [[provisionamento]].
