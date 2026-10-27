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

Cada uma é fraca; juntas rodam um modelo. O AI Cluster descobre e admite os nós com segurança, mede o que cada um aguenta, divide o trabalho, **fatia os pesos do modelo entre as máquinas** e continua funcionando quando uma delas cai. Escrito em Rust, com API compatível com OpenAI.

<p align="center">
  <img src="docs/assets/architecture.svg" alt="Arquitetura: cliente, gateway, controller e workers" width="100%">
</p>

## Por que usar

- **Reaproveita hardware de qualquer arquitetura:** x86_64 e ARM no mesmo cluster, sem GPU obrigatória.
- **Inferência distribuída de verdade:** os pesos ficam residentes nos workers; só passam resultados parciais. Se um worker cai, as faixas dele são reatribuídas na hora.
- **Nós entram e saem sem cerimônia:** join com token, aprovação, drain, saída limpa, quarentena automática de nó que erra.
- **Segurança por papéis:** credenciais separadas para entrar, operar e inferir; mTLS opcional; o gateway roda sem o token de operador.
- **Sistema operacional próprio para SBCs:** imagem Alpine A/B com rollback automático, **atualização assinada (Ed25519)** e rollout com canário feito pelo controller.
- **Observável:** métricas Prometheus, eventos, traces de requisição e painel no terminal.

<p align="center">
  <img src="docs/assets/terminal-nodes.svg" alt="Saída real do ai-cluster nodes com um PC x86 e um Orange Pi" width="100%">
</p>

## O que você precisa

| Item | Requisito |
|---|---|
| **Onde rodar os comandos** | No seu computador, **dentro da pasta `code/` do repositório clonado**. Os comandos abaixo levantam um cluster de teste em contêineres Docker na própria máquina; você não precisa de várias máquinas para experimentar. |
| **Sistema** | Linux, macOS ou Windows (com Docker Desktop). No Windows use o Git Bash ou o PowerShell. |
| **Docker** | Docker Engine ou Docker Desktop em execução. |
| **RAM / disco** | ~4 GB livres de RAM e ~6 GB de disco para a compilação. |
| **Rust** | Só se for compilar sem Docker: Rust estável ≥ 1.85 (edition 2024). `protoc` já vem embutido. |
| **Modelos de IA** | Opcionais. A inferência com GPT-2 ou SmolLM2 precisa dos pesos em `code/models/` ([como obter](docs/guia/instalacao.md)); o cluster, a CLI e o benchmark funcionam sem eles. |
| **Rede** | Em cluster real, os nós precisam alcançar a porta do controller (padrão `50051`). |

## Comece em 5 minutos (uma máquina, sem hardware extra)

```bash
git clone https://github.com/julianscunha/Real.AI.Cluster.git
cd Real.AI.Cluster/code                    # todos os comandos a seguir rodam aqui

bash scripts/dev-cluster.sh build          # compila (alguns minutos na primeira vez)
bash scripts/dev-cluster.sh up 2           # sobe 1 controller + 2 workers em contêineres
bash scripts/dev-cluster.sh cli nodes      # lista os nós e a saúde de cada um
bash scripts/dev-cluster.sh cli dashboard --watch 2
bash scripts/dev-cluster.sh cli benchmark --size 256 --repetitions 8
bash scripts/dev-cluster.sh gateway       # (opcional) API em 127.0.0.1:8080; exige os pesos em code/models/smollm2
bash scripts/dev-cluster.sh down           # desliga tudo
```

No Windows (PowerShell): `scripts\dev-cluster.ps1` com os mesmos argumentos.

> O cluster de desenvolvimento roda **sem TLS e com tokens fixos**. É só para a sua máquina; nunca o exponha em uma rede aberta. Para produção veja [`docs/guia/operacao.md`](docs/guia/operacao.md).

