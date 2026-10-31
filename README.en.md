<p align="center">
  <img src="docs/assets/banner.svg" alt="AI Cluster" width="100%">
</p>

<p align="center">
  <a href="https://github.com/julianscunha/AI.Cluster/actions/workflows/rust.yml"><img alt="CI" src="https://github.com/julianscunha/AI.Cluster/actions/workflows/rust.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-blue"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/rust-edition%202024-orange?logo=rust">
  <img alt="Platforms" src="https://img.shields.io/badge/x86__64%20%7C%20ARMv7-tested-informational">
  <img alt="Status" src="https://img.shields.io/badge/status-pre--production-yellow">
  <a href="SECURITY.md"><img alt="Security" src="https://img.shields.io/badge/security-policy-green"></a>
</p>

**English** · [Português](README.md)

# AI Cluster

**Turn the idle machines you already own (old PCs, laptops, servers, Raspberry/Orange Pi boards) into a single pool of compute for AI.**

Each one is weak; together they run a model. You download a model in **GGUF**, drop it in a folder and run `aic cluster model start`: the controller checks that the workers' **combined memory** holds the model (and says what is missing if not), picks which machines load it, splits the layers among them and hands you the **API key**. When a new machine joins, it re-plans by itself. No GPU, no per-node configuration. Execution is done by [llama.cpp](https://github.com/ggml-org/llama.cpp) (hundreds of architectures, quantized weights, x86 and ARM); AI Cluster takes care of securely admitting nodes, measuring what each can do, splitting the model and keeping going when a machine dies. Written in Rust, with an OpenAI-compatible API.

```bash
aic cluster model list            # the GGUF models in the folder
aic cluster model check qwen2.5-7b-q4.gguf   # fits? OK, or NOK with what is missing
aic cluster model start qwen2.5-7b-q4.gguf   # loads it and shows the API key (once)
aic cluster model status          # state, plan workers and measured tokens/s
```

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
| **`aic` command** | How you administer and monitor the cluster (`aic cluster …`) and update each machine (`aic update`). | Your computer, or the server itself. | — |

Usage flow:

