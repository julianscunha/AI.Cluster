# AI Cluster

AI Cluster is a Linux-first distributed AI runtime. The repository root is the architecture/specification layer; the only executable workspace is `code/phase-0/`, which is currently a Phase 0 Rust skeleton for the Controller/Worker substrate—not an LLM runtime.

## Source of truth

- Treat local files as authoritative.
- `ARCHITECTURE.md` is the master architecture document.
- `STATUS.md` distinguishes decided, proposed, experimental, and open decisions.
- Keep Phase 0 scope aligned with `implementation/roadmap.md` and `code/phase-0/docs/phase-0-acceptance.md`.

## Workspace and CI

Run Rust commands from `code/phase-0/`:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

These are the exact commands in `.github/workflows/rust.yml`. The workspace has no source `#[test]` functions yet; Phase 0 needs real test coverage before claiming acceptance.

## Layout

- `code/phase-0/crates/common` — shared domain types such as `NodeId`, `WorkId`, `RequestId`, and `NodeState`.
- `code/phase-0/crates/protocol` — generated gRPC/Protobuf boundary.
- `code/phase-0/crates/controller` — cluster control-plane entry point.
- `code/phase-0/crates/worker` — node-agent entry point and local hardware interaction.
- `code/phase-0/crates/transport` — transport trait and transport kinds.
- `code/phase-0/crates/benchmark` — capability measurement; never placement decisions.
- `code/phase-0/crates/cli` — operator CLI; do not put worker execution in it.
- `code/phase-0/proto/cluster.proto` — wire contract, package `ai.cluster.v1`.

## Conventions

- Use Rust 2024 and one focused crate per control/data-plane boundary.
- Keep shared types in `common`; depend on them through local path dependencies.
- Keep wire messages in `proto/cluster.proto`; `crates/protocol/build.rs` generates bindings and `src/lib.rs` exposes them under `cluster`.
- Existing binaries use `#[tokio::main]` and return `Result<(), Box<dyn std::error::Error>>`.
- Existing history uses Conventional Commit-style subjects, e.g. `test(controller): ...`, `style(controller): ...`, and `docs: ...`.

## Pitfalls

- The Cargo workspace is not at repository root. The CI workflow currently runs Cargo without `working-directory: code/phase-0`; fix that before relying on GitHub Actions.
- Do not edit generated Protobuf output in `target/`; edit `code/phase-0/proto/cluster.proto` instead.
- `tonic-prost-build` is used without a vendored `protoc` dependency; provide `protoc` on build hosts if protocol generation fails.
- `.gitignore` excludes `**/target/`, `.hermes/`, and `.env*` except `.env.example`.
