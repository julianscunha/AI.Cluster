# Real.AI.Cluster

Runtime distribuído e heterogêneo para computação de IA.

O Real.AI.Cluster transforma máquinas Linux com arquiteturas e capacidades diferentes em um conjunto de Workers coordenado por um Controller. O sistema mede capacidade e conectividade, registra os Workers e distribui unidades de trabalho entre os nós elegíveis. A implementação atual é experimental e ainda não é uma plataforma pronta para produção.

## Como funciona

```text
Aplicação / CLI
      |
      | gRPC
      v
  Controller :50051
      |
      +---- Worker :50052
      +---- Worker :50052
      +---- Worker :50052
```

- **Controller:** mantém o registro dos nós, recebe heartbeats, planeja a alocação e encaminha trabalhos.
- **Worker:** registra-se no Controller, detecta parte do hardware, executa benchmarks locais, reporta métricas e recebe trabalhos por gRPC.
- **CLI:** consulta os nós, envia uma unidade de trabalho e executa benchmarks de multiplicação matricial distribuída.
- **Protocolo:** comunicação gRPC definida em `crates/protocol`.

A implementação inclui execução inicial de Work Units e grafos, multiplicação matricial distribuída e mecanismos experimentais de armazenamento e transferência de buffers. Consulte [docs/STATUS.md](docs/STATUS.md) para o estado atual e os itens ainda experimentais.

## Pré-requisitos

- Linux em cada máquina que executará Controller ou Worker.
- Rust estável e Cargo (edição Rust 2021).
- Acesso à rede entre os processos: Workers precisam alcançar o Controller, e o Controller precisa alcançar os endpoints anunciados pelos Workers.

Instale Rust/Cargo com o instalador oficial do [rustup](https://rustup.rs/). Não há etapa separada para instalar as dependências Rust do projeto: Cargo as baixa durante o build.

Verifique a instalação:

```bash
rustc --version
cargo --version
```

## Obter o código e compilar

```bash
git clone https://github.com/julianscunha/Real.AI.Cluster.git
cd Real.AI.Cluster

# Compilar todos os crates
cargo build --workspace

# Validar formatação, compilação, lint e testes
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Para compilar binários otimizados:

```bash
cargo build --release --workspace
```

Os binários ficam em `target/release/`; durante desenvolvimento, Cargo os executa a partir do workspace sem exigir instalação global.

## Executar localmente

Abra terminais separados na raiz do repositório.

**1. Inicie o Controller:**

```bash
cargo run -p real-ai-cluster-controller
```

Por padrão, o Controller escuta em `0.0.0.0:50051`. Para alterar o endereço:

```bash
RAC_CONTROLLER_ADDR=0.0.0.0:50051 cargo run -p real-ai-cluster-controller
```

**2. Inicie um Worker:**

```bash
RAC_CONTROLLER_ENDPOINT=http://127.0.0.1:50051 \
RAC_WORKER_ADDR=0.0.0.0:50052 \
RAC_WORKER_ENDPOINT=http://127.0.0.1:50052 \
cargo run -p real-ai-cluster-worker
```

O Worker registra-se no Controller, executa benchmarks iniciais e envia heartbeats periódicos. O processo permanece ativo enquanto reporta saúde.

**3. Consulte os nós usando a CLI:**

```bash
cargo run -p real-ai-cluster-cli -- --controller http://127.0.0.1:50051 nodes
```

Envie uma unidade de trabalho:

```bash
cargo run -p real-ai-cluster-cli -- --controller http://127.0.0.1:50051 work run
```

Execute uma multiplicação matricial distribuída:

```bash
cargo run -p real-ai-cluster-cli -- --controller http://127.0.0.1:50051 benchmark distributed-matmul
```

Compare escalabilidade com diferentes quantidades de Workers:

```bash
cargo run -p real-ai-cluster-cli -- --controller http://127.0.0.1:50051 benchmark scaling --worker-counts 1,2,3
```

Use `--help` em qualquer nível da CLI para ver as opções disponíveis.

## Executar em várias máquinas

1. Inicie o Controller em uma máquina e permita conexões TCP na porta configurada (por padrão, `50051`).
2. Em cada máquina Worker, defina `RAC_CONTROLLER_ENDPOINT` com o endereço alcançável do Controller.
3. Defina `RAC_WORKER_ADDR` como o endereço local de escuta e `RAC_WORKER_ENDPOINT` como o endereço que o Controller consegue alcançar. O endpoint anunciado não deve ser `127.0.0.1` quando o Controller estiver em outra máquina.
4. Permita conexões TCP dos clientes para o Controller e do Controller para cada Worker (por padrão, porta `50052`).
5. Execute a CLI apontando para o endereço do Controller.

Exemplo em um Worker remoto (substitua os endereços pelos da sua rede):

```bash
RAC_CONTROLLER_ENDPOINT=http://192.0.2.10:50051 \
RAC_WORKER_ADDR=0.0.0.0:50052 \
RAC_WORKER_ENDPOINT=http://192.0.2.21:50052 \
cargo run --release -p real-ai-cluster-worker
```

Para mais de um Worker na mesma máquina, cada processo precisa de uma porta de escuta e um endpoint anunciados distintos.

## Configuração disponível

| Variável | Componente | Padrão | Finalidade |
|---|---|---|---|
| `RAC_CONTROLLER_ADDR` | Controller | `0.0.0.0:50051` | Endereço local de escuta do Controller |
| `RAC_CONTROLLER_ENDPOINT` | Worker | `http://127.0.0.1:50051` | Endpoint gRPC do Controller |
| `RAC_WORKER_ADDR` | Worker | `0.0.0.0:50052` | Endereço local de escuta do Worker |
| `RAC_WORKER_ENDPOINT` | Worker | `http://127.0.0.1:<porta>` | Endpoint anunciado ao Controller |
| `RAC_NODE_ID` | Worker | hostname | Identificador do nó |
| `RAC_DEVICE_ID` | Worker | `device-<node-id>` | Identificador do dispositivo |
| `RAC_OS_VERSION` | Worker | `bootstrap` | Versão informada pelo Worker |
| `RAC_BUFFER_STORE_BYTES` | Worker | 134217728 | Capacidade do buffer store local em bytes |

## Deploy e limitações atuais

O repositório não fornece, neste momento, imagens Docker, manifests Kubernetes, pacotes do sistema operacional nem unidades systemd. O deploy suportado/documentado aqui é compilar com Cargo e executar os processos manualmente. Para manter processos em produção, configure um supervisor de serviços e política de reinício no ambiente escolhido, após validar a segurança e a confiabilidade da instalação.

A comunicação gRPC descrita neste README não configura TLS nem autenticação por si só. Não exponha as portas em redes não confiáveis sem adicionar controles de acesso e proteção de transporte apropriados. Persistência durável do estado do cluster e operação de produção ainda exigem trabalho adicional.

## Documentação

- [Estado da implementação](docs/STATUS.md)
- [Protocolo de calibração da confiabilidade dos enlaces](docs/peer-link-reliability-calibration.md)
- [Plano de implementação](implementation/)