0. **Just to try it**, everything on one computer with Docker: see the [quick start](#quick-start-on-one-computer-docker).
1. **Install the controller** on the server and save the two tokens (join and operator).
2. **Turn each device into a worker.** The project's path is **AI Cluster OS**: its own immutable, lean system that **replaces the device's operating system** (the device is wiped) and dedicates it to being a worker, with signed updates and automatic rollback. Today the image exists for **ARMv7 boards** (tested on hardware) and **x86_64 UEFI PCs** (validated in QEMU, not yet on real hardware); for **ARM64 it does not exist yet**. Where there is no image the worker can be installed on top of the Linux the device already runs (transition path, nothing is wiped).
3. **Approve** each new node and **monitor** everything with the CLI.
4. **Put a GGUF model in the controller's folder and run `aic cluster model start`** (step 5). Point your applications at the gateway with the key it shows.

## Prerequisites

| Where | Needs |
|---|---|
| **Controller/gateway server** | Linux x86_64 (glibc), network access from workers to port `50051` and from clients to `8080`. The gateway is the model's main node (it runs `llama-server`): it needs RAM for the layers it keeps and for the context. |
| **Each worker** | **AI Cluster OS** (ARMv7 board or x86_64 UEFI PC: the disk is wiped and rewritten) **or** an installed x86_64/ARMv7/ARM64 Linux with `root` and, preferably, systemd. Either way: outbound connection to the controller's port `50051` and free memory for the model slice. |
| **Build machine** (once) | Linux with stable Rust ≥ 1.85 (`rustup`). It compiles the binaries; servers and devices do not need Rust. To produce ARMv7 binaries from x86, Docker on that machine. |
| **Your computer** | The `aic` command and network access to the controller. |
| **Models** | **GGUF** files (most open models have a quantized version) in a folder both the controller and the gateway can see. Without a model the cluster works, it just does not answer chat. The [model catalog](docs/guia/modelos.md) lists open-weight models with scores, estimated RAM, download links and which ones are verified on the cluster. |

Docker is **not** required to use AI Cluster. It only appears for people developing the project (see the end of this README).

## Guided install (Linux)

```bash
curl -fsSL https://github.com/julianscunha/AI.Cluster/releases/latest/download/install.sh | sudo sh -s -- --lang en
```

`--lang en` makes the installer, the `aic` command and the invite scripts speak English (it is remembered on the server, so `aic cluster invite` later stays in English). Without it everything is in Portuguese. The help and messages of the `aic` command are in Portuguese for now: an English version of the CLI does not exist yet.

One script asks what each computer will be and downloads only what that role needs from the **latest release**. It does not auto-detect your hardware: you tell it, and it only checks that the processor you named is the real one. It explains the three components and asks:

| Component | What it does | Runs on |
|---|---|---|
| **Controller** | The brain: registers the computers, decides where each task runs, keeps keys and models. One per cluster. | x86_64, ARMv7 |
| **Gateway** | The front door: serves the chat API (OpenAI-compatible) and splits the model among the workers. One per cluster. | x86_64 (`llama-server` is not built for ARM yet) |
| **Worker** | Lends CPU and memory. As many as you like. | x86_64, ARMv7 |

Start with the controller (with the gateway on the same computer, or on another one: the script asks), then run `sudo aic cluster invite` on it. The invite prints an **8-character code** and how the other computers join (the worker and gateway installers ask for the code; without it nobody gets in): the same installer choosing "Worker", or the short `curl .../join | sudo sh` line. Each worker generates its own tunnel key; the server only signs the certificate request. A worker's package comes from your own server, in the cluster's version, so the workers do not need internet access. Then `aic cluster model start <file.gguf>` puts the model online at `http://<ip>:8080`.

On a computer that already runs Linux and will become a dedicated AI Cluster OS, the invite also prints `curl -fsSL http://<ip>:8099/wait | sudo sh`: the machine introduces itself with a fingerprint, and **nothing is installed or erased until you approve it** on the server (`aic cluster install list`, `aic cluster install approve <fingerprint> --disk <disk>`). The installation itself does not exist yet ([ADR-023](docs/system/05-decisoes/adr-023-instalacao-com-aprovacao-do-controller.md)).

The script can also install on a **Linux you already have** (shared with whatever else runs on it) or, for the worker, point you to the **dedicated AI Cluster OS** image (the machine only runs the cluster; the disk is rewritten). Details, limits and the dedicated-install order (controller first) are in [the installation guide](docs/guia/instalacao.md). ARM 64-bit and phones do not have packages yet; the script says so instead of installing anything.

Without prompts: `sudo sh install.sh --mode linux --role server|controller|gateway|worker --arch x86_64|armv7 --kind fisico|vm|conteiner [--server IP] --yes`.

After the installation the only command is `aic`. It covers everything: `aic cluster …` (the cluster as seen by the controller), `aic controller|gateway|worker status` (this machine), `aic update` and `aic version`. The full reference is in [the CLI guide](docs/guia/cli.md).

### Two computers

1. **On the server:** choose *Controller* (with the gateway on the same machine, the simplest). Then run `sudo aic cluster invite`: it shows an **8-character code** and the line the other computers use to join.
2. **On the second computer:** run the invite line (`curl -fsSL http://<server-ip>:8099/join-<CODE> | sudo sh`) and choose *Worker*. Without the code, nobody gets in.
3. **Back on the server:** `aic cluster nodes` shows the new node as pending; `aic cluster node approve <id> --fingerprint <fingerprint>` approves it. Only approved nodes receive work.
4. `aic cluster model start <file.gguf>` puts the model online and shows the API key. The API is at `http://<ip>:8080`.

On a Linux computer that will become a dedicated AI Cluster OS, the invite also prints `curl -fsSL http://<ip>:8099/wait | sudo sh`: the machine introduces itself with a fingerprint, and **nothing is installed or erased until you approve it** with `aic cluster install approve`. Details, limits and the dedicated mode are in [the installation guide](docs/guia/instalacao.md). The step-by-step install below still applies if you want to control each part.

### Update and services on boot

- `aic update status` shows the installed and the available version; `sudo aic update apply` updates this computer (controller, gateway or worker on Linux). On Linux, the update checks the release signature with the Linux key, stored at install.
- On AI Cluster OS and on Linux workers, the controller pushes the signed release. `aic cluster nodes` shows the `VERSION` of each node, so you can see who is behind.
- Services start by themselves when the computer boots (systemd, with automatic restart). In a container without systemd they do not come back by themselves after the container restarts. What each component does on `aic update` is in [the operations guide](docs/guia/operacao.md).


## Quick start on one computer (Docker)

To try the whole flow without setting anything up by hand (needs Docker, bash and openssl):

```bash
bash deploy/init.sh                                    # generates tokens and certificates (once)
cp my-model.gguf deploy/models/                        # any GGUF model
docker compose -f deploy/docker-compose.yml up -d --build --scale worker=2
docker compose -f deploy/docker-compose.yml exec controller aic cluster model list
docker compose -f deploy/docker-compose.yml exec controller aic cluster model start my-model.gguf
docker compose -f deploy/docker-compose.yml exec controller aic cluster model status
```

`aic cluster model start` checks the workers' RAM, splits the model among them and shows the API key (once). Then call `POST http://localhost:8080/v1/chat/completions` with `Authorization: Bearer <key>`. See [the models and scores](docs/guia/modelos.md) and, for workers on other machines, [the installation guide](docs/guia/instalacao.md).

## Step-by-step installation

### 1. Build the binaries (once)

On a Linux build machine:

```bash
git clone https://github.com/julianscunha/AI.Cluster.git
cd AI.Cluster/code
cargo build --release --workspace
# binaries in target/release/: ai-cluster-controller, ai-cluster-gateway, ai-cluster-worker, aic
```

Copy `ai-cluster-controller` and `ai-cluster-gateway` to the server, `aic` to whoever administers, and `ai-cluster-worker` to each device **of the same architecture**. For ARMv7 boards build with `TARGET=armv7-unknown-linux-musleabihf bash scripts/build-armv7.sh` (see the [installation guide](docs/guia/instalacao.md)). For x86_64 there is a ready-made package in the [v0.2.1 release](https://github.com/julianscunha/AI.Cluster/releases/tag/v0.2.1).

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

> `AI_CLUSTER_INSECURE=1` turns TLS off: use it only on a trusted local network. On any other network configure TLS (controller certificate and `AI_CLUSTER_TLS_CA` on the workers): [`docs/guia/instalacao.md`](docs/guia/instalacao.md). `aic controller install` already installs the controller as a systemd service; running it by hand, as above, is for understanding each part.

### 3. Worker (on each device)

**Recommended path: AI Cluster OS (ARMv7 boards).** The device is wiped and runs the project's own system, already with the worker, the controller configured and signed A/B updates. See step 6.

**Transition path: worker on the existing Linux.** It serves ARM64 (no image yet) and anyone who does not want to wipe the PC yet (the x86_64 image is new and only validated in QEMU). Nothing is wiped, and the device stays an ordinary Linux: no A/B updates or boot rollback. Copy `ai-cluster-worker`, `code/os/bootstrap.sh` and the join token to the device and, as root:

```bash
echo '<join-token>' > /root/join-token && chmod 600 /root/join-token
sudo ./bootstrap.sh --controller http://<server-ip>:50051 --insecure \
  --token-file /root/join-token --binary ./ai-cluster-worker --node-id room-pc1
```

The script creates the user, installs the worker as a systemd service and confirms it registered. (`--insecure` only because the example uses `http://`; with TLS use `https://` and the CA.)

### 4. Approve and monitor (CLI)

```bash
export AI_CLUSTER_CONTROLLER=http://<server-ip>:50051 AI_CLUSTER_OPERATOR_TOKEN=<operator-token>
aic cluster nodes             # the new node shows up, pending
aic cluster node approve room-pc1 --fingerprint <fingerprint>  # only approved nodes receive work
aic cluster dashboard --watch 2
aic cluster events            # joins, leaves, failures
```

<p align="center">
  <img src="docs/assets/terminal-nodes.svg" alt="Real output of aic cluster nodes with an x86 PC and an Orange Pi" width="100%">
</p>

### 5. Model, gateway and API

Every worker that will split models runs llama.cpp's `ggml-rpc-server` **behind an mTLS tunnel** (the RPC server has no password, so it is never exposed): on the ARMv7 image it ships and starts by itself once the certificates are in `/data/rpc-tls`; on a regular Linux use `bootstrap.sh --rpc-server-bin ... --rpc-tls-dir ...`. The gateway gets `llama-server`, the models folder and the certificates ([`docs/guia/configuracao.md`](docs/guia/configuracao.md)). Then:

```bash
aic cluster model list                       # ROD column: the engine runs this model
aic cluster model check my-model.gguf        # sums worker RAM; NOK says how much is missing
aic cluster model start my-model.gguf        # plan, load and the API key (once)
aic cluster model status                     # LOADING, READY; plan workers; tokens/s
```

```bash
curl http://<server-ip>:8080/v1/chat/completions \
  -H "Authorization: Bearer <key-shown-by-start>" -H "Content-Type: application/json" \
  -d '{"model":"my-model","messages":[{"role":"user","content":"Hello!"}],"max_tokens":64}'
```

**How the controller decides:** fastest workers first, **only until their combined RAM holds the model**. Every extra worker adds a network hop per word, so a slow one (an Orange Pi next to a PC) joins only when memory would run out; with the model fitting on the PC, the Pi would cost about 5x of its speed (measured). Among the chosen ones, the layer split is proportional to speed and capped by each one's RAM. A new worker changes the plan only if it changes that set, after 30 s of stability. If RAM stops being enough, the model stops with the reason and comes back by itself when it does.

**API keys:** one per tenant, with models, quota and priority class (`aic cluster model key add|rotate|remove|list`). The controller keeps only the hash. **Several slots** (`AI_CLUSTER_LLAMA_SLOTS`) serve more requests at once: in the lab, 2 concurrent requests gave 34% more total tokens per second.

<p align="center">
  <img src="docs/assets/inference-split.svg" alt="One model layer split across three workers" width="100%">
</p>

The project's own engine (GPT-2 and SmolLM2 in safetensors, with projections sliced across workers) is still available as a parity reference; see [`docs/guia/api.md`](docs/guia/api.md). All variables, tenants, quotas and the full API: [`docs/guia/configuracao.md`](docs/guia/configuracao.md). (The guides are in Brazilian Portuguese.)

### 6. AI Cluster OS: the device becomes a dedicated worker

**AI Cluster OS** is the project's destination for any device: an immutable Alpine image with two slots (A/B) that wipes the previous system and leaves the machine with only what a worker needs. The operator signs an update; the controller announces it, a canary node updates first and the rest follow in waves. If the new node is not healthy, boot returns to the previous slot by itself.

**Availability today:** **ARMv7** boards (tested on an Orange Pi PC) and **x86_64 UEFI** PCs (validated in QEMU with OVMF, real hardware still to be tested; needs Secure Boot off; see the [guide](docs/guia/instalacao.md#imagem-do-ai-cluster-os-x86_64-uefi)). **There is no image yet for ARM64**, which is the project's next step (see [`pendencias.md`](docs/implementation/pendencias.md)). Until then, desktops and servers use the transition path from step 3.

<p align="center">
  <img src="docs/assets/os-update.svg" alt="Signed update flow with A/B slots" width="100%">
</p>

Building the image, flashing the card and publishing releases: [`docs/guia/instalacao.md`](docs/guia/instalacao.md) and [`docs/guia/operacao.md`](docs/guia/operacao.md).

## What you get

- **A guided installer:** one script asks the role of each computer (controller, gateway or worker) and downloads only what it needs for x86_64 or ARMv7, with SHA-256 checks.
- **One command puts the model online:** `aic cluster model start` checks the RAM, picks the workers, splits the layers, loads and hands out the API key. No per-model variables, no copying weights to every node.
- **Says "it does not fit" before trying:** `aic cluster model check` sums worker RAM and answers OK or NOK with what is missing and how many nodes to add. More machines, more model: no size ceiling beyond the combined memory.
- **Picks the right workers:** fast first, slow only when RAM requires; the layer split follows each node's speed and memory limit.
- **Quantized models and hundreds of architectures** (GGUF, through llama.cpp), on x86_64 and on a 1 GB ARMv7, no GPU required. The Orange Pi PC runs a 360M model at 2.6 tokens/s alone and a 1.5B one split with a PC.
- **A new worker joins by itself:** the plan is redone, the model reloads and the benchmark is measured again.
- **More requests at once:** several slots per model, with a queue by priority class.
- **Per-tenant API keys:** models, quota and class per key; rotation and removal; only the hash is stored.
- **Nodes come and go:** token join, approval checked against the credential fingerprint, drain, clean leave, automatic quarantine of misbehaving nodes.
- **Role-based security:** separate credentials to join, operate and infer; TLS and mTLS; the gateway runs without the operator token; each worker holds its own tunnel key. llama.cpp's `rpc-server`, which has no password, is only served through an mTLS tunnel with a gateway role.
- **Safe system updates:** signed release (Ed25519), canary, waves and automatic rollback.
- **Observable:** Prometheus metrics on `:9101`, events, request traces and a terminal dashboard.

## Project status

Pre-production: the core (cluster, placement by speed and RAM, GGUF models split across machines, role-based security, A/B image with signed releases) runs and is tested on an x86 PC and an Orange Pi (ARMv7). Real measurements and the reasoning behind each choice are in [ADR-022](docs/system/05-decisoes/adr-022-fluxo-de-modelos-e-motor-hibrido.md). What is still missing is in [`docs/implementation/pendencias.md`](docs/implementation/pendencias.md): per-node tunnel certificates, accelerators (GPU/NPU), mobile worker, key revocation and hardware attestation, among others.

**Limits worth knowing:** a slow worker limits the speed of every request that goes through it, so a cluster pays off when the model **does not fit on one machine**; big models on weak hardware yield few words per second; the [catalog](docs/guia/modelos.md) scores come from official model cards and were only spot-checked.

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
  crates/{common,protocol,controller,worker,transport,benchmark,cli,engine,gateway,catalog,tunnel}
  proto/cluster.proto      wire contract (ai.cluster.v1)
  os/                      bootstrap.sh, A/B image, netboot, installer
  third_party/llama.cpp/   32-bit patch and build script for llama.cpp
  scripts/ tests/          ARM build, development cluster, end-to-end tests
deploy/                    Docker Compose: single Dockerfile, init.sh (tokens and certificates)
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
