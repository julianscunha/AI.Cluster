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

Each one is weak; together they run a model. AI Cluster securely admits nodes, measures what each can do, splits the work, **slices the model weights across machines**, and keeps going when one of them dies. Written in Rust, with an OpenAI-compatible API.

<p align="center">
  <img src="docs/assets/architecture.svg" alt="Architecture: client, gateway, controller and workers" width="100%">
</p>

## How it works in practice

AI Cluster is a Linux system with **roles**. You choose where each one runs:

<p align="center">
  <img src="docs/assets/deployment.svg" alt="Controller and gateway on a server, workers on devices, CLI on your computer" width="100%">
</p>

| Role | What it is | Where it runs | How many |
|---|---|---|---|
| **Controller** | The cluster's brain: registers nodes, distributes work, keeps state and metrics. | An always-on Linux server. | 1 |
| **Worker** | Executes work and keeps model slices in memory. | **Every device you want to put to use**: PC, laptop, server, ARM board. | as many as you have |
| **Gateway** | The API (`/v1/chat/completions`, OpenAI-compatible) your applications call. | On the controller's server (or another Linux host that can reach it). | 1 |
| **`ai-cluster` CLI** | How you administer and monitor: list nodes, approve, update. | Your computer, or the server itself. | — |

Usage flow:

1. **Install the controller** on the server and save the two tokens (join and operator).
2. **Install the worker** on each device, pointing at the controller. It is a real Linux machine running the worker, no container in between.
3. **Approve** each new node and **monitor** everything with the CLI.
4. **Start the gateway** with the model and point your applications at it.
5. For ARM boards, use **AI Cluster OS**, a dedicated system that updates itself safely.

## Prerequisites

