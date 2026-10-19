# Controller ↔ Worker Protocol

Status: PROPOSED

## Transport

gRPC + Protobuf é a opção inicial para control plane.

## Lifecycle messages

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

## Message requirements

Cada mensagem deve possuir:

- protocol version;
- node_id;
- correlation id;
- timestamp/monotonic sequence quando aplicável;
- authentication context;
- idempotency key para operações mutáveis.

## Streaming

Telemetry e transferências longas devem suportar streaming/backpressure.

## Compatibility

O Controller deve conhecer capabilities/features suportadas antes de enviar determinada operação.
