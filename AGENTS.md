---
title: "AI Cluster"
type: system
status: living
tags: [system, agents]
---

# AI Cluster

AI Cluster is a Linux-first distributed AI runtime. The repository root is the architecture/specification layer; the only executable workspace is `code/`, which is currently a Phase 0 Rust skeleton for the Controller/Worker substrate—not an LLM runtime.

## Source of truth

- Treat local files as authoritative.
- `ARCHITECTURE.md` is the master architecture document.
- `STATUS.md` distinguishes decided, proposed, experimental, and open decisions.
- Keep Phase 0 scope aligned with `implementation/roadmap.md` and `implementation/phase-0/phase-0-acceptance.md`.

## Workspace and CI

Run Rust commands from `code/`:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

These are the exact commands in `.github/workflows/rust.yml`. 24 unit tests pass, including metadata/version-gate coverage; keep them green.

## Layout

- `code/crates/common` — shared domain types such as `NodeId`, `WorkId`, `RequestId`, and `NodeState`.
- `code/crates/protocol` — generated gRPC/Protobuf boundary.
- `code/crates/controller` — cluster control-plane entry point.
- `code/crates/worker` — node-agent entry point and local hardware interaction.
- `code/crates/transport` — transport trait and transport kinds.
- `code/crates/benchmark` — capability measurement; never placement decisions.
- `code/crates/cli` — operator CLI; do not put worker execution in it.
- `code/proto/cluster.proto` — wire contract, package `ai.cluster.v1`.

## Conventions

- Use Rust 2024 and one focused crate per control/data-plane boundary.
- Keep shared types in `common`; depend on them through local path dependencies.
- Keep wire messages in `proto/cluster.proto`; `crates/protocol/build.rs` generates bindings and `src/lib.rs` exposes them under `cluster`.
- Existing binaries use `#[tokio::main]` and return `Result<(), Box<dyn std::error::Error>>`.
- Existing history uses Conventional Commit-style subjects, e.g. `test(controller): ...`, `style(controller): ...`, and `docs: ...`.

## Pitfalls

- The Cargo workspace is `code/`; the CI workflow sets it as the working directory for every Cargo command.
- Do not edit generated Protobuf output in `target/`; edit `code/proto/cluster.proto` instead.
- The protocol build script selects `protoc-bin-vendored`; do not depend on a host `protoc` installation.
- `.gitignore` excludes `**/target/`, `.hermes/`, and `.env*` except `.env.example`.
