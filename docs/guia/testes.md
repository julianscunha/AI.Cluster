---
title: "Testes"
aliases: ["Rodar testes", "Suíte E2E", "Testes unitários"]
tags: [guia, testes]
type: guia
---

# Testes

Volte ao [[guia]] para o índice. Pré-requisitos de build estão em [[instalacao]].

## Checagens de código

Rode a partir de `code/`. São as mesmas do CI (`.github/workflows/rust.yml`):

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

O CI roda só essas quatro. Os scripts E2E não estão no CI.

Um teste ou um crate:

```bash
cargo test -p ai-cluster-controller <filtro>
cargo test -p ai-cluster-gateway <filtro>
```

Os nomes de pacote são `ai-cluster-common`, `ai-cluster-protocol`, `ai-cluster-controller`, `ai-cluster-worker`, `ai-cluster-engine`, `ai-cluster-gateway`, `ai-cluster-cli` e `ai-cluster-benchmark`.

## Paridade numérica

Três testes em `code/crates/engine/tests/` comparam a saída do motor com a do `transformers`, usando fixtures já geradas:

| Teste | Variável com o diretório dos pesos |
|---|---|
| `gpt2_parity.rs` | `AI_CLUSTER_GPT2_DIR` |
| `smollm2_parity.rs` | `AI_CLUSTER_SMOLLM2_DIR` |
| `smollm2_chat_parity.rs` | `AI_CLUSTER_SMOLLM2_INSTRUCT_DIR` |
| `qwen2_parity.rs` | `AI_CLUSTER_QWEN2_DIR` (marcado `#[ignore]`: rode com `cargo test --release -p ai-cluster-engine --test qwen2_parity -- --ignored --test-threads=1`) |

Sem a variável, o teste avisa e **passa sem verificar nada** (o do Qwen2 fica `#[ignore]`, então a ausência aparece no relatório). O Qwen2.5-0.5B inteiro não cabe na RAM do motor (pesos em `f64`, ~4 GB): use um diretório com `num_hidden_layers` reduzido para 4 e gere o fixture com o mesmo diretório (`reference/smollm2_reference.py`). Para rodar de verdade, baixe os pesos para `code/models/gpt2`, `code/models/smollm2` e `code/models/smollm2-instruct` (a pasta não vai para o git) e aponte a variável para o diretório. O `gpt2` tem cerca de 500 MB.

## Suíte E2E

Os scripts ficam em `code/tests/`. Cada um sobe um controller e workers reais em contêineres, numa rede Docker própria, e derruba tudo ao fim. Rode a partir de `code/`:

```bash
bash tests/phase-s4-tls.sh
```

Pré-requisitos:

1. Docker.
2. Binários compilados no volume `aic-target`. Use `bash scripts/dev-cluster.sh build`, que compila no volume com a imagem `AIC_RUST_IMAGE`.
3. A imagem `aic-rust:1` que os scripts usam. O repositório não traz o Dockerfile dela: monte-a a partir de uma imagem Rust com as ferramentas do build.
4. Para os scripts `phase-j-*`, os pesos em `code/models/`.

Requisitos extras:

- `phase-c-network-degrade.sh` instala `iproute2` no contêiner para estrangular o link com `tc`.
- `phase-r-lan.sh` não sobe nada: confere um cluster físico que já está de pé, com nós de duas arquiteturas.
- `phase-u-netboot.sh` precisa de uma placa armv7 em bancada, com boot pela rede.
- A atualização assinada (rollout) é um teste de bancada em placa armv7, feito à mão. Não há script para ele.

### Soak

`phase-t-soak.sh` roda rodadas de falha (kill ou pause de um worker em rodízio, com lote em andamento). `ROUNDS` controla a duração (padrão 6, cerca de 3 minutos). `UNITS` controla o tamanho do lote (padrão 6). Para um soak longo, use `ROUNDS=200`.

## Mapa de scripts

O que cada grupo prova. A lista completa e os critérios estão nos comentários de cada script.

