# Validação local

## Pré-requisitos

- Rust estável com Cargo;
- Git;
- Linux para o primeiro teste de integração;
- rede IP entre Controller e Workers.

## Configuração do teste

1. Iniciar o Controller em um Linux:
   `cargo run -p real-ai-cluster-controller`
2. Iniciar um Worker:
   `RAC_CONTROLLER_ENDPOINT=http://IP_DO_CONTROLLER:50051 cargo run -p real-ai-cluster-worker`
3. Consultar:
   `cargo run -p real-ai-cluster-cli -- --controller http://IP_DO_CONTROLLER:50051 nodes`
4. Encerrar o Worker e aguardar o timeout do heartbeat.
5. Confirmar que o Controller marcou o node como OFFLINE.

## Observação

O ambiente do projeto deve ser validado com `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features` e `cargo test --workspace` antes de considerar a fase validada.
