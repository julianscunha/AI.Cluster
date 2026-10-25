---
title: "Protocolo Controller ↔ Worker"
type: protocol
status: proposed
tags: [protocol, controller-worker]
---

# Protocolo Controller ↔ Worker

Status: PROPOSTO

## Transporte

gRPC + Protobuf é a opção inicial para control plane.

## Mensagens de ciclo de vida

```text
HELLO
REGISTER
CAPABILITIES
BENCHMARK_REQUEST
BENCHMARK_RESULT
HEARTBEAT
HEALTH_REPORT
TELEMETRY
LOAD_MODEL
UNLOAD_MODEL
LOAD_SHARD
UNLOAD_SHARD
ASSIGN_WORK
CANCEL_WORK
CHECKPOINT
RESTORE
DRAIN
LEAVE
```

## Requisitos de mensagem

Cada mensagem deve possuir:

- protocol version;
- node_id;
- correlation id;
- timestamp/sequência monotônica quando aplicável;
- authentication context;
- idempotency key para operações mutáveis.

### Estado de implementação (Fase 0)

O `node_id` viaja em cada payload protobuf. Os demais campos são carregados como
metadata de requisição gRPC, definidos por um interceptor para que nenhum ponto de chamada os repita:

| campo | header | estado |
|---|---|---|
| protocol version | `x-ai-cluster-protocol-version` | emitido pelos clientes, imposto pelo controller |
| correlation id | `x-ai-cluster-correlation-id` | valor novo a cada chamada |
| authentication context | `x-ai-cluster-auth` | placeholder `anonymous` até a fase de trust |
| idempotency | — | estrutural: uma unidade de trabalho sai da tabela de dispatch ao completar, então uma submissão duplicada não tem o que completar |

O controller rejeita qualquer chamada cuja versão de protocolo não bata com a sua.

## Streaming

Telemetria e transferências longas devem suportar streaming/backpressure.

## Compatibilidade

O Controller deve conhecer as capabilities/features suportadas antes de enviar determinada operação.
