---
title: "F19 — Hardening de produção"
aliases: ["Fase 9 — Production hardening", "Fase 9", "roadmap 9"]
tags: [implementacao/fase/19, implementacao/estado/planejado]
type: implementacao/fase
status: planejado
codigos-antigos: ["roadmap 9", "docs/19-security (parte de produção)"]
sistema: ["[[seguranca]]", "[[observabilidade]]", "[[ai-cluster-os]]", "[[adr-010-papeis-de-credencial]]"]
---
# F19 — Hardening de produção

## Objetivo

Endurecer o cluster para uso contínuo: mTLS, attestation de nó, upgrade e rollback, trilha de auditoria, injeção de falhas e soak tests de longa duração.

## Depende de

- [[fase-07-seguranca-base]]: TLS, token de join, aprovação, revogação e credencial por nó.
- [[fase-15-ai-cluster-os]]: upgrade e rollback dependem de A/B.

## Entrega

- mTLS entre controller, workers e gateway.
- Attestation de nó.
- Upgrade e rollback.
- Trilha de auditoria de ações de operador.
- Injeção de falhas sistemática.
- Soak tests.

## Checklist

Verificado em `code/crates/protocol/src/metadata.rs` (TLS e comentário sobre mTLS), `controller/src/registry.rs` (aprovação e revogação), `controller/src/events.rs` (eventos em memória) e `code/tests/*.sh`.

- [x] TLS do canal (S4). Certificado e CA de servidor, herdados pelo cliente. Entregue em [[fase-07-seguranca-base]].
- [x] Token de join, aprovação de nó, revogação e credencial por nó (S1, S2, S3, S5). Entregue em F07.
- [ ] mTLS. Não existe; o próprio código marca mTLS como o próximo degrau.
- [ ] Attestation de nó. Não existe.
- [ ] Upgrade e rollback. Não existe.
- [~] Auditoria. Os eventos são um ring buffer em memória (`events.rs`). Não há trilha persistente de approve, revoke e leave.
- [~] Injeção de falhas. Há E2E ad hoc: worker morto em `phase-2-failure.sh` e em meio à geração em `phase-j-shards.sh`. Não há framework.
- [ ] Soak tests de longa duração. Não existem.
- [ ] TLS na perna HTTP do gateway. Pendente, também listado em [[fase-11-request-manager]].

## Gate

Gate padrão (EP §19): código, benchmarks, telemetria, testes de falha, atualização de documentação e ADR quando a arquitetura mudar.

Gate específico (manifesto): soak e failure injection passando.

## Slices

Notas desta pasta (esta nota é o MOC): nenhum slice criado.

Slices a criar:

- `f19-mtls`.
- `f19-attestation`.
- `f19-upgrade-e-rollback`.
- `f19-trilha-de-auditoria`.
- `f19-injecao-de-falha`.
- `f19-soak`.

## Códigos antigos

- Roadmap 9 (security hardening, attestation, upgrade/rollback, audit, failure injection, long-run soak tests).
- Parte de produção de `docs/19-security`, hoje em [[seguranca]].

## Specs do sistema

- [[seguranca]]: papéis, credenciais e transporte.
- [[observabilidade]]: eventos, métricas e trilha.
- [[ai-cluster-os]]: upgrade e rollback da imagem.
- [[adr-010-papeis-de-credencial]]: papéis de credencial.
