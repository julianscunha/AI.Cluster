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

### Implementation status (Phase 0)

`node_id` travels in each protobuf payload. The remaining fields are carried as
gRPC request metadata, set by an interceptor so no call site repeats them:

| field | header | status |
|---|---|---|
| protocol version | `x-ai-cluster-protocol-version` | emitted by clients, enforced by the controller |
| correlation id | `x-ai-cluster-correlation-id` | fresh value per call |
| authentication context | `x-ai-cluster-auth` | placeholder `anonymous` until the trust phase |
| idempotency | — | structural: a work unit leaves the dispatch table on completion, so a duplicate submission has nothing to complete |

The controller rejects any call whose protocol version does not match its own.

## Streaming

Telemetry e transferências longas devem suportar streaming/backpressure.

## Compatibility

O Controller deve conhecer capabilities/features suportadas antes de enviar determinada operação.
