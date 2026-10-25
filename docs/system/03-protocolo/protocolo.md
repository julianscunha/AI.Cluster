---
title: "Protocolo"
aliases: ["protocol", "Protocolo"]
tags: [sistema/moc]
type: sistema/moc
status: vivo
implementacao: parcial
fases: ["[[fase-00-fundacao]]", "[[fase-01-nucleo-do-cluster]]", "[[fase-08-inferencia]]"]
---

# Protocolo

Como controller, workers e gateway se falam: transporte, quem inicia cada conversa, identidade, catálogo de mensagens, versionamento e o canal residente de fatias. Pasta do [[system]].

## Notas

- [[protocolo-controller-worker]]: transporte gRPC/TLS, fluxo de interações, identidade por token e credencial, metadados e streaming.
- [[mensagens]]: os 2 serviços e 22 RPCs de `code/proto/cluster.proto`, com quem chama, autorização e campos de cada mensagem.
- [[versionamento]]: versão `v1`, regras de evolução do schema, campos novos com efeito semântico e upgrade gradual.
- [[canal-de-fatias]]: canal residente em que o worker abre `ShardChannel` e recebe `ShardCompute`; framing, sessão, limites, erros e allowlist de projeções.

Decisões relacionadas: [[adr-003-controller-worker]], [[adr-009-canal-residente-de-fatias]].