| Script | O que prova |
|---|---|
| `phase-s1-join-token.sh` | Controller não sobe sem token; nó sem token ou com token errado não aparece; CLI sem token é recusada. |
| `phase-m-catalog.sh` | Catálogo de modelos e OK/NOK: `model list` lê os cabeçalhos, `model check` cabe ou não cabe com o que falta. Precisa dos pesos em `code/models`. |
| `phase-r-llama-rpc.sh` | Um GGUF no llama.cpp dividido entre gateway e worker por RPC sobre mTLS: pronto, chat e stream, 401/404, o rpc-server escondido e o túnel exigindo certificado. Precisa de `/t/llama/{llama-server,ggml-rpc-server,smollm2-360m-q4.gguf}`. |
| `phase-s-resistance.sh [minutos]` | Resistência: carga contínua em 2 workers com partição de rede de um deles, controller reiniciado sob carga (SIGTERM e `kill`) e soak com memória amostrada a cada 30 s (falha se o controller ou um worker cresce mais de 60 MiB e 50%). Medido em 1 minuto de soak: 572 unidades, 0 falhas, controller 8 a 14 MiB, workers 147 a 148 MiB. |
| `phase-r2-model-start.sh` | O fluxo `model list`, `check`, `start`, `status`, `stop`: chave de API só em hash, plano de workers, worker novo entrando no plano, mesma chave após o stop, várias chaves (`add`, `list`, `rotate`, `remove` com cota e classe) e worker de outra versão do rpc-server fora do plano. |
| `phase-s2-approval.sh` | Nó novo fica pendente sem trabalho até `approve`; a aprovação sobrevive a re-registro. |
| `phase-s3-roles.sh` | Token de join e de inferência não administram; operador administra; ações de operador entram na auditoria. |
| `phase-s4-tls.sh` | gRPC com TLS e CA; `http://` e CA errada são recusados; sem certificado, só com `INSECURE`. |
| `phase-s5-persist.sh` | Aprovação sobrevive a reinício de worker e de controller; `revoke` derruba o worker revogado. |
| `phase-t-mtls.sh` | Com CA de cliente, só entra quem tem certificado dessa CA; sem certificado, com outra CA ou com meio par, é recusado. |
| `phase-1-leave.sh` | Saída por SIGTERM: o sobrevivente termina cada unidade, sem nada encalhado. |
| `phase-2-failure.sh` | Worker morto com unidade em voo: a unidade volta à fila e o grafo termina. |
| `phase-2-graph.sh` | Diamante A → (B, C) → D: B e C só despacham depois de A; D depois de B e C. |
| `phase-g-placement.sh` | O custo escolhe o nó capaz, não o fraco. |
| `phase-g-quarantine.sh` | Worker que mente no checksum vai para `QUARANTINED` e o trabalho termina no honesto. |
| `phase-g-reservations.sh` | Nó reservado recusa trabalho de outras requisições e roda o da dona. |
| `phase-f11-cancel.sh` | `request cancel` para a multiplicação que já roda no worker: o aviso chega pela batida, a conta é interrompida e nenhum resultado é enviado. |
| `phase-g-slots.sh`, `phase-g-cores.sh`, `phase-g-priority.sh` | Slots de concorrência, cores concedidos e prioridade de unidade. |
| `phase-c-benchmark.sh`, `phase-c-current.sh` | Perfil com compute medido e capacidade corrente com confiança. |
| `phase-c4-health.sh`, `phase-c-storage-health.sh`, `phase-c-battery.sh` | Nó quente, com disco degradado ou com bateria descarregando tem saúde pior, e a unidade vai para o nó saudável. |
| `phase-h-metrics.sh`, `phase-h-dashboard.sh`, `phase-h-traces.sh` | Roll-up de métricas, painel e rastreio de requisição por `request_id`. |
| `phase-h-predictions.sh` | Previsão de tempo por unidade e no roll-up. |
| `phase-h-inference-metrics.sh` | TTFT, ITL, tokens por segundo e cache KV no `/metrics` do gateway. |
| `phase-i-state.sh`, `phase-i-checkpoint.sh`, `phase-i-events.sh` | Fila sobrevive a queda abrupta; o snapshot é proporcional ao trabalho que falta; eventos atravessam reinício. |
| `phase-i4-recovery.sh`, `phase-i4-replica.sh` | Unidade em voo atravessa reinício do controller; fila volta da cópia de um nó quando os arquivos locais somem. |
| `phase-t-soak.sh` | Rodízio de kill e pause sob carga: todo lote fecha e o cluster volta a todos os nós. |
| `phase-j-inference.sh`, `phase-j-chat.sh` | Autenticação por chave, modelos, embeddings sobre matmul, chat com continuação determinística, erros estáveis. |
| `phase-s-gateway-tls.sh` | Gateway com certificado serve HTTPS e recusa texto puro; certificado sem chave, ou escuta fora de loopback sem TLS, não sobe. |
| `phase-j-shards.sh` | Projeções da SmolLM2 em dois workers dão a mesma continuação; sem worker, 503. |
| `phase-m-request-manager.sh` | ACL e 401 por tenant; cota com 429; `interactive` passa na frente de `batch`; grafo abandonado é cancelado. |
| `phase-o-cache.sh` | Pedido repetido vem do cache, com ganho de latência, também no streaming, e as métricas contam os acertos. |
| `phase-p-bootstrap.sh` | Um Debian limpo vira nó pronto só com o binário e `os/bootstrap.sh`; argumentos inválidos são recusados; a execução é idempotente. |
| `phase-p-image.sh` | A imagem mínima do worker é construída do zero e vira nó pronto. |
| `phase-q-learning.sh` | O modelo aprendido corrige a previsão de um nó mal calibrado em poucas dezenas de unidades. |
| `phase-b4-node-show.sh`, `phase-i-storage.sh` | `node show` completo; classe de armazenamento detectada a partir de `/sys/block`. |
| `phase-u-netboot.sh` | Boot pela rede em placa armv7: o nó baixa a recuperação por TFTP e volta ao cartão. Exige bancada. |
| `phase-r-lan.sh` | Cluster físico com x86 e ARM: lote em mais de um nó e checksum igual entre arquiteturas. Exige a LAN montada. |
| `scaling-sweep.sh` | 1, 2 e 3 workers com tamanhos de 32 a 256: custo de compute contra transferência por unidade. |

`os/test-upgrade.sh` (na pasta `os/`) testa atualização e rollback do worker em um Linux com systemd. Roda na máquina de destino, como root.