Quer rodar os testes? `cd code && cargo test --workspace --all-features` (ou veja [`docs/guia/testes.md`](docs/guia/testes.md)).

## Cluster de verdade: várias máquinas

1. Em uma máquina, rode o **controller**; nas outras, o **worker** apontando para ele (`AI_CLUSTER_CONTROLLER=http://<ip-do-controller>:50051`).
2. Cada nó novo entra com o *join token* e é aprovado por você: `ai-cluster approve <nó>`.
3. Acompanhe com `ai-cluster nodes`, `dashboard` e `events`.

Passo a passo, TLS, tokens e todas as variáveis de ambiente: [`docs/guia/instalacao.md`](docs/guia/instalacao.md) e [`docs/guia/configuracao.md`](docs/guia/configuracao.md).

## Inferência distribuída

O gateway expõe `POST /v1/chat/completions` (com SSE), `GET /v1/models` e `/metrics`. Cada projeção de peso é dividida entre os workers, que a mantêm na memória.

<p align="center">
  <img src="docs/assets/inference-split.svg" alt="Uma camada do modelo dividida entre três workers" width="100%">
</p>

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Authorization: Bearer dev-key" -H "Content-Type: application/json" \
  -d '{"model":"smollm2-135m","messages":[{"role":"user","content":"Hello!"}],"max_tokens":16}'
```

Com `dev-cluster.sh gateway` a chave é `dev-key` e o modelo leva cerca de 30 s para carregar. Detalhes da API, modelos suportados e como ligar as fatias nos workers: [`docs/guia/api.md`](docs/guia/api.md).

## AI Cluster OS: atualização segura de nós

Em placas ARM o projeto traz uma imagem imutável com dois slots (A/B). O operador assina a release; o controller a anuncia, um nó canário atualiza primeiro e os demais seguem em ondas. Se o nó novo não ficar saudável, o boot volta ao slot anterior sozinho.

<p align="center">
  <img src="docs/assets/os-update.svg" alt="Fluxo de atualização assinada com slots A/B" width="100%">
</p>

Instalação em placa e rollout: [`docs/guia/operacao.md`](docs/guia/operacao.md). Os limites de segurança conhecidos estão em [`SECURITY.md`](SECURITY.md).

## Estado do projeto

Pré-produção: o núcleo (cluster, placement, inferência distribuída, segurança por papéis, imagem A/B com release assinada) roda e é testado em PC x86 e Orange Pi (ARMv7). O que ainda falta está em [`docs/implementation/pendencias.md`](docs/implementation/pendencias.md): aceleradores (GPU/NPU), worker móvel, revogação de chave e *attestation* por hardware, entre outros.

## Documentação

| Para | Veja |
|---|---|
| Instalar, configurar e operar | [`docs/guia/`](docs/guia/guia.md) |
| Entender como funciona e como é estruturado | [`docs/system/`](docs/system/system.md) (arquitetura, protocolo, modelo de dados, decisões) |
| Contribuir com código | [`CONTRIBUTING.md`](CONTRIBUTING.md) e [`AGENTS.md`](AGENTS.md) |

A pasta `docs/` é um *vault* do Obsidian, mas lê-se bem no GitHub.

## Estrutura do repositório

```
code/
  crates/{common,protocol,controller,worker,transport,benchmark,cli,engine,gateway}
  proto/cluster.proto      contrato de comunicação (ai.cluster.v1)
  os/                      imagem A/B, netboot, instalador
  scripts/ tests/          cluster de desenvolvimento, testes de ponta a ponta
docs/                      guia/, system/, implementation/ (pendências)
```

## Contribua

Dê uma ⭐ se a ideia te interessa, abra uma issue com ideias ou problemas e leia o [`CONTRIBUTING.md`](CONTRIBUTING.md). Vulnerabilidades: [`SECURITY.md`](SECURITY.md) (relato privado, não abra issue). Conduta: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

Licença: [Apache-2.0](LICENSE).
