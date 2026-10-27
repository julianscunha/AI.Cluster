# AI Cluster — workspace Rust

Workspace do AI Cluster: controller, workers, CLI, gateway de inferência e o motor de inferência em CPU. A documentação (guia, arquitetura, pendências) está em [`../docs/inicio.md`](../docs/inicio.md).

## Comandos

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Testes E2E: `bash tests/<script>.sh` (mapa em [`../docs/guia/testes.md`](../docs/guia/testes.md)). Os de inferência com modelo exigem os pesos em `models/` (veja [`../docs/implementation/08-inferencia/f08-engine-local.md`](../docs/implementation/08-inferencia/f08-engine-local.md)).

## Layout

`crates/{common,protocol,controller,worker,transport,benchmark,cli,engine,gateway}`, `proto/cluster.proto`, `tests/`, `os/` (imagem do AI Cluster OS, ainda não iniciada: fase F15).
