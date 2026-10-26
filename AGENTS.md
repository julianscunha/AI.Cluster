# AI Cluster

Runtime de IA distribuído, Linux-first. O workspace executável é `code/` (Rust). A documentação é um vault do Obsidian em `docs/`, em duas pastas: `docs/system/` (o sistema, arquitetura-alvo completa) e `docs/implementation/` (fases `F00`–`F19`, slices, status, testes).

## Fonte da verdade

- Trate arquivos locais como autoritativos. Comece por `docs/inicio.md`.
- Arquitetura-alvo: `docs/system/` (mestre: `docs/system/01-arquitetura/arquitetura.md`). Decisões: `docs/system/05-decisoes/`.
- O que existe e o que falta: `docs/implementation/status.md`; ordem e dependências: `docs/implementation/implementation.md`.
- Convenções de formato dos docs (wikilinks, frontmatter, checkbox `Superado`): `docs/convencoes.md`. Ao mudar código, atualize o slice `fNN-*` correspondente e, se a decisão for arquitetural, registre um ADR.

## Workspace e CI

Rode comandos Rust a partir de `code/` (são os do workflow `.github/workflows/rust.yml`):

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Os testes E2E ficam em `code/tests/*.sh` (mapa em `docs/implementation/transversal/evidencias-e2e.md`). Os que usam pesos de modelo exigem `code/models/{gpt2,smollm2,smollm2-instruct}` (ignorados pelo git).

## Layout

- `code/crates/common` — tipos de domínio compartilhados (`NodeId`, `WorkId`, `RequestId`, `Matrix`…).
- `code/crates/protocol` — fronteira gRPC/Protobuf gerada, metadados de autenticação e papéis de credencial.
- `code/crates/controller` — control plane: registry, work store, placement, canal de fatias, eventos, métricas.
- `code/crates/worker` — node agent: heartbeat, benchmark, execução de Work Units, `ShardStore`.
- `code/crates/transport` — trait e tipos de transporte.
- `code/crates/benchmark` — medição de capability; nunca decisões de placement.
- `code/crates/cli` — CLI do operador; não coloque execução de worker nela.
- `code/crates/engine` — motor de inferência em CPU (GPT-2, Llama/SmolLM2, tokenizador, KV cache, fatias).
- `code/crates/gateway` — Inference API HTTP (`/v1/chat/completions`, SSE).
- `code/proto/cluster.proto` — contrato de wire, pacote `ai.cluster.v1`.

## Convenções

- Rust 2024 e um crate focado por fronteira de control/data-plane; tipos compartilhados em `common`.
- Mensagens de wire só em `code/proto/cluster.proto`; `crates/protocol/build.rs` gera os bindings (não edite a saída em `target/`).
- Binários usam `#[tokio::main]` e retornam `Result<(), Box<dyn std::error::Error>>`.
- Commits no estilo Conventional Commit (`feat(inference): …`, `fix(controller): …`, `docs: …`).

## Armadilhas

- O workspace Cargo é `code/`; o CI o usa como diretório de trabalho.
- O build do protocolo usa `protoc-bin-vendored`; não dependa de `protoc` do host.
- `.gitignore` exclui `**/target/`, `.hermes/`, `code/models/` e `.env*` exceto `.env.example`.
- Imagem A/B (`code/os/image/`): o worker da imagem Alpine é o musl (`TARGET=armv7-unknown-linux-musleabihf bash code/scripts/build-armv7.sh`); sem `TARGET` sai o glibc e a imagem copia o musl antigo. A imagem só aceita release assinada: `release-keys.sh` gera a chave em `~/.aic` e `sign-release.sh` imprime os argumentos de `ai-cluster rollout`. Ver `docs/implementation/19-hardening-de-producao/f19-rollout-de-release.md`.
