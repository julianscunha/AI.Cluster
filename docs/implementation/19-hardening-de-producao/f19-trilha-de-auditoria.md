---
title: "F19 — Trilha de auditoria"
aliases: ["audit", "auditoria de operador"]
tags: [implementacao/fase/19, implementacao/fatia, implementacao/estado/implementado]
type: implementacao/fatia
status: implementado
superado: false
fase: "[[fase-19-hardening-de-producao]]"
sistema: ["[[observabilidade]]"]
evidencia: ["code/tests/phase-s3-roles.sh"]
---
# F19 — Trilha de auditoria

- [ ] Superado

## Objetivo

Saber quem fez o quê no cluster: cada ação de operador deixa um rastro que sobrevive a restart.

## O que existe

- `operator_peer` valida o token de operador e devolve o endereço do peer; `ClusterControlService::audit` grava o evento `audit` com ação, alvo e peer.
- Ações cobertas: `approve`, `revoke`, `enqueue_work`, `enqueue_graph`, `cancel_request`, `reserve`, `release_reservation`. `GetOutput` (leitura) não audita.
- Vai para o mesmo log de eventos, então `AI_CLUSTER_EVENTS` ([[f06-eventos-persistidos]]) a mantém entre restarts.
- Tentativas negadas (token errado) não entram na trilha: falham antes da ação. `phase-s3-roles.sh` confere isso.

## Limites

- O peer é o endereço de rede, não uma identidade; identidade por operador vem com mTLS (F19, pendente).
- A trilha não é à prova de adulteração: é o log de eventos, sem assinatura.
