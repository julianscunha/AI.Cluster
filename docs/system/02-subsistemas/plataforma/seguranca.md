---
title: "19 — Security & Trust"
type: spec
status: draft
tags: [spec, security]
---

# 19 — Segurança e confiança

## Identidade

Separar:

- physical device identity;
- logical node identity;
- cluster certificate.

## Registro

Novo node começa UNTRUSTED.

## Autenticação

PROPOSED: mTLS no control plane e credenciais de workload para data-plane access.

## Autorização

Node só acessa:

- Work Units autorizadas;
- model shards autorizados;
- state objects autorizados.

## Atestação

Quando disponível, validar:

- OS image;
- kernel/profile;
- worker version;
- driver state;
- integrity.

## Quarentena

Pode ser acionada por:

- integrity mismatch;
- comportamento inesperado;
- credenciais comprometidas.

## Revogação

Revogar certificado, invalidar sessões e remover acesso a modelo/estado.
