# Phase 0 Acceptance Criteria

## A1 Build

- Workspace resolves reproducibly.
- Controller, Worker and CLI have independent binaries.
- Protocol is generated from `.proto` definitions.

## A2 Protocol

- Node can register.
- Controller acknowledges registration.
- Worker sends heartbeat.
- Worker sends health report.

## A3 Identity

- Node identity is independent of IP address.
- Device identity is separate from logical node identity.

## A4 Measurement

- Benchmark harness can measure a node without changing its cluster score manually.
- Benchmark output is versioned.

## A5 No LLM

The phase is considered complete without any model runtime. The gate is the distributed substrate, not inference quality.
