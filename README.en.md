<p align="center">
  <img src="docs/assets/banner.svg" alt="AI Cluster" width="100%">
</p>

<p align="center">
  <a href="https://github.com/julianscunha/Real.AI.Cluster/actions/workflows/rust.yml"><img alt="CI" src="https://github.com/julianscunha/Real.AI.Cluster/actions/workflows/rust.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-blue"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/rust-edition%202024-orange?logo=rust">
  <img alt="Platforms" src="https://img.shields.io/badge/x86__64%20%7C%20ARMv7-tested-informational">
  <img alt="Status" src="https://img.shields.io/badge/status-pre--production-yellow">
  <a href="SECURITY.md"><img alt="Security" src="https://img.shields.io/badge/security-policy-green"></a>
</p>

**English** · [Português](README.md)

# AI Cluster — Distributed AI Runtime

A Linux-first runtime written in Rust that turns repurposed, heterogeneous hardware (PCs, laptops, servers, SBCs such as the Orange Pi) into a **single pool of compute for AI**. One inference can be split across several devices; nodes join, leave, fail and drain without taking the cluster down.

> Note: the project documentation (`docs/`) and commit messages are in Brazilian Portuguese.

## Purpose

Plenty of idle machines are useful for AI once orchestrated: each is weak, the set is not. This project covers the gap between "I have ten old machines" and "I have an inference service": secure node discovery and admission, per-node capability measurement, cost-based placement, failure tolerance, observability, and updating the nodes' own operating system.

## What it does

| Feature | How |
|---|---|
| **Cluster** | The `controller` keeps the node registry, work store, reservations and events; the `worker` is the node agent (token join, heartbeat, drain, clean leave). |
| **Capability and placement** | Each node measures CPU, memory, network and disk; the controller decides where each Work Unit runs by cost, with DAGs, retries and re-planning. |
| **Distributed inference** | The `gateway` serves `/v1/chat/completions` (SSE) on GPT-2 and SmolLM2; the `engine` slices the weight projections across workers, which keep them resident. If a worker dies, its ranges are reassigned. Numerical parity with `transformers` is an acceptance criterion. |
| **Role-based security** | Three credentials (join, operator, inference); optional mTLS; the gateway runs without the operator token. |
| **Observability** | Prometheus metrics, events, traces and a CLI dashboard. |
| **AI Cluster OS** | Alpine A/B image for SBCs with u-boot, automatic rollback, an **Ed25519-signed release** verified on the node, controller-driven canary rollout, and a SHA-256 measurement of the slot as software attestation. |

The exact state (what exists vs. target architecture) is in [`docs/implementation/status.md`](docs/implementation/status.md). Known security limits are in the ADRs, e.g. [ADR-018](docs/system/05-decisoes/adr-018-release-assinada-e-medida-do-slot.md).

## Architecture at a glance

```
                 operator (ai-cluster CLI)
                          │  gRPC, operator token
                          ▼
   client ──HTTP──► gateway ──gRPC──► controller ◄──gRPC── workers (x86, ARM…)
 (/v1/chat/…)        │  engine           registry · placement     benchmark · ShardMatmul
                     └─ weight slices ──────────────────────────► resident on workers
```

## Getting started

Requirements: stable Rust (edition 2024) **or** Docker. `protoc` is vendored.

```bash
git clone https://github.com/julianscunha/Real.AI.Cluster.git
cd Real.AI.Cluster/code

cargo test --workspace --all-features          # unit tests
bash scripts/dev-cluster.sh build              # build into the aic-target Docker volume
bash scripts/dev-cluster.sh up 2               # controller + 2 workers (no TLS, development only)
bash scripts/dev-cluster.sh cli nodes          # nodes and health
bash scripts/dev-cluster.sh cli dashboard --watch 2
bash scripts/dev-cluster.sh down
```

On Windows use `scripts/dev-cluster.ps1`. The development cluster uses fixed tokens (`dev-token`, `dev-operator`): **never** expose it to an open network.

End-to-end tests: `bash tests/<script>.sh` (map in [`docs/implementation/transversal/evidencias-e2e.md`](docs/implementation/transversal/evidencias-e2e.md)). Inference tests need the weights in `code/models/{gpt2,smollm2,smollm2-instruct}` (see [`f08-engine-local.md`](docs/implementation/08-inferencia/f08-engine-local.md)).

To install the OS on a real node (A/B image, signed rollout): [`provisionamento`](docs/system/02-subsistemas/plataforma/provisionamento.md) and [`f19-rollout-de-release`](docs/implementation/19-hardening-de-producao/f19-rollout-de-release.md).

## Layout

```
code/
  crates/{common,protocol,controller,worker,transport,benchmark,cli,engine,gateway}
  proto/cluster.proto      wire contract (ai.cluster.v1)
  os/                      A/B image, netboot, installer
  tests/ scripts/          shell E2E, development cluster
docs/                      Obsidian vault: system/ (the system) and implementation/ (phases F00–F19)
```

## Documentation

All project knowledge lives in an Obsidian vault under `docs/`. Start at [`docs/inicio.md`](docs/inicio.md); conventions in [`docs/convencoes.md`](docs/convencoes.md). Crate layout and pitfalls: [`AGENTS.md`](AGENTS.md).

## Contributing, security and license

- Contributing: [`CONTRIBUTING.md`](CONTRIBUTING.md) · conduct: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md)
- Vulnerabilities: [`SECURITY.md`](SECURITY.md) (do not open a public issue)
- License: [Apache-2.0](LICENSE)
