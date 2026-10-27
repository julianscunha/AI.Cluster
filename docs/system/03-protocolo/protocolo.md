---
title: "Protocolo"
aliases: ["protocol", "Protocolo"]
tags: [sistema/moc]
type: sistema/moc
---

# Protocolo

Como controller, workers, gateway e operador se falam: transporte, quem inicia cada conversa, identidade, catálogo de mensagens, versionamento e o canal residente de fatias. Pasta do [[system]].

## Notas

- [[protocolo-controller-worker]]: transporte gRPC/TLS, quem inicia cada interação, identidade por token e credencial, metadados e streaming.
- [[mensagens]]: os 2 serviços e 24 RPCs do schema, com quem chama, quem pode chamar e os campos de cada mensagem.
- [[versionamento]]: versão `v1`, regras de evolução do schema e atualização gradual dos nós (releases).
- [[canal-de-fatias]]: canal residente em que o worker abre `ShardChannel` e recebe `ShardCompute`; framing, sessão, limites, erros e allowlist de projeções.

Decisões relacionadas: [[adr-003-controller-worker]], [[adr-009-canal-residente-de-fatias]], [[adr-010-papeis-de-credencial]], [[adr-016-mtls-opcional-no-plano-de-controle]].
