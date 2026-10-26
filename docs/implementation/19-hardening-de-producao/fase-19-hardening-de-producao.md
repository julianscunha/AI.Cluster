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
- [~] mTLS. [[f19-mtls]]: certificado de cliente exigido por `AI_CLUSTER_TLS_CLIENT_CA`. Falta ligar certificado a `node_id`, rotação e revogação.
- [ ] Attestation de nó. Não existe.
- [~] Upgrade e rollback. [[f19-upgrade-e-rollback]]: troca do binário com gate de saúde e rollback local, validado em hardware real. Falta o rollout automático pelo controller.
- [~] Auditoria. Os eventos são um ring buffer em memória (`events.rs`). Não há trilha persistente de approve, revoke e leave.
- [~] Injeção de falhas. [[f19-injecao-de-falha]]: kill e pause em rodízio sob carga (`phase-t-soak.sh`). Faltam partição de rede e controller reiniciado.
- [~] Soak. `phase-t-soak.sh` com `ROUNDS` configurável; falta uma rodada longa (horas) com medição de recurso.
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