| Where | Needs |
|---|---|
| **Controller/gateway server** | Linux (x86_64 or ARM, glibc), network access from workers to port `50051` and from clients to `8080`. The gateway with SmolLM2 keeps the model in memory (about 1 GB). |
| **Each worker** | Linux (x86_64 or ARMv7), `root` to install, systemd recommended, outbound connection to the controller's port `50051`. Free memory for the model slice it will hold. |
| **Build machine** (once) | Linux with stable Rust ≥ 1.85 (`rustup`). It compiles the binaries; servers and devices do not need Rust. To produce ARMv7 binaries from x86, Docker on that machine. |
| **Your computer** | The `ai-cluster` binary and network access to the controller. |
| **Models** | GPT-2 or SmolLM2 weights on the gateway server ([where to download](docs/guia/instalacao.md#pesos-dos-modelos)). Optional: without them the cluster works, it just does not answer chat. |

Docker is **not** required to use AI Cluster. It only appears for people developing the project (see the end of this README).

## Step-by-step installation

### 1. Build the binaries (once)

On a Linux build machine:

```bash
git clone https://github.com/julianscunha/Real.AI.Cluster.git
cd Real.AI.Cluster/code
cargo build --release --workspace
# binaries in target/release/: ai-cluster-controller, ai-cluster-gateway, ai-cluster-worker, ai-cluster
```

Copy `ai-cluster-controller` and `ai-cluster-gateway` to the server, `ai-cluster` to whoever administers, and `ai-cluster-worker` to each device **of the same architecture**. For ARMv7 boards build with `TARGET=armv7-unknown-linux-musleabihf bash scripts/build-armv7.sh` (see the [installation guide](docs/guia/instalacao.md)). There are no prebuilt binaries to download yet.

### 2. Controller (on the server)

```bash
export AI_CLUSTER_JOIN_TOKEN=$(openssl rand -hex 24)       # workers use it to join
export AI_CLUSTER_OPERATOR_TOKEN=$(openssl rand -hex 24)   # you use it to administer
echo "join=$AI_CLUSTER_JOIN_TOKEN operator=$AI_CLUSTER_OPERATOR_TOKEN"   # save both

mkdir -p /var/lib/aic
AI_CLUSTER_INSECURE=1 \
AI_CLUSTER_NODES=/var/lib/aic/nodes AI_CLUSTER_STATE=/var/lib/aic/state.bin AI_CLUSTER_EVENTS=/var/lib/aic/events.log \
./ai-cluster-controller        # listens on 0.0.0.0:50051
```

> `AI_CLUSTER_INSECURE=1` turns TLS off: use it only on a trusted local network. On any other network configure TLS (controller certificate and `AI_CLUSTER_TLS_CA` on the workers): [`docs/guia/instalacao.md`](docs/guia/instalacao.md). To keep the controller up across reboots, run it as a systemd service.

### 3. Worker (on each device)

Copy `ai-cluster-worker`, `code/os/bootstrap.sh` and the join token to the device and, as root:

```bash
echo '<join-token>' > /root/join-token && chmod 600 /root/join-token
sudo ./bootstrap.sh --controller http://<server-ip>:50051 --insecure \
  --join-token-file /root/join-token --binary ./ai-cluster-worker --node-id room-pc1
```

The script creates the user, installs the worker as a systemd service and confirms it registered. (`--insecure` only because the example uses `http://`; with TLS use `https://` and the CA.)

### 4. Approve and monitor (CLI)

```bash
export AI_CLUSTER_CONTROLLER=http://<server-ip>:50051 AI_CLUSTER_OPERATOR_TOKEN=<operator-token>
ai-cluster nodes             # the new node shows up, pending
ai-cluster approve room-pc1  # only approved nodes receive work
ai-cluster dashboard --watch 2
ai-cluster events            # joins, leaves, failures
```

<p align="center">
  <img src="docs/assets/terminal-nodes.svg" alt="Real output of ai-cluster nodes with an x86 PC and an Orange Pi" width="100%">
</p>

### 5. Gateway and inference

On the server, with the SmolLM2 weights in `/srv/models/smollm2`:

```bash
AI_CLUSTER_INSECURE=1 AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 \
AI_CLUSTER_CONTROLLER=http://127.0.0.1:50051 AI_CLUSTER_OPERATOR_TOKEN=<operator-token> \
AI_CLUSTER_API_KEY=<your-apps-key> \
AI_CLUSTER_SMOLLM2_DIR=/srv/models/smollm2 \
./ai-cluster-gateway
```

```bash
curl http://<server-ip>:8080/v1/chat/completions \
  -H "Authorization: Bearer <your-apps-key>" -H "Content-Type: application/json" \
  -d '{"model":"smollm2-135m","messages":[{"role":"user","content":"Hello!"}],"max_tokens":16}'
```

To **split the model across workers**, install the weights on them too (`--model-dir` of `bootstrap.sh`) and start the gateway with `AI_CLUSTER_SMOLLM2_SHARDS=room-pc1,room-pc2`: each worker keeps a slice in memory and the gateway sums the partial results.

<p align="center">
  <img src="docs/assets/inference-split.svg" alt="One model layer split across three workers" width="100%">
</p>

All variables, tenants, quotas and the full API: [`docs/guia/configuracao.md`](docs/guia/configuracao.md) and [`docs/guia/api.md`](docs/guia/api.md). (The guides are in Brazilian Portuguese.)

### 6. ARM boards: AI Cluster OS

On SBCs you can flash **AI Cluster OS**, an immutable Alpine image with two slots (A/B). The operator signs an update; the controller announces it, a canary node updates first and the rest follow in waves. If the new node is not healthy, boot returns to the previous slot by itself.

<p align="center">
  <img src="docs/assets/os-update.svg" alt="Signed update flow with A/B slots" width="100%">
</p>

Building the image, flashing the card and publishing releases: [`docs/guia/instalacao.md`](docs/guia/instalacao.md) and [`docs/guia/operacao.md`](docs/guia/operacao.md).

## What you get

- **Any architecture:** x86_64 and ARM in the same cluster, no GPU required.
- **Truly distributed inference:** weights stay resident on the workers; only partial results travel. If a worker dies, its work is reassigned immediately.
- **Nodes come and go:** token join, approval, drain, clean leave, automatic quarantine of misbehaving nodes.
- **Role-based security:** separate credentials to join, operate and infer; TLS and mTLS; the gateway runs without the operator token.
- **Safe system updates:** signed release (Ed25519), canary, waves and automatic rollback.
- **Observable:** Prometheus metrics on `:9101`, events, request traces and a terminal dashboard.

## Project status

Pre-production: the core (cluster, placement, distributed inference, role-based security, A/B image with signed releases) runs and is tested on an x86 PC and an Orange Pi (ARMv7). What is still missing is in [`docs/implementation/pendencias.md`](docs/implementation/pendencias.md): accelerators (GPU/NPU), mobile worker, key revocation and hardware attestation, among others.

## Documentation

Documentation is written in Brazilian Portuguese.

| To | See |
|---|---|
| Install, configure and operate | [`docs/guia/`](docs/guia/guia.md) |
| Understand how it works and is structured | [`docs/system/`](docs/system/system.md) (architecture, protocol, data model, decisions) |
| Contribute code | [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md) |

## Repository layout

```
code/
  crates/{common,protocol,controller,worker,transport,benchmark,cli,engine,gateway}
  proto/cluster.proto      wire contract (ai.cluster.v1)
  os/                      bootstrap.sh, A/B image, netboot, installer
  scripts/ tests/          ARM build, development cluster, end-to-end tests
docs/                      guia/, system/, implementation/ (pending work)
```

## For project developers

There is a development cluster in Docker containers that runs everything on your machine to test changes. **It is not the way to use AI Cluster**: it is a development tool (no TLS, fixed tokens).

```bash
cd code
bash scripts/dev-cluster.sh build && bash scripts/dev-cluster.sh up 2   # controller + 2 workers
bash scripts/dev-cluster.sh cli nodes
bash scripts/dev-cluster.sh gateway                                     # API at 127.0.0.1:8080, key dev-key
bash scripts/dev-cluster.sh down
cargo test --workspace --all-features
```

More in [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`docs/guia/testes.md`](docs/guia/testes.md).

## Contribute

Give it a ⭐ if you like the idea, open an issue with ideas or problems and read [`CONTRIBUTING.md`](CONTRIBUTING.md). Vulnerabilities: [`SECURITY.md`](SECURITY.md) (private report, do not open an issue). Conduct: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

License: [Apache-2.0](LICENSE).
