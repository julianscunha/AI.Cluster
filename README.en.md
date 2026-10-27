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

# AI Cluster

**Turn the idle machines you already own (old PCs, laptops, servers, Raspberry/Orange Pi boards) into a single pool of compute for AI.**

Each one is weak; together they run a model. AI Cluster securely discovers and admits nodes, measures what each can do, splits the work, **slices the model weights across machines**, and keeps going when one of them dies. Written in Rust, with an OpenAI-compatible API.

<p align="center">
  <img src="docs/assets/architecture.svg" alt="Architecture: client, gateway, controller and workers" width="100%">
</p>

## Why use it

- **Reuses any hardware:** x86_64 and ARM in the same cluster, no GPU required.
- **Truly distributed inference:** weights stay resident on the workers; only partial results travel. If a worker dies, its ranges are reassigned immediately.
- **Nodes come and go:** token join, approval, drain, clean leave, automatic quarantine of misbehaving nodes.
- **Role-based security:** separate credentials to join, operate and infer; optional mTLS; the gateway runs without the operator token.
- **Its own OS for SBCs:** immutable Alpine A/B image with automatic rollback, **Ed25519-signed updates** and controller-driven canary rollout.
- **Observable:** Prometheus metrics, events, request traces and a terminal dashboard.

<p align="center">
  <img src="docs/assets/terminal-nodes.svg" alt="Real output of ai-cluster nodes with an x86 PC and an Orange Pi" width="100%">
</p>

## What you need

| Item | Requirement |
|---|---|
| **Where to run the commands** | On your own computer, **inside the `code/` folder of the cloned repository**. The commands below start a test cluster in Docker containers on the same machine; you do not need several machines to try it. |
| **OS** | Linux, macOS or Windows (with Docker Desktop). On Windows use Git Bash or PowerShell. |
| **Docker** | Docker Engine or Docker Desktop running. |
| **RAM / disk** | ~4 GB free RAM and ~6 GB disk for the build. |
| **Rust** | Only to build without Docker: stable Rust ≥ 1.85 (edition 2024). `protoc` is bundled. |
| **AI models** | Optional. GPT-2 / SmolLM2 inference needs the weights in `code/models/` ([how to get them](docs/guia/instalacao.md)); the cluster, CLI and benchmark work without them. |
| **Network** | On a real cluster, nodes must reach the controller port (default `50051`). |

## Get started in 5 minutes (one machine, no extra hardware)

```bash
git clone https://github.com/julianscunha/Real.AI.Cluster.git
cd Real.AI.Cluster/code                    # all commands below run here

bash scripts/dev-cluster.sh build          # compile (a few minutes the first time)
bash scripts/dev-cluster.sh up 2           # start 1 controller + 2 workers in containers
bash scripts/dev-cluster.sh cli nodes      # list nodes and their health
bash scripts/dev-cluster.sh cli dashboard --watch 2
bash scripts/dev-cluster.sh cli benchmark --size 256 --repetitions 8
bash scripts/dev-cluster.sh down           # stop everything
```

On Windows (PowerShell): `scripts\dev-cluster.ps1` with the same arguments.

> The development cluster runs **without TLS and with fixed tokens**. It is for your own machine only; never expose it to an open network. For production see [`docs/guia/operacao.md`](docs/guia/operacao.md).

Run the tests: `cd code && cargo test --workspace --all-features` (or see [`docs/guia/testes.md`](docs/guia/testes.md)).

## A real cluster: several machines

1. On one machine run the **controller**; on the others run the **worker** pointing at it (`AI_CLUSTER_CONTROLLER=http://<controller-ip>:50051`).
2. Each new node joins with the *join token* and you approve it: `ai-cluster approve <node>`.
3. Watch it with `ai-cluster nodes`, `dashboard` and `events`.

Step by step, TLS, tokens and every environment variable: [`docs/guia/instalacao.md`](docs/guia/instalacao.md) and [`docs/guia/configuracao.md`](docs/guia/configuracao.md). (The guides are in Brazilian Portuguese.)

## Distributed inference

The gateway exposes `POST /v1/chat/completions` (with SSE), `GET /v1/models` and `/metrics`. Each weight projection is split across workers, which keep it in memory.

<p align="center">
  <img src="docs/assets/inference-split.svg" alt="One model layer split across three workers" width="100%">
</p>

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer <your-key>" -H "Content-Type: application/json" \
  -d '{"model":"smollm2-135m","messages":[{"role":"user","content":"Hello!"}],"max_tokens":32}'
```

API details, supported models and how to enable slicing on workers: [`docs/guia/api.md`](docs/guia/api.md).

## AI Cluster OS: safe node updates

On ARM boards the project ships an immutable image with two slots (A/B). The operator signs a release; the controller announces it, a canary node updates first and the rest follow in waves. If the new node is not healthy, boot returns to the previous slot by itself.

<p align="center">
  <img src="docs/assets/os-update.svg" alt="Signed update flow with A/B slots" width="100%">
</p>

Board install and rollout: [`docs/guia/operacao.md`](docs/guia/operacao.md). Known security limits: [`SECURITY.md`](SECURITY.md).

## Project status

Pre-production: the core (cluster, placement, distributed inference, role-based security, A/B image with signed releases) runs and is tested on an x86 PC and an Orange Pi (ARMv7). What is still missing is in [`docs/implementation/pendencias.md`](docs/implementation/pendencias.md): accelerators (GPU/NPU), mobile worker, key revocation and hardware attestation, among others.

## Documentation

| To | See |
|---|---|
| Install, configure and operate | [`docs/guia/`](docs/guia/guia.md) |
| Understand how it works and is structured | [`docs/system/`](docs/system/system.md) (architecture, protocol, data model, decisions) |
| Contribute code | [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md) |

Documentation is written in Brazilian Portuguese.

## Repository layout

```
code/
  crates/{common,protocol,controller,worker,transport,benchmark,cli,engine,gateway}
  proto/cluster.proto      wire contract (ai.cluster.v1)
  os/                      A/B image, netboot, installer
  scripts/ tests/          development cluster, end-to-end tests
docs/                      guia/, system/, implementation/ (pending work)
```

## Contribute

Give it a ⭐ if you like the idea, open an issue with ideas or problems and read [`CONTRIBUTING.md`](CONTRIBUTING.md). Vulnerabilities: [`SECURITY.md`](SECURITY.md) (private report, do not open an issue). Conduct: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

License: [Apache-2.0](LICENSE).
