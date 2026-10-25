---
title: "F07 — Segurança base"
aliases: ["Fase S", "fase-s", "S1–S5", "segurança base"]
tags: [implementacao/fase/07, implementacao/estado/implementado]
type: implementacao/fase
status: implementado
codigos-antigos: ["S1", "S2", "S3a", "S3b", "S4", "S5", "trust"]
sistema: ["[[seguranca]]", "[[saude-e-confianca]]"]
---
# F07 — Segurança base

Esta é a nota da fase e o MOC de `07-seguranca-base/`.

## Objetivo

Nó desconhecido não recebe trabalho e nenhuma RPC é aceita sem credencial. Cada nó tem sua
credencial, os papéis de administração são separados, o canal é cifrado e a aprovação e a
revogação sobrevivem a restart. Gate do requisito NFR-011: nó desconhecido não recebe trabalho.

## Depende de

- [[fase-01-nucleo-do-cluster]] (registro de nó, join e leave)
- [[fase-06-estado-e-recuperacao]] (aprovação e credenciais persistidas no estado)

## Entrega

- Token de join em toda RPC, fail-closed.
- Aprovação manual de nó novo, com `approve` e `revoke`.
- Token de operador separado e credencial por nó emitida no registro.
- TLS de um sentido nos dois serviços de gRPC.
- Persistência de aprovação e credenciais (`AI_CLUSTER_NODES`, `AI_CLUSTER_NODE_CREDENTIAL_FILE`).

## Checklist

| Estado | Item | Slice |
| --- | --- | --- |
| [x] | S1 token de join obrigatório em toda RPC (fail-closed) | [[f07-token-de-join]] |
| [x] | S2 aprovação manual de nó novo (`pendente` / `aprovado`) | [[f07-aprovacao-de-no]] |
| [x] | S3a token de operador separado para RPCs administrativas | [[f07-operador-e-credencial-por-no]] |
| [x] | S3b credencial por nó emitida no registro | [[f07-operador-e-credencial-por-no]] |
| [x] | S4 TLS de um sentido no canal gRPC | [[f07-tls]] |
| [x] | S5 persistência da aprovação e revogação (`revoke`, rotação de credencial) | [[f07-aprovacao-de-no]] |
| [ ] | Evidência E2E de negação de papel (nó tentando RPC de operador) | [[f07-operador-e-credencial-por-no]] |
| [ ] | mTLS e attestation de nó | pendente, [[fase-19-hardening-de-producao]] |
| [ ] | Rotação periódica e revogação de operador e de tokens de join | pendente, [[fase-19-hardening-de-producao]] |
| [ ] | Segredos fora de claro (arquivo em claro, variável visível em `docker inspect`) | pendente, [[fase-19-hardening-de-producao]] |
| [ ] | Níveis intermediários de trust (quarentena por confiança) | pendente, [[fase-02-capability-e-saude]] |
| [ ] | Auditoria e exposição de leitura conforme o [[seguranca]] | pendente, [[fase-19-hardening-de-producao]] |

Legenda: `[x]` implementado e validado · `[~]` parcial · `[ ]` não iniciado.

## Gate

NFR-011 (nó desconhecido não recebe trabalho): **atendido**. Nó pendente não ganha unidade, não
entra no ranking e não conta como disponível; nó revogado não volta aprovado com aprovação manual.

| Item do gate padrão (EP §19) | Situação |
| --- | --- |
| Código | feito (`protocol/src/metadata.rs`, `controller/src/service.rs`, `registry.rs`, CLI, worker) |
| Benchmark | não aplicável: a fase não tem métrica de desempenho; a medição de rede (C2) inclui o custo do TLS |
| Telemetria | feita: eventos `node_approved` e `node_revoked` |
| Teste de falha | feito: join errado, nó pendente, impostor, revogação e TLS sem certificado |
| Doc | feito (este conjunto) |
| ADR | feito: [[adr-010-papeis-de-credencial]] |

## Slices

- [[f07-token-de-join]]: S1, segredo de join em toda RPC.
- [[f07-aprovacao-de-no]]: S2 e S5, aprovação, persistência e revogação.
- [[f07-operador-e-credencial-por-no]]: S3a e S3b, papéis e credencial por nó.
- [[f07-tls]]: S4, TLS.

## Códigos antigos

- S1 (join token), S2 (node approval), S3a (token de operador), S3b (credencial por nó), S4 (TLS),
  S5 (persistência e revogação).
- `trust` do EP §8 (G1, filtragem de candidatos): parcial até a S2; os níveis intermediários
  continuam pendentes.

## Specs do sistema

- [[seguranca]]: identidades, papéis de credencial, registro e aprovação, transporte, revogação.
- [[adr-010-papeis-de-credencial]]: decisão sobre os papéis de credencial.
- [[saude-e-confianca]]: confiança e quarentena (alvo).

> [!info] O que já existe
> Token de join em toda RPC; aprovação de nó novo com `approve` e `revoke`; token de operador e
> de inferência separados; credencial por nó; TLS de um sentido; persistência de aprovação e
> credenciais. Tudo em `code/crates/protocol` e `code/crates/controller`.

> [!todo] O que falta
> mTLS e attestation (F19), rotação periódica, revogação de operador, segredos fora de claro,
> níveis intermediários de confiança e um E2E de negação de papel.
