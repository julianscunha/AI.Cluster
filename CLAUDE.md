# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`AGENTS.md` na raiz já traz layout dos crates, convenções e armadilhas; leia-o primeiro. Este arquivo só acrescenta comandos e a visão de conjunto. Idioma do repositório (docs, commits, comentários): pt-br.

## Comandos

Rode tudo a partir de `code/` (workspace Cargo; o CI usa esse diretório):

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test -p <crate> <filtro>            # um teste / um crate
bash tests/<script>.sh                    # E2E (sobe controller + workers; mapa em docs/guia/testes.md)
scripts/dev-cluster.sh                    # cluster local de desenvolvimento (.ps1 no Windows)
```

- Testes de inferência com modelo exigem pesos em `code/models/{gpt2,smollm2,smollm2-instruct}` (ignorados pelo git).
- `protoc` vem de `protoc-bin-vendored`; mensagens de wire só em `code/proto/cluster.proto`.

## Arquitetura (visão de conjunto)

- **Control plane × data plane.** `controller` guarda registry de nós, work store, placement por custo, reservas, eventos e métricas, e expõe o gRPC `ClusterControlService`. `worker` é o node agent: registra-se (join token), manda heartbeat, roda benchmark de capability e executa Work Units. `benchmark` só mede; decisão de placement fica no controller.
- **Inferência.** `gateway` (HTTP, `/v1/chat/completions` com SSE) roda o modelo via `engine` (GPT-2, Llama/SmolLM2, tokenizador, KV cache). Apenas as projeções de peso são distribuídas: o `engine` fatia os pesos (`Slice`, `ShardStore`), os workers os mantêm residentes e atendem `ShardMatmul`; o gateway reatribui faixas se um worker cai. A paridade numérica contra `transformers` é critério de aceite.
- **Segurança por papéis.** Três credenciais em `protocol` (join, operador, inferência); o papel de inferência só chama `ShardMatmul`, e o gateway roda sem o token de operador.
- **Docs são parte do código.** o código é a verdade do que existe; ao mudar código, atualize `docs/system/` e `docs/guia/` (o que falta fica em `docs/implementation/pendencias.md`) e registre ADR se a decisão for arquitetural (convenções em `docs/convencoes.md`).
- Itens de fase ainda não iniciados (F11 em diante, p.ex. Request Manager) não têm código; `crates/transport` é só esqueleto (trait `Transport`).
