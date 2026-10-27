---
title: "ADR-002 — OS imutável do AI Cluster"
aliases: ["ADR-002", "ADR-002 — Immutable AI Cluster OS"]
tags: [sistema/adr, subsistema/plataforma]
type: sistema/adr
---

# ADR-002 — OS imutável do AI Cluster

Status: proposto · Data: 2026-10-19 · Voltar para [[decisoes]].

## Contexto

Um nó que acumula configuração manual diverge dos outros (configuration drift), e corrigir isso dá trabalho. Imagens versionadas tornam o estado de cada nó reproduzível e a reversão simples.

## Decisão

Usar imagem versionada e imutável, com reimage inicial e atualização A/B.

## Consequências

- Evita configuration drift e facilita rollback e recuperação.
- A atualização de versão passa por release assinada conferida no nó ([[adr-018-release-assinada-e-medida-do-slot]]).

## Alternativas

- Configuração mutável por nó: a justificativa é o drift que a decisão evita.

## Limites

- Raiz somente leitura e slots A/B existem hoje só na imagem armv7 ([[adr-017-instalacao-a-b-e-boot-de-rede]]). x86_64 e ARM64 ainda não têm imagem.
- Reimage, atualização A/B e rollback: ver [[provisionamento]].
