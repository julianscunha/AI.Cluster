# 19 — Security & Trust

## Identity

Separar:

- physical device identity;
- logical node identity;
- cluster certificate.

## Enrollment

Novo node começa UNTRUSTED.

## Authentication

PROPOSED: mTLS no control plane e credenciais de workload para data-plane access.

## Authorization

Node só acessa:

- Work Units autorizadas;
- model shards autorizados;
- state objects autorizados.

## Attestation

Quando disponível, validar:

- OS image;
- kernel/profile;
- worker version;
- driver state;
- integrity.

## Quarantine

Pode ser acionada por:

- integrity mismatch;
- comportamento inesperado;
- credenciais comprometidas.

## Revocation

Revogar certificado, invalidar sessões e remover acesso a modelo/estado.
