<p align="center">
  <img src="docs/assets/banner.svg" alt="AI Cluster" width="100%">
</p>

<p align="center">
  <a href="https://github.com/julianscunha/Real.AI.Cluster/actions/workflows/rust.yml"><img alt="CI" src="https://github.com/julianscunha/Real.AI.Cluster/actions/workflows/rust.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="Licença" src="https://img.shields.io/badge/licen%C3%A7a-Apache--2.0-blue"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/rust-edition%202024-orange?logo=rust">
  <img alt="Plataformas" src="https://img.shields.io/badge/x86__64%20%7C%20ARMv7-testado-informational">
  <img alt="Estado" src="https://img.shields.io/badge/estado-pr%C3%A9--produ%C3%A7%C3%A3o-yellow">
  <a href="SECURITY.md"><img alt="Segurança" src="https://img.shields.io/badge/seguran%C3%A7a-pol%C3%ADtica-green"></a>
</p>

[English](README.en.md) · **Português**

# AI Cluster

**Transforme as máquinas paradas que você já tem (PCs velhos, notebooks, servidores, Raspberry/Orange Pi) em um único pool de computação para IA.**

Cada uma é fraca; juntas rodam um modelo. O AI Cluster admite os nós com segurança, mede o que cada um aguenta, divide o trabalho, **fatia os pesos do modelo entre as máquinas** e continua funcionando quando uma delas cai. Escrito em Rust, com API compatível com OpenAI.

<p align="center">
  <img src="docs/assets/architecture.svg" alt="Arquitetura: cliente, gateway, controller e workers" width="100%">
</p>

## Como funciona na prática

O AI Cluster é um sistema Linux com **papéis**. Você escolhe onde cada um roda:

<p align="center">
  <img src="docs/assets/deployment.svg" alt="Controller e gateway em um servidor, workers nos dispositivos, CLI no seu computador" width="100%">
</p>

| Papel | O que é | Onde roda | Quantos |
|---|---|---|---|
| **Controller** | Cérebro do cluster: registra os nós, distribui o trabalho, guarda estado e métricas. | Um servidor Linux que fica sempre ligado. | 1 |
| **Worker** | Executa o trabalho e guarda as fatias do modelo na memória. | **Cada dispositivo que você quer aproveitar**: PC, notebook, servidor, placa ARM. | quantos você tiver |
| **Gateway** | A API (`/v1/chat/completions`, compatível com OpenAI) que seus aplicativos chamam. | No mesmo servidor do controller (ou em outro Linux com acesso a ele). | 1 |
| **CLI `ai-cluster`** | Como você administra e monitora: listar nós, aprovar, atualizar. | Seu computador, ou o próprio servidor. | — |

Fluxo de uso:

1. **Instale o controller** no servidor e anote os dois tokens (join e operador).
2. **Transforme cada dispositivo em worker.** O caminho do projeto é o **AI Cluster OS**: um sistema próprio, imutável e enxuto que **substitui o sistema do dispositivo** (ele é formatado) e o deixa dedicado a ser worker, com atualização assinada e rollback automático. Hoje a imagem existe para **placas ARMv7**; para **x86_64 e ARM64 ela ainda está em desenvolvimento**, e nesse meio-tempo o worker pode ser instalado sobre o Linux que o dispositivo já tem (caminho de transição, sem formatar).
3. **Aprove** cada nó novo e **monitore** tudo pela CLI.
4. **Suba o gateway** com o modelo e aponte seus aplicativos para ele.

## Pré-requisitos

