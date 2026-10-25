# AI Cluster — workspace Rust

Workspace do AI Cluster: controller, workers, CLI, gateway de inferência e o motor de inferência em CPU. A documentação (arquitetura, fases, status) está em [`../docs/inicio.md`](../docs/inicio.md); o estado do que está implementado, em [`../docs/implementation/status.md`](../docs/implementation/status.md).

## Comandos

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Testes E2E: `bash tests/<script>.sh` (mapa em [`../docs/implementation/transversal/evidencias-e2e.md`](../docs/implementation/transversal/evidencias-e2e.md)). Os de inferência com modelo exigem os pesos em `models/` (veja [`../docs/implementation/08-inferencia/f08-engine-local.md`](../docs/implementation/08-inferencia/f08-engine-local.md)).

## Layout

`crates/{common,protocol,controller,worker,transport,benchmark,cli,engine,gateway}`, `proto/cluster.proto`, `tests/`, `os/` (imagem do AI Cluster OS, ainda não iniciada: fase F15).