| Onde | Precisa de |
|---|---|
| **Servidor do controller/gateway** | Linux (x86_64 ou ARM, glibc), acesso de rede dos workers à porta `50051` e dos clientes à `8080`. O gateway com SmolLM2 mantém o modelo em memória (cerca de 1 GB). |
| **Cada worker** | **AI Cluster OS** (placa ARMv7: o cartão é apagado e regravado) **ou**, enquanto a imagem x86_64/ARM64 não existe, um Linux x86_64/ARMv7 já instalado com `root` e, de preferência, systemd. Nos dois casos: conexão de saída até a porta `50051` do controller e memória livre para a fatia do modelo. |
| **Máquina de build** (uma vez) | Linux com Rust estável ≥ 1.85 (`rustup`). Compila os binários; servidores e dispositivos não precisam de Rust. Para gerar binário ARMv7 a partir de x86, Docker nessa máquina. |
| **Seu computador** | O binário `ai-cluster` e acesso de rede ao controller. |
| **Modelos** | Pesos GPT-2 ou SmolLM2 no servidor do gateway ([onde baixar](docs/guia/instalacao.md#pesos-dos-modelos)). Opcional: sem eles o cluster funciona, só não responde chat. |

Docker **não** é requisito para usar o AI Cluster. Ele só aparece para quem desenvolve o projeto (veja o fim deste README).

## Instalação passo a passo

### 1. Compile os binários (uma vez)

Em uma máquina Linux de build:

```bash
git clone https://github.com/julianscunha/Real.AI.Cluster.git
cd Real.AI.Cluster/code
cargo build --release --workspace
# binários em target/release/: ai-cluster-controller, ai-cluster-gateway, ai-cluster-worker, ai-cluster
```

Copie `ai-cluster-controller` e `ai-cluster-gateway` para o servidor, `ai-cluster` para quem vai administrar e `ai-cluster-worker` para cada dispositivo **da mesma arquitetura**. Para placas ARMv7 compile com `TARGET=armv7-unknown-linux-musleabihf bash scripts/build-armv7.sh` (veja o [guia de instalação](docs/guia/instalacao.md)). Ainda não há binários pré-compilados para download.

### 2. Controller (no servidor)

```bash
export AI_CLUSTER_JOIN_TOKEN=$(openssl rand -hex 24)       # os workers usam para entrar
export AI_CLUSTER_OPERATOR_TOKEN=$(openssl rand -hex 24)   # você usa para administrar
echo "join=$AI_CLUSTER_JOIN_TOKEN operador=$AI_CLUSTER_OPERATOR_TOKEN"   # guarde os dois

mkdir -p /var/lib/aic
AI_CLUSTER_INSECURE=1 \
AI_CLUSTER_NODES=/var/lib/aic/nodes AI_CLUSTER_STATE=/var/lib/aic/state.bin AI_CLUSTER_EVENTS=/var/lib/aic/events.log \
./ai-cluster-controller        # escuta em 0.0.0.0:50051
```

> `AI_CLUSTER_INSECURE=1` desliga o TLS: use só em rede local confiável. Em qualquer outra rede configure o TLS (certificado do controller e `AI_CLUSTER_TLS_CA` nos workers): [`docs/guia/instalacao.md`](docs/guia/instalacao.md#produção-com-tls). Para manter o controller ligado após reinício, rode-o como serviço do systemd.

### 3. Worker (em cada dispositivo)

**Caminho recomendado: AI Cluster OS (placas ARMv7).** O dispositivo é formatado e passa a rodar o sistema do projeto, já com o worker, o controller configurado e atualização A/B assinada. Veja o passo 6.

**Caminho de transição: worker sobre o Linux existente.** É o único disponível hoje para desktops e servidores x86_64 e para ARM64, enquanto as imagens desses não existem. Nada é formatado, e o dispositivo continua um Linux comum: sem atualização A/B nem rollback de boot. Copie `ai-cluster-worker`, `code/os/bootstrap.sh` e o token de join para o dispositivo e, como root:

```bash
echo '<token-de-join>' > /root/join-token && chmod 600 /root/join-token
sudo ./bootstrap.sh --controller http://<ip-do-servidor>:50051 --insecure \
  --join-token-file /root/join-token --binary ./ai-cluster-worker --node-id sala-pc1
```

O script cria o usuário, instala o worker como serviço do systemd e confirma que ele se registrou. (`--insecure` só porque o exemplo usa `http://`; com TLS use `https://` e a CA.)

### 4. Aprove e monitore (CLI)

```bash
export AI_CLUSTER_CONTROLLER=http://<ip-do-servidor>:50051 AI_CLUSTER_OPERATOR_TOKEN=<token-de-operador>
ai-cluster nodes             # o nó novo aparece, pendente
ai-cluster approve sala-pc1  # só nós aprovados recebem trabalho
ai-cluster dashboard --watch 2
ai-cluster events            # entradas, saídas, falhas
```

<p align="center">
  <img src="docs/assets/terminal-nodes.svg" alt="Saída real do ai-cluster nodes com um PC x86 e um Orange Pi" width="100%">
</p>

### 5. Gateway e inferência

No servidor, com os pesos do SmolLM2 em `/srv/models/smollm2`:

```bash
AI_CLUSTER_INSECURE=1 AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 \
AI_CLUSTER_CONTROLLER=http://127.0.0.1:50051 AI_CLUSTER_OPERATOR_TOKEN=<token-de-operador> \
AI_CLUSTER_API_KEY=<chave-dos-seus-apps> \
AI_CLUSTER_SMOLLM2_DIR=/srv/models/smollm2 \
./ai-cluster-gateway
```

```bash
curl http://<ip-do-servidor>:8080/v1/chat/completions \
  -H "Authorization: Bearer <chave-dos-seus-apps>" -H "Content-Type: application/json" \
  -d '{"model":"smollm2-135m","messages":[{"role":"user","content":"Hello!"}],"max_tokens":16}'
```

Para **dividir o modelo entre os workers**, instale os pesos também neles (`--model-dir` do `bootstrap.sh`) e inicie o gateway com `AI_CLUSTER_SMOLLM2_SHARDS=sala-pc1,sala-pc2`: cada worker guarda uma fatia na memória e o gateway soma os resultados parciais.

<p align="center">
  <img src="docs/assets/inference-split.svg" alt="Uma camada do modelo dividida entre três workers" width="100%">
</p>

Todas as variáveis, tenants, cotas e a API completa: [`docs/guia/configuracao.md`](docs/guia/configuracao.md) e [`docs/guia/api.md`](docs/guia/api.md).

### 6. AI Cluster OS: o dispositivo vira um worker dedicado

O **AI Cluster OS** é o destino do projeto para qualquer dispositivo: uma imagem Alpine imutável, com dois slots (A/B), que apaga o sistema anterior e deixa a máquina só com o que um worker precisa. O operador assina a atualização; o controller a anuncia, um nó canário atualiza primeiro e os demais seguem em ondas. Se o nó novo não ficar saudável, o boot volta ao slot anterior sozinho.

**Disponibilidade hoje:** placas **ARMv7** (testado em Orange Pi PC). **Ainda não existe imagem para x86_64 (UEFI) nem ARM64**; elas são o próximo passo do projeto (veja [`pendencias.md`](docs/implementation/pendencias.md)). Até lá, desktops e servidores usam o caminho de transição do passo 3.

<p align="center">
  <img src="docs/assets/os-update.svg" alt="Fluxo de atualização assinada com slots A/B" width="100%">
</p>

Gerar a imagem, gravar o cartão e publicar releases: [`docs/guia/instalacao.md`](docs/guia/instalacao.md) e [`docs/guia/operacao.md`](docs/guia/operacao.md).

## O que você ganha

- **Hardware de qualquer arquitetura:** x86_64 e ARM no mesmo cluster, sem GPU obrigatória.
- **Inferência distribuída de verdade:** os pesos ficam residentes nos workers; só passam resultados parciais. Se um worker cai, o trabalho dele é reatribuído na hora.
- **Nós entram e saem sem cerimônia:** join com token, aprovação, drain, saída limpa, quarentena automática de nó que erra.
- **Segurança por papéis:** credenciais separadas para entrar, operar e inferir; TLS e mTLS; o gateway roda sem o token de operador.
- **Atualização segura do sistema:** release assinada (Ed25519), canário, ondas e rollback automático.
- **Observável:** métricas Prometheus em `:9101`, eventos, traces de requisição e painel no terminal.

## Estado do projeto

Pré-produção: o núcleo (cluster, placement, inferência distribuída, segurança por papéis, imagem A/B com release assinada) roda e é testado em PC x86 e Orange Pi (ARMv7). O que ainda falta está em [`docs/implementation/pendencias.md`](docs/implementation/pendencias.md): aceleradores (GPU/NPU), worker móvel, revogação de chave e *attestation* por hardware, entre outros.

## Documentação

| Para | Veja |
|---|---|
| Instalar, configurar e operar | [`docs/guia/`](docs/guia/guia.md) |
| Entender como funciona e como é estruturado | [`docs/system/`](docs/system/system.md) (arquitetura, protocolo, modelo de dados, decisões) |
| Contribuir com código | [`CONTRIBUTING.md`](CONTRIBUTING.md) e [`AGENTS.md`](AGENTS.md) |

## Estrutura do repositório

```
code/
  crates/{common,protocol,controller,worker,transport,benchmark,cli,engine,gateway}
  proto/cluster.proto      contrato de comunicação (ai.cluster.v1)
  os/                      bootstrap.sh, imagem A/B, netboot, instalador
  scripts/ tests/          build ARM, cluster de desenvolvimento, testes de ponta a ponta
docs/                      guia/, system/, implementation/ (pendências)
```

## Para quem desenvolve o projeto

Há um cluster de desenvolvimento em contêineres Docker, que roda tudo na sua máquina para testar mudanças. **Não é o jeito de usar o AI Cluster**: é ferramenta de desenvolvimento (sem TLS, tokens fixos).

```bash
cd code
bash scripts/dev-cluster.sh build && bash scripts/dev-cluster.sh up 2   # controller + 2 workers
bash scripts/dev-cluster.sh cli nodes
bash scripts/dev-cluster.sh gateway                                     # API em 127.0.0.1:8080, chave dev-key
bash scripts/dev-cluster.sh down
cargo test --workspace --all-features
```

Mais em [`CONTRIBUTING.md`](CONTRIBUTING.md) e [`docs/guia/testes.md`](docs/guia/testes.md).

## Contribua

Dê uma ⭐ se a ideia te interessa, abra uma issue com ideias ou problemas e leia o [`CONTRIBUTING.md`](CONTRIBUTING.md). Vulnerabilidades: [`SECURITY.md`](SECURITY.md) (relato privado, não abra issue). Conduta: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

Licença: [Apache-2.0](LICENSE).
